//! WordInfo construction — port of `himotoki/output/word_info.py`.

use rusqlite::Connection;

use crate::conj::get_word_conj_data;
use crate::constants::conj_type_name;
use crate::output::meanings::{
    get_matching_kana_for_kanji, has_conjugable_pos, populate_meanings,
    split_copula_compound_for_output, ReadingsCache,
};
use crate::output::types::{
    WordInfo, WordType, SUPPRESS_CONJ_FOR_NOUNS, SUPPRESS_CONJ_FOR_PARTICLES,
    SUPPRESS_CONJ_FOR_VERBS,
};
use crate::types::{Conj, PathNode, Reading, Segment, SegmentList, Word};

/// `word_info_from_word_match` — component WordInfo for compound words.
pub fn word_info_from_word_match(
    conn: &Connection,
    word_match: &crate::types::WordMatch,
    cache: Option<&ReadingsCache>,
) -> WordInfo {
    let reading = &word_match.reading;
    let seq = word_match.seq_opt();
    let (word_type, kana) = if word_match.word_type() == "kanji" {
        let kana = reading
            .best_kana()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| match seq {
                Some(s) => get_matching_kana_for_kanji(conn, s, reading.text()),
                // PlaceholderReading: seq=None → query yields nothing → ''
                None => String::new(),
            });
        (WordType::Kanji, kana)
    } else {
        (WordType::Kana, reading.text().to_string())
    };

    let mut conj_type_str: Option<String> = None;
    let mut source_text: Option<String> = None;

    if let Conj::Ids(conj_ids) = &word_match.conjugations {
        if let Some(conj_id) = conj_ids.first() {
            if let Ok((from_seq,)) = conn.query_row(
                "SELECT \"from\" FROM conjugation WHERE id = ?1",
                [conj_id],
                |r| Ok((r.get::<_, Option<i64>>(0)?,)),
            ) {
                if let Ok(ct) = conn.query_row(
                    "SELECT conj_type FROM conj_prop WHERE conj_id = ?1 LIMIT 1",
                    [conj_id],
                    |r| r.get::<_, i64>(0),
                ) {
                    conj_type_str = Some(conj_type_name(ct));
                }
                if let Some(fs) = from_seq {
                    source_text = match cache {
                        Some(c) => c.get_source_text(fs),
                        None => conn
                            .query_row(
                                "SELECT text FROM kanji_text WHERE seq = ?1 AND ord = 0 LIMIT 1",
                                [fs],
                                |r| r.get::<_, String>(0),
                            )
                            .ok(),
                    };
                }
            }
        }
    } else if has_conjugable_pos(conn, seq) {
        conj_type_str = Some("Non-past".to_string());
    }

    WordInfo {
        type_: word_type,
        text: word_match.text().to_string(),
        kana: vec![kana],
        seq: seq.map(|s| vec![s]),
        // Python: `word_match.conjugations if != 'root' else None`
        conjugations: match &word_match.conjugations {
            Conj::Unset | Conj::Root => None,
            other => Some(other.clone()),
        },
        conj_type: conj_type_str,
        source_text,
        ..Default::default()
    }
}

/// `word_info_from_segment`.
pub fn word_info_from_segment(
    conn: &Connection,
    segment: &Segment,
    cache: Option<&ReadingsCache>,
) -> WordInfo {
    let word = &segment.word;

    if let Word::Counter(c) = word {
        return WordInfo {
            type_: WordType::Kanji,
            text: segment.text().to_string(),
            kana: vec![c.kana.clone()],
            true_text: Some(c.text.clone()),
            seq: c.seq().map(|s| vec![s]),
            conjugations: Some(Conj::Ids(vec![])),
            score: segment.score as i64,
            start: Some(segment.start),
            end: Some(segment.end),
            ..Default::default()
        };
    }

    if let Word::Compound(cw) = word {
        let word_type = if cw.word_type() == "kana" {
            WordType::Kana
        } else {
            WordType::Kanji
        };
        let conj_data = segment.info.conj.clone();
        let mut conjugations = None;
        if !conj_data.is_empty() {
            let ids: Vec<i64> = conj_data
                .iter()
                .filter_map(|cd| cd.prop.as_ref().map(|p| p.conj_id))
                .collect();
            if !ids.is_empty() {
                conjugations = Some(Conj::Ids(ids));
            }
        }
        let mut conj_type_str = None;
        let (mut conj_neg, mut conj_fml) = (false, false);
        let mut source_text = None;
        if let Some(cd) = conj_data.first() {
            if let Some(prop) = &cd.prop {
                conj_type_str = Some(conj_type_name(prop.conj_type));
                conj_neg = prop.neg.unwrap_or(false);
                conj_fml = prop.fml.unwrap_or(false);
            }
            if cd.from_seq != 0 {
                source_text = match cache {
                    Some(c) => c.get_source_text(cd.from_seq),
                    None => conn
                        .query_row(
                            "SELECT text FROM kanji_text WHERE seq = ?1 AND ord = 0 LIMIT 1",
                            [cd.from_seq],
                            |r| r.get::<_, String>(0),
                        )
                        .ok()
                        .or_else(|| {
                            conn.query_row(
                                "SELECT text FROM kana_text WHERE seq = ?1 AND ord = 0 LIMIT 1",
                                [cd.from_seq],
                                |r| r.get::<_, String>(0),
                            )
                            .ok()
                        }),
                };
            }
        } else {
            // Fallback: conj info from the compound's last word.
            if let Some(last) = cw.words.last() {
                let cd2 = get_word_conj_data(conn, &Word::Simple(last.clone()));
                if let Some(cd) = cd2.first() {
                    if let Some(prop) = &cd.prop {
                        conj_type_str =
                            Some(conj_type_name(prop.conj_type));
                        conj_neg = prop.neg.unwrap_or(false);
                        conj_fml = prop.fml.unwrap_or(false);
                    }
                    let last_text = last.text().to_string();
                    for (t, st) in &cd.src_map {
                        if *t == last_text {
                            source_text = Some(st.clone());
                            break;
                        }
                    }
                }
            }
            if conjugations.is_none() {
                if let Conj::Ids(ids) = word.conjugations() {
                    conjugations = Some(Conj::Ids(ids.clone()));
                }
            }
        }

        let compound_texts: Vec<String> =
            cw.components().iter().map(|s| s.to_string()).collect();
        let component_word_infos: Vec<WordInfo> = cw
            .words
            .iter()
            .map(|wm| word_info_from_word_match(conn, wm, cache))
            .collect();

        return WordInfo {
            type_: word_type,
            text: segment.text().to_string(),
            kana: vec![cw.kana.clone()],
            true_text: Some(cw.text.clone()),
            seq: Some(vec![cw.seq()]),
            conjugations,
            score: segment.score as i64,
            components: component_word_infos,
            start: Some(segment.start),
            end: Some(segment.end),
            is_compound: true,
            compound_texts,
            conj_type: conj_type_str,
            conj_neg,
            conj_fml,
            source_text,
            ..Default::default()
        };
    }

    // Simple word
    let w = word.as_simple().unwrap();
    let (word_type, kana) = match &w.reading {
        Reading::Kanji(r) => {
            let kana = r
                .best_kana
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    get_matching_kana_for_kanji(conn, w.seq(), &r.text)
                });
            (WordType::Kanji, kana)
        }
        _ => (WordType::Kana, w.text().to_string()),
    };

    let mut conj_data = segment.info.conj.clone();
    if let Some(seq) = word.seq() {
        if SUPPRESS_CONJ_FOR_PARTICLES.contains(&seq)
            || SUPPRESS_CONJ_FOR_NOUNS.contains(&seq)
            || SUPPRESS_CONJ_FOR_VERBS.contains(&seq)
        {
            conj_data.clear();
        }
    }

    let mut conjugations = match word.conjugations() {
        Conj::Unset => None,
        other => Some(other.clone()),
    };
    if conjugations.is_none() && !conj_data.is_empty() {
        let ids: Vec<i64> = conj_data
            .iter()
            .filter_map(|cd| cd.prop.as_ref().map(|p| p.conj_id))
            .collect();
        if !ids.is_empty() {
            conjugations = Some(Conj::Ids(ids));
        }
    }

    let mut conj_type_str = None;
    let (mut conj_neg, mut conj_fml) = (false, false);
    let mut source_text = None;
    if let Some(cd) = conj_data.first() {
        if let Some(prop) = &cd.prop {
            conj_type_str = Some(conj_type_name(prop.conj_type));
            conj_neg = prop.neg.unwrap_or(false);
            conj_fml = prop.fml.unwrap_or(false);
        }
        if cd.from_seq != 0 {
            source_text = match cache {
                Some(c) => c.get_source_text(cd.from_seq),
                None => conn
                    .query_row(
                        "SELECT text FROM kanji_text WHERE seq = ?1 AND ord = 0 LIMIT 1",
                        [cd.from_seq],
                        |r| r.get::<_, String>(0),
                    )
                    .ok()
                    .or_else(|| {
                        conn.query_row(
                            "SELECT text FROM kana_text WHERE seq = ?1 AND ord = 0 LIMIT 1",
                            [cd.from_seq],
                            |r| r.get::<_, String>(0),
                        )
                        .ok()
                    }),
            };
        }
    } else if has_conjugable_pos(conn, word.seq()) {
        conj_type_str = Some("Non-past".to_string());
    }

    WordInfo {
        type_: word_type,
        text: segment.text().to_string(),
        kana: vec![kana],
        true_text: Some(w.text().to_string()),
        seq: word.seq().map(|s| vec![s]),
        conjugations,
        score: segment.score as i64,
        start: Some(segment.start),
        end: Some(segment.end),
        is_compound: false,
        conj_type: conj_type_str,
        conj_neg,
        conj_fml,
        source_text,
        ..Default::default()
    }
}

/// `word_info_from_segment_list`.
pub fn word_info_from_segment_list(
    conn: &Connection,
    segment_list: &SegmentList,
    cache: Option<&ReadingsCache>,
) -> WordInfo {
    let segments = &segment_list.segments;
    if segments.is_empty() {
        return WordInfo {
            type_: WordType::Gap,
            text: String::new(),
            kana: vec!["".to_string()],
            start: Some(segment_list.start),
            end: Some(segment_list.end),
            ..Default::default()
        };
    }

    let mut wi_list: Vec<WordInfo> = segments
        .iter()
        .map(|s| word_info_from_segment(conn, s, cache))
        .collect();
    let wi1 = wi_list[0].clone();
    let max_score = wi1.score;
    let cutoff = max_score as f64 * 0.67;
    wi_list.retain(|wi| wi.score as f64 >= cutoff);

    let matches = segment_list.matches;
    if wi_list.len() == 1 {
        let mut wi = wi1;
        wi.skipped = matches.saturating_sub(1);
        return wi;
    }

    let mut kana_list: Vec<String> = Vec::new();
    let mut seq_list: Vec<i64> = Vec::new();
    for wi in &wi_list {
        kana_list.extend(wi.kana.iter().cloned());
        if let Some(s) = &wi.seq {
            seq_list.extend(s.iter().copied());
        }
    }
    let mut seen = std::collections::HashSet::new();
    let unique_kana: Vec<String> = kana_list
        .into_iter()
        .filter(|k| seen.insert(k.clone()))
        .collect();

    WordInfo {
        type_: wi1.type_,
        text: wi1.text.clone(),
        kana: unique_kana,
        seq: if seq_list.is_empty() { None } else { Some(seq_list) },
        components: wi_list.clone(),
        alternative: true,
        score: wi1.score,
        start: Some(segment_list.start),
        end: Some(segment_list.end),
        skipped: matches.saturating_sub(wi_list.len()),
        conjugations: wi1.conjugations.clone(),
        conj_type: wi1.conj_type.clone(),
        conj_neg: wi1.conj_neg,
        conj_fml: wi1.conj_fml,
        source_text: wi1.source_text.clone(),
        ..Default::default()
    }
}

/// `word_info_from_text` — gap WordInfo for unmatched text.
pub fn word_info_from_text(text: &str) -> WordInfo {
    WordInfo {
        type_: WordType::Gap,
        text: text.to_string(),
        kana: vec![text.to_string()],
        ..Default::default()
    }
}

/// `fill_segment_path` — convert a path to WordInfos incl. gaps + meanings.
pub fn fill_segment_path(
    conn: &Connection,
    text: &str,
    path: &[std::rc::Rc<PathNode>],
    include_meanings: bool,
) -> Vec<WordInfo> {
    let seqs = crate::output::meanings::collect_seqs_from_path(path);
    let mut cache = ReadingsCache::new();
    cache.preload(conn, &seqs);
    let cache = &cache;

    let chars: Vec<char> = text.chars().collect();
    let text_len = chars.len();
    let mut result = Vec::new();
    let mut idx = 0usize;

    for node in path {
        let (_sl_start, sl_end) = (node.start(), node.end());
        match &**node {
            PathNode::List(l) => {
                if l.start > idx {
                    let gap: String = chars[idx..l.start].iter().collect();
                    let mut wi = word_info_from_text(&gap);
                    wi.start = Some(idx);
                    wi.end = Some(l.start);
                    result.push(wi);
                }
                let wi = word_info_from_segment_list(conn, l, Some(cache));
                result.extend(split_copula_compound_for_output(wi));
                idx = sl_end;
            }
            PathNode::Seg(s) => {
                if s.start > idx {
                    let gap: String = chars[idx..s.start].iter().collect();
                    let mut wi = word_info_from_text(&gap);
                    wi.start = Some(idx);
                    wi.end = Some(s.start);
                    result.push(wi);
                }
                let wi = word_info_from_segment(conn, s, Some(cache));
                result.extend(split_copula_compound_for_output(wi));
                idx = sl_end;
            }
            PathNode::Syn(_) => {}
        }
    }

    if idx < text_len {
        let gap: String = chars[idx..].iter().collect();
        let mut wi = word_info_from_text(&gap);
        wi.start = Some(idx);
        wi.end = Some(text_len);
        result.push(wi);
    }

    if include_meanings {
        populate_meanings(conn, &mut result);
    }
    result
}
