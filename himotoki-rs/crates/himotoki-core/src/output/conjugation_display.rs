//! Conjugation breakdown-tree text display — port of
//! `himotoki/output/conjugation_display.py`.

use rusqlite::Connection;

use crate::conj::get_conj_data;
use crate::constants::{
    conj_step_gloss, conj_type_name, CONJ_CAUSATIVE,
    CONJ_CAUSATIVE_PASSIVE,
};
use crate::db::rows::ConjugationRow;
use crate::grammar::suffixes::get_suffix_description;
use crate::output::meanings::get_entry_reading;
use crate::output::types::{ConjStep, WordInfo};
use crate::types::Conj;

/// `_get_conjugation_display`.
pub fn get_conjugation_display(conn: &Connection, wi: &WordInfo) -> Vec<String> {
    if !wi.has_seq() {
        return Vec::new();
    }

    if wi.is_compound && !wi.components.is_empty() {
        return get_compound_display(conn, wi);
    }

    if wi.alternative && !wi.components.is_empty() {
        let primary = &wi.components[0];
        if primary.is_compound && !primary.components.is_empty() {
            return get_compound_display(conn, primary);
        }
        for alt in &wi.components {
            if let Some(Conj::Ids(ids)) = &alt.conjugations {
                if !ids.is_empty() && alt.has_seq() {
                    if let Some(seq) = alt.first_seq() {
                        let mut lines =
                            format_conjugation_info(conn, seq, Some(ids));
                        if wi.text == "ではない" {
                            lines = lines
                                .into_iter()
                                .map(|l| l.replace("(じゃない)", "(ではない)"))
                                .collect();
                        }
                        return lines;
                    }
                }
            }
        }
    }

    let ids = match &wi.conjugations {
        Some(Conj::Ids(ids)) if !ids.is_empty() => ids.clone(),
        _ => return Vec::new(),
    };

    let seq = wi.first_seq().unwrap();
    let mut lines = format_conjugation_info(conn, seq, Some(&ids));
    if wi.text == "ではない" {
        lines = lines
            .into_iter()
            .map(|l| l.replace("(じゃない)", "(ではない)"))
            .collect();
    }
    lines
}

fn first_kana(wi: &WordInfo) -> String {
    wi.kana.first().cloned().unwrap_or_else(|| wi.text.clone())
}
fn first_seq(wi: &WordInfo) -> Option<i64> {
    wi.first_seq()
}
fn has_real_conjs(wi: &WordInfo) -> bool {
    matches!(&wi.conjugations, Some(Conj::Ids(ids)) if !ids.is_empty())
}

/// `_get_compound_display`.
fn get_compound_display(conn: &Connection, wi: &WordInfo) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    if wi.components.is_empty() {
        return result;
    }
    let primary = &wi.components[0];

    let mut primary_chain: Vec<String> = Vec::new();
    if has_real_conjs(primary) && primary.has_seq() {
        if let (Some(seq), Some(Conj::Ids(ids))) =
            (first_seq(primary), primary.conjugations.clone())
        {
            primary_chain = format_conjugation_info(conn, seq, Some(&ids));
        }
    }
    result.extend(primary_chain.iter().cloned());

    let mut depth = primary_chain
        .iter()
        .filter(|l| l.trim_start().starts_with("└─"))
        .count();

    for comp in &wi.components[1..] {
        let indent = "     ".repeat(depth);
        let mut comp_kana = first_kana(comp);
        if comp_kana.is_empty() {
            comp_kana = comp.text.clone();
        }
        let comp_seq = first_seq(comp);
        let desc = if comp_seq.is_some() || !comp_kana.is_empty() {
            get_suffix_description(comp_seq, Some(&comp_kana))
        } else {
            None
        };
        let desc_str = desc.map(|d| format!(" ({})", d)).unwrap_or_default();

        // na-adjective copula compounds
        if primary_chain.is_empty() && depth == 0 && (comp_kana == "です" || comp_kana == "でした")
        {
            let primary_kana = first_kana(primary);
            if !primary.text.is_empty() && primary.text != primary_kana {
                result.push(format!("  ← {}だ 【{}だ】", primary.text, primary_kana));
            } else {
                result.push(format!("  ← {}だ", primary_kana));
            }
            result.push("  └─ Polite (です)".to_string());
            depth = 1;
            if comp_kana == "でした" {
                result.push("       └─ Past (~ta) (でした): did/was".to_string());
                depth = 2;
            }
            continue;
        }

        // i-adj + すぎる compounds without explicit conjugation IDs
        if primary_chain.is_empty() && depth == 0 && comp_kana == "すぎる" {
            let primary_kana = first_kana(primary);
            if !primary_kana.is_empty() && primary_kana.ends_with('い') {
                if let Some(pseq) = first_seq(primary) {
                    result.push(format!("  ← {}", get_entry_reading(conn, pseq)));
                }
                let stem_kana: String =
                    primary_kana.chars().take(primary_kana.chars().count() - 1).collect();
                result.push(format!("  └─ Adjective Stem ({}): stem", stem_kana));
                depth = 1;
            }
        }

        if has_real_conjs(comp) && comp.has_seq() {
            let comp_ids = match &comp.conjugations {
                Some(Conj::Ids(ids)) => ids.clone(),
                _ => Vec::new(),
            };
            let comp_chain = comp_seq
                .map(|s| format_conjugation_info(conn, s, Some(&comp_ids)))
                .unwrap_or_default();
            if !comp_chain.is_empty() {
                let aux_name = extract_aux_name_from_chain(&comp_chain);

                if aux_name.as_deref() == Some("する") && primary_chain.is_empty() {
                    let primary_kana = first_kana(primary);
                    if !primary.text.is_empty() && primary.text != primary_kana {
                        result.push(format!(
                            "  ← {}する 【{}する】",
                            primary.text, primary_kana
                        ));
                    } else {
                        result.push(format!("  ← {}する", primary_kana));
                    }
                    for line in &comp_chain[1..] {
                        let stripped = line.trim_start();
                        if stripped.starts_with("└─") {
                            let sub_indent = "     ".repeat(depth);
                            result.push(format!("  {}{}", sub_indent, stripped));
                            depth += 1;
                        }
                    }
                } else if (aux_name.as_deref() == Some("いる")
                    || aux_name.as_deref() == Some("おる"))
                    && last_line_is_te(&result)
                {
                    relabel_te_progressive(&mut result);
                    for line in &comp_chain[1..] {
                        let stripped = line.trim_start();
                        if stripped.starts_with("└─") {
                            let sub_indent = "     ".repeat(depth);
                            result.push(format!("  {}{}", sub_indent, stripped));
                            depth += 1;
                        }
                    }
                } else {
                    for line in &comp_chain {
                        let stripped = line.trim_start();
                        if stripped.starts_with('←') {
                            let sub_indent = "     ".repeat(depth);
                            result.push(format!(
                                "  {}└─ {}{}",
                                sub_indent,
                                aux_name.clone().unwrap_or_default(),
                                desc_str
                            ));
                            depth += 1;
                        } else if stripped.starts_with("└─") {
                            let sub_indent = "     ".repeat(depth);
                            result.push(format!("  {}{}", sub_indent, stripped));
                            depth += 1;
                        }
                    }
                }
            } else {
                result.push(format!("  {}└─ {}{}", indent, comp_kana, desc_str));
                depth += 1;
            }
        } else {
            if comp_kana == "する" && primary_chain.is_empty() {
                let primary_kana = first_kana(primary);
                if !primary.text.is_empty() && primary.text != primary_kana {
                    result.push(format!(
                        "  ← {}する 【{}する】",
                        primary.text, primary_kana
                    ));
                } else {
                    result.push(format!("  ← {}する", primary_kana));
                }
            } else if (comp_kana == "いる" || comp_kana == "おる")
                && last_line_is_te(&result)
            {
                relabel_te_progressive(&mut result);
            } else {
                result.push(format!("  {}└─ {}{}", indent, comp_kana, desc_str));
                depth += 1;
            }
        }
    }

    result
}

fn extract_aux_name_from_chain(comp_chain: &[String]) -> Option<String> {
    let first = comp_chain.first()?.trim_start().to_string();
    if !first.starts_with('←') {
        return None;
    }
    let aux_name = first.trim_start_matches('←').trim();
    if let (Some(s), Some(e)) = (aux_name.find('【'), aux_name.find('】')) {
        return Some(aux_name[s + '【'.len_utf8()..e].to_string());
    }
    Some(aux_name.to_string())
}

fn last_line_is_te(result: &[String]) -> bool {
    for line in result.iter().rev() {
        let stripped = line.trim_start();
        if stripped.starts_with("└─") && stripped.contains("Conjunctive (~te") {
            return true;
        }
        if stripped.starts_with("└─") || stripped.starts_with('←') {
            return false;
        }
    }
    false
}

fn relabel_te_progressive(result: &mut [String]) {
    for line in result.iter_mut().rev() {
        if line.contains("Conjunctive (~te)") {
            *line = line.replace(
                "Conjunctive (~te)",
                "Conjunctive (~te, progressive)",
            );
            return;
        }
    }
}

/// `format_conjugation_info`.
pub fn format_conjugation_info(
    conn: &Connection,
    seq: i64,
    conjugations: Option<&[i64]>,
) -> Vec<String> {
    let mut result = Vec::new();

    let (sql, params): (String, Vec<rusqlite::types::Value>) = match conjugations {
        Some(ids) if !ids.is_empty() => {
            let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let mut p: Vec<rusqlite::types::Value> = vec![seq.into()];
            p.extend(ids.iter().map(|i| (*i).into()));
            (
                format!(
                    "SELECT id, seq, \"from\", via FROM conjugation WHERE seq = ?1 AND id IN ({})",
                    ph
                ),
                p,
            )
        }
        _ => (
            "SELECT id, seq, \"from\", via FROM conjugation WHERE seq = ?1".to_string(),
            vec![seq.into()],
        ),
    };

    let conjs: Vec<ConjugationRow> = conn
        .prepare(&sql)
        .and_then(|mut s| {
            s.query_map(rusqlite::params_from_iter(params), |r| {
                Ok(ConjugationRow {
                    id: r.get(0)?,
                    seq: r.get(1)?,
                    from_seq: r.get(2)?,
                    via: r.get(3)?,
                })
            })
            .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    for conj in conjs.iter().take(1) {
        let props: Vec<crate::db::rows::ConjPropRow> = conn
            .prepare("SELECT id, conj_id, conj_type, pos, neg, fml FROM conj_prop WHERE conj_id = ?1")
            .and_then(|mut s| {
                s.query_map([conj.id], |r| {
                    Ok(crate::db::rows::ConjPropRow {
                        id: r.get(0)?,
                        conj_id: r.get(1)?,
                        conj_type: r.get(2)?,
                        pos: r.get(3)?,
                        neg: r.get(4)?,
                        fml: r.get(5)?,
                    })
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();

        for prop in props.iter().take(1) {
            let steps = build_conj_chain(conn, conj, prop);
            if !steps.is_empty() {
                let root_reading =
                    get_entry_reading(conn, conj.from_seq);
                result.push(format!("  ← {}", root_reading));
                let mut current_depth = 0usize;
                for step in &steps {
                    let indent = "     ".repeat(current_depth);
                    if step.conj_type == "Causative-Passive" {
                        let suffix = &step.suffix;
                        let (caus_suffix, pass_suffix) = if let Some(i) = suffix.find("られ") {
                            (suffix[..i].to_string(), suffix[i..].to_string())
                        } else if let Some(i) = suffix.find("され") {
                            (
                                suffix[..i + "さ".len()].to_string(),
                                suffix[i + "さ".len()..].to_string(),
                            )
                        } else {
                            (suffix.clone(), String::new())
                        };
                        result.push(format!(
                            "  {}└─ Causative ({}): makes do",
                            indent, caus_suffix
                        ));
                        current_depth += 1;
                        let indent = "     ".repeat(current_depth);
                        if !pass_suffix.is_empty() {
                            result.push(format!(
                                "  {}└─ Passive ({}): is done (to)",
                                indent, pass_suffix
                            ));
                        } else {
                            result.push(format!("  {}└─ Passive: is done (to)", indent));
                        }
                        current_depth += 1;
                    } else if step.fml {
                        let polite_morpheme =
                            if step.suffix.starts_with('で') { "です" } else { "ます" };
                        result.push(format!("  {}└─ Polite ({})", indent, polite_morpheme));
                        current_depth += 1;
                        if step.conj_type != "Non-past" || step.neg {
                            let indent = "     ".repeat(current_depth);
                            let mut suffix = step.suffix.clone();
                            if step.conj_type == "Volitional" {
                                suffix = "よう".to_string();
                            } else if let Some(i) = suffix.find('ま') {
                                suffix = suffix[i + 'ま'.len_utf8()..].to_string();
                            }
                            if step.neg {
                                if step.conj_type == "Non-past" {
                                    result.push(format!(
                                        "  {}└─ Negative ({}): not",
                                        indent, suffix
                                    ));
                                    current_depth += 1;
                                } else if suffix.starts_with("せん")
                                    && suffix.chars().count() > 2
                                {
                                    let conj_suffix =
                                        suffix["せん".len()..].to_string();
                                    result.push(format!(
                                        "  {}└─ Negative (せん): not",
                                        indent
                                    ));
                                    current_depth += 1;
                                    let indent = "     ".repeat(current_depth);
                                    let gloss_str = if step.gloss.is_empty() {
                                        String::new()
                                    } else {
                                        format!(": {}", step.gloss)
                                    };
                                    result.push(format!(
                                        "  {}└─ {} ({}){}",
                                        indent, step.conj_type, conj_suffix, gloss_str
                                    ));
                                    current_depth += 1;
                                } else {
                                    let (neg_part, conj_part) = split_neg_suffix(&suffix);
                                    result.push(format!(
                                        "  {}└─ Negative ({}): not",
                                        indent, neg_part
                                    ));
                                    current_depth += 1;
                                    if !conj_part.is_empty() {
                                        let indent = "     ".repeat(current_depth);
                                        let gloss_str = if step.gloss.is_empty() {
                                            String::new()
                                        } else {
                                            format!(": {}", step.gloss)
                                        };
                                        result.push(format!(
                                            "  {}└─ {} ({}){}",
                                            indent, step.conj_type, conj_part, gloss_str
                                        ));
                                        current_depth += 1;
                                    }
                                }
                            } else {
                                let gloss_str = if step.gloss.is_empty() {
                                    String::new()
                                } else {
                                    format!(": {}", step.gloss)
                                };
                                result.push(format!(
                                    "  {}└─ {} ({}){}",
                                    indent, step.conj_type, suffix, gloss_str
                                ));
                                current_depth += 1;
                            }
                        }
                    } else {
                        let mut type_label = step.conj_type.clone();
                        let mut gloss = step.gloss.clone();
                        let suffix = step.suffix.clone();
                        if type_label == "Potential" {
                            const GODAN_POTENTIAL: &[char] =
                                &['え', 'け', 'せ', 'て', 'ね', 'へ', 'め', 'げ'];
                            let first_char = suffix.chars().next();
                            if !first_char.map(|c| GODAN_POTENTIAL.contains(&c)).unwrap_or(false)
                            {
                                type_label = "Potential/Passive".to_string();
                                gloss = "can do / is done (to)".to_string();
                            }
                        }
                        if step.neg {
                            if type_label == "Non-past" {
                                result.push(format!(
                                    "  {}└─ Negative ({}): not",
                                    indent, suffix
                                ));
                                current_depth += 1;
                            } else if suffix.ends_with("ない") {
                                let conj_suffix =
                                    suffix[..suffix.len() - "ない".len()].to_string();
                                let gloss_str = if gloss.is_empty() {
                                    String::new()
                                } else {
                                    format!(": {}", gloss)
                                };
                                result.push(format!(
                                    "  {}└─ {} ({}){}",
                                    indent, type_label, conj_suffix, gloss_str
                                ));
                                current_depth += 1;
                                let indent = "     ".repeat(current_depth);
                                result.push(format!(
                                    "  {}└─ Negative (ない): not",
                                    indent
                                ));
                                current_depth += 1;
                            } else {
                                let (neg_part, conj_part) = split_neg_suffix(&suffix);
                                result.push(format!(
                                    "  {}└─ Negative ({}): not",
                                    indent, neg_part
                                ));
                                current_depth += 1;
                                if !conj_part.is_empty() {
                                    let indent = "     ".repeat(current_depth);
                                    let gloss_str = if gloss.is_empty() {
                                        String::new()
                                    } else {
                                        format!(": {}", gloss)
                                    };
                                    result.push(format!(
                                        "  {}└─ {} ({}){}",
                                        indent, type_label, conj_part, gloss_str
                                    ));
                                    current_depth += 1;
                                }
                            }
                        } else {
                            let gloss_str = if gloss.is_empty() {
                                String::new()
                            } else {
                                format!(": {}", gloss)
                            };
                            result.push(format!(
                                "  {}└─ {} ({}){}",
                                indent, type_label, suffix, gloss_str
                            ));
                            current_depth += 1;
                        }
                    }
                }
            } else {
                let neg_str = if prop.neg.unwrap_or(false) {
                    " Negative"
                } else {
                    " Affirmative"
                };
                let fml_str = if prop.fml.unwrap_or(false) {
                    " Formal"
                } else {
                    " Plain"
                };
                let type_desc = conj_type_name(prop.conj_type);
                result.push(format!(
                    "  ← [{}] {}{}{}",
                    prop.pos, type_desc, neg_str, fml_str
                ));
                result.push(format!(
                    "     {}",
                    get_entry_reading(conn, conj.from_seq)
                ));
            }
        }
    }

    result
}

/// `_build_conj_chain`.
fn build_conj_chain(
    conn: &Connection,
    conj: &ConjugationRow,
    outer_prop: &crate::db::rows::ConjPropRow,
) -> Vec<ConjStep> {
    let mut steps = Vec::new();

    if let Some(via) = conj.via {
        collect_via_steps(conn, via, conj.from_seq, &mut steps);
    }

    let type_name = conj_type_name(outer_prop.conj_type);
    let gloss = conj_step_gloss(outer_prop.conj_type).to_string();
    let suffix = get_conj_suffix(conn, conj, outer_prop);

    steps.push(ConjStep {
        conj_type: type_name,
        suffix,
        gloss,
        neg: outer_prop.neg.unwrap_or(false),
        fml: outer_prop.fml.unwrap_or(false),
    });

    steps
}

/// `_collect_via_steps`.
fn collect_via_steps(
    conn: &Connection,
    via_seq: i64,
    from_seq: i64,
    steps: &mut Vec<ConjStep>,
) {
    let via_data = get_conj_data(conn, via_seq, Some(from_seq), None, None);
    let cd = match via_data.into_iter().next() {
        Some(cd) => cd,
        None => return,
    };

    if let Some(via) = cd.via {
        collect_via_steps(conn, via, cd.from_seq, steps);
    }

    if let Some(prop) = &cd.prop {
        let type_name = conj_type_name(prop.conj_type);
        let gloss = conj_step_gloss(prop.conj_type).to_string();

        let prefer_long =
            prop.conj_type == CONJ_CAUSATIVE || prop.conj_type == CONJ_CAUSATIVE_PASSIVE;
        let mut suffix = String::new();
        if !cd.src_map.is_empty() {
            let mut best_suffix: Option<String> = None;
            for (text, src) in &cd.src_map {
                let s = extract_suffix(text, src);
                if !s.is_empty() && s != *text {
                    match &best_suffix {
                        None => best_suffix = Some(s),
                        Some(bs) => {
                            let better = if prefer_long {
                                s.chars().count() > bs.chars().count()
                            } else {
                                s.chars().count() < bs.chars().count()
                            };
                            if better {
                                best_suffix = Some(s);
                            }
                        }
                    }
                }
            }
            suffix = match best_suffix {
                Some(s) => s,
                None => extract_suffix(&cd.src_map[0].0, &cd.src_map[0].1),
            };
        }

        steps.push(ConjStep {
            conj_type: type_name,
            suffix,
            gloss,
            neg: prop.neg.unwrap_or(false),
            fml: prop.fml.unwrap_or(false),
        });
    }
}

/// `_get_conj_suffix`.
fn get_conj_suffix(
    conn: &Connection,
    conj: &ConjugationRow,
    prop: &crate::db::rows::ConjPropRow,
) -> String {
    let src_readings: Vec<(String, String)> = conn
        .prepare("SELECT text, source_text FROM conj_source_reading WHERE conj_id = ?1")
        .and_then(|mut s| {
            s.query_map([conj.id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    if src_readings.is_empty() {
        return String::new();
    }

    let prefer_long =
        prop.conj_type == CONJ_CAUSATIVE || prop.conj_type == CONJ_CAUSATIVE_PASSIVE;
    let mut best_suffix: Option<String> = None;
    for (text, src) in &src_readings {
        let suffix = extract_suffix(text, src);
        if !suffix.is_empty() && suffix != *text {
            match &best_suffix {
                None => best_suffix = Some(suffix),
                Some(bs) => {
                    let better = if prefer_long {
                        suffix.chars().count() > bs.chars().count()
                    } else {
                        suffix.chars().count() < bs.chars().count()
                    };
                    if better {
                        best_suffix = Some(suffix);
                    }
                }
            }
        }
    }

    best_suffix.unwrap_or_else(|| extract_suffix(&src_readings[0].0, &src_readings[0].1))
}

/// `_split_neg_suffix`.
fn split_neg_suffix(suffix: &str) -> (String, String) {
    for ending in ["かったら", "かった", "ければ", "くて"] {
        if suffix.ends_with(ending) {
            let neg_part =
                format!("{}い", &suffix[..suffix.len() - ending.len()]);
            return (neg_part, ending.to_string());
        }
    }
    if suffix.ends_with('で') && suffix.chars().count() >= 3 {
        let potential_neg = &suffix[..suffix.len() - 'で'.len_utf8()];
        if potential_neg.ends_with("ない") {
            return (potential_neg.to_string(), "で".to_string());
        }
    }
    (suffix.to_string(), String::new())
}

/// `_extract_suffix`.
fn extract_suffix(conj_text: &str, src_text: &str) -> String {
    let mut common_len = 0usize;
    for (c1, c2) in conj_text.chars().zip(src_text.chars()) {
        if c1 == c2 {
            common_len += c1.len_utf8();
        } else {
            break;
        }
    }
    let suffix_part = &conj_text[common_len..];
    if suffix_part.is_empty() {
        conj_text.to_string()
    } else {
        suffix_part.to_string()
    }
}
