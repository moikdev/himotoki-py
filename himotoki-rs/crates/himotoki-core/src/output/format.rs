//! JSON/text output formatting — port of `himotoki/output/format.py`.

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::chars::romanize_word;
use crate::output::meanings::{
    conj_info_json, get_senses_json, get_senses_raw, get_senses_str,
    word_info_reading_str,
};
use crate::output::types::{special_conj_info, WordInfo, WordType};
use crate::output::word_info::fill_segment_path;

/// `word_info_gloss_json` — JSON dict for a WordInfo.
pub fn word_info_gloss_json(
    conn: &Connection,
    word_info: &WordInfo,
    root_only: bool,
) -> Value {
    let mut js = json!({
        "reading": word_info_reading_str(word_info),
        "text": word_info.text,
        "kana": word_info.kana_json(),
    });

    if word_info.score != 0 {
        js["score"] = json!(word_info.score);
    }

    if word_info.alternative {
        js["alternative"] = json!(word_info
            .components
            .iter()
            .map(|wi| word_info_gloss_json(conn, wi, root_only))
            .collect::<Vec<_>>());
        return js;
    }

    if !word_info.components.is_empty() {
        js["compound"] =
            json!(word_info.components.iter().map(|wi| wi.text.clone()).collect::<Vec<_>>());
        js["components"] = json!(word_info
            .components
            .iter()
            .map(|wi| word_info_gloss_json(conn, wi, root_only))
            .collect::<Vec<_>>());
        return js;
    }

    // Compound with only texts (no component WordInfos)
    if word_info.is_compound {
        if let Some(seq) = word_info.first_seq() {
            js["seq"] = word_info.seq_json();
            let glosses: Vec<Value> = get_senses_raw(conn, seq)
                .into_iter()
                .map(|(_, gloss, props)| {
                    let pos_list = props.get("pos").cloned().unwrap_or_default();
                    let pos_str = if pos_list.is_empty() {
                        "[]".to_string()
                    } else {
                        format!("[{}]", pos_list.join(","))
                    };
                    json!({"pos": pos_str, "gloss": gloss, "props": props})
                })
                .collect();
            if !glosses.is_empty() {
                js["gloss"] = json!(glosses);
            }
        }
        if !word_info.compound_texts.is_empty() {
            js["compound"] = json!(word_info.compound_texts);
        }
        if let Some(conj_type) = &word_info.conj_type {
            let mut conj_prop = json!({"type": conj_type});
            if word_info.conj_neg {
                conj_prop["neg"] = json!(true);
            }
            if word_info.conj_fml {
                conj_prop["fml"] = json!(true);
            }
            let mut conj_entry = json!({"prop": [conj_prop], "readok": true});
            if let Some(st) = &word_info.source_text {
                conj_entry["reading"] = json!(st);
            }
            js["conj"] = json!([conj_entry]);
        }
        return js;
    }

    if let Some((value, ordinal)) = word_info.counter {
        js["counter"] = json!({"value": value, "ordinal": ordinal});
        if let Some(seq) = word_info.first_seq() {
            js["seq"] = word_info.seq_json();
            let gloss = get_senses_json(conn, seq, Some(&["ctr"]));
            if !gloss.is_empty() {
                js["gloss"] = json!(gloss);
            }
        }
        return js;
    }

    // Regular word
    if let Some(seq) = word_info.first_seq() {
        js["seq"] = word_info.seq_json();

        let conj_is_root = matches!(word_info.conjugations, Some(crate::types::Conj::Root));
        if root_only || word_info.conjugations.is_none() || conj_is_root {
            let gloss = get_senses_json(conn, seq, None);
            if !gloss.is_empty() {
                js["gloss"] = json!(gloss);
            }
        }

        // `has_conjugations = conjugations and != 'root'`
        let has_conjugations = match &word_info.conjugations {
            Some(crate::types::Conj::Ids(ids)) if !ids.is_empty() => true,
            _ => false,
        };
        let has_special_conj = special_conj_info(seq).is_some();

        if has_conjugations || has_special_conj {
            let ids: Option<Vec<i64>> = if has_conjugations {
                match &word_info.conjugations {
                    Some(crate::types::Conj::Ids(ids)) => Some(ids.clone()),
                    _ => None,
                }
            } else {
                None
            };
            let conj = conj_info_json(
                conn,
                seq,
                ids.as_deref(),
                word_info.true_text.as_deref(),
            );
            if !conj.is_empty() {
                js["conj"] = json!(conj);
            }
        }
    }

    js
}

/// `dict_segment` — segment + WordInfo fill, returns [(word_infos, score)].
pub fn dict_segment(
    conn: &Connection,
    text: &str,
    limit: usize,
) -> Vec<(Vec<WordInfo>, f64)> {
    let results = crate::segment::segment_text(conn, text, None, limit);
    results
        .into_iter()
        .map(|(path, score)| (fill_segment_path(conn, text, &path, true), score))
        .collect()
}

/// `simple_segment` — best-path WordInfos.
pub fn simple_segment(conn: &Connection, text: &str, limit: usize) -> Vec<WordInfo> {
    dict_segment(conn, text, limit)
        .into_iter()
        .next()
        .map(|(wis, _)| wis)
        .unwrap_or_default()
}

/// `segment_to_json` — [[segments, score]] ichiran-compatible.
pub fn segment_to_json(conn: &Connection, text: &str, limit: usize) -> Vec<Value> {
    let results = dict_segment(conn, text, limit);
    let mut output = Vec::new();
    for (word_infos, score) in results {
        let mut segments = Vec::new();
        for wi in &word_infos {
            let romanized = romanize_word(wi.kana_str_or_text());
            let segment_json = word_info_gloss_json(conn, wi, false);
            segments.push(json!([romanized, segment_json, []]));
        }
        // Python emits int scores; match by serializing integral floats as int.
        let score_json = crate::output::golden::num_json(score);
        output.push(json!([segments, score_json]));
    }
    output
}

/// `segment_to_text` — ichiran -i style formatted output.
pub fn segment_to_text(conn: &Connection, text: &str, limit: usize) -> String {
    let results = dict_segment(conn, text, limit);
    if results.is_empty() {
        return text.to_string();
    }
    let (word_infos, _score) = &results[0];

    let mut lines: Vec<String> = Vec::new();
    lines.push(
        word_infos
            .iter()
            .map(|wi| romanize_word(wi.kana_str_or_text()))
            .collect::<Vec<_>>()
            .join(" "),
    );

    for wi in word_infos {
        if wi.type_ == WordType::Gap {
            continue;
        }
        lines.push(String::new());
        let romanized = romanize_word(wi.kana_str_or_text());
        lines.push(format!("* {}  {}", romanized, word_info_reading_str(wi)));
        if wi.has_seq() {
            if let Some(seq) = wi.first_seq() {
                let senses = get_senses_str(conn, seq);
                lines.push(senses);
            }
        }
        for cs in crate::output::conjugation_display::get_conjugation_display(conn, wi) {
            lines.push(cs);
        }
    }
    lines.join("\n")
}

impl WordInfo {
    /// `wi.kana if str else wi.kana[0] if wi.kana else wi.text`.
    pub fn kana_str_or_text(&self) -> &str {
        if self.kana.is_empty() {
            &self.text
        } else {
            &self.kana[0]
        }
    }
}
