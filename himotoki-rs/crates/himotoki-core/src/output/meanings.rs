//! Sense/gloss lookups + reading helpers — port of `himotoki/output/meanings.py`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::constants::conj_type_name;
use crate::output::types::special_conj_info;
use crate::types::{PathNode, Segment};

/// `reading_str` — "kanji 【kana】" or bare kana.
pub fn reading_str(kanji: Option<&str>, kana: &str) -> String {
    match kanji {
        Some(k) if !k.is_empty() => format!("{} 【{}】", k, kana),
        _ => kana.to_string(),
    }
}

/// `get_entry_reading` — kanji ord=0 + kana ord=0.
pub fn get_entry_reading(conn: &Connection, seq: i64) -> String {
    let kanji: Option<String> = conn
        .query_row(
            "SELECT text FROM kanji_text WHERE seq = ?1 AND ord = 0 LIMIT 1",
            [seq],
            |r| r.get(0),
        )
        .ok();
    let kana: Option<String> = conn
        .query_row(
            "SELECT text FROM kana_text WHERE seq = ?1 AND ord = 0 LIMIT 1",
            [seq],
            |r| r.get(0),
        )
        .ok();
    reading_str(kanji.as_deref(), kana.as_deref().unwrap_or(""))
}

/// `get_matching_kana_for_kanji` — suffix-matched kana for a kanji text.
pub fn get_matching_kana_for_kanji(conn: &Connection, seq: i64, kanji_text: &str) -> String {
    let mut stmt =
        match conn.prepare_cached("SELECT text, ord FROM kana_text WHERE seq = ?1 ORDER BY ord") {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
    let kana_results: Vec<(String, i64)> = stmt
        .query_map([seq], |r| Ok((r.get(0)?, r.get(1)?)))
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default();
    if kana_results.is_empty() {
        return String::new();
    }
    if kana_results.len() == 1 {
        return kana_results[0].0.clone();
    }
    // Extract trailing hiragana suffix of kanji_text.
    let mut kanji_suffix = String::new();
    for c in kanji_text.chars().rev() {
        let code = c as u32;
        if (0x3040..=0x309F).contains(&code) {
            kanji_suffix.insert(0, c);
        } else {
            break;
        }
    }
    if !kanji_suffix.is_empty() {
        for (kana, _) in &kana_results {
            if kana.ends_with(&kanji_suffix) {
                return kana.clone();
            }
        }
    }
    kana_results[0].0.clone()
}

// ============================================================================
// Meanings cache — global (Python `_MEANINGS_CACHE`)
// ============================================================================

const MEANINGS_CACHE_MAX: usize = 50000;

static MEANINGS_CACHE: LazyLock<Mutex<HashMap<i64, (Vec<String>, Option<String>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn get_cached_meanings(seq: i64) -> Option<(Vec<String>, Option<String>)> {
    MEANINGS_CACHE.lock().unwrap().get(&seq).cloned()
}

fn cache_meanings(seq: i64, meanings: Vec<String>, pos: Option<String>) {
    let mut m = MEANINGS_CACHE.lock().unwrap();
    if m.len() >= MEANINGS_CACHE_MAX {
        // Drop arbitrary half (Python keeps insertion-ordered half).
        let keep: Vec<i64> = m.keys().copied().skip(m.len() / 2).collect();
        let mut nm = HashMap::new();
        for k in keep {
            if let Some(v) = m.remove(&k) {
                nm.insert(k, v);
            }
        }
        *m = nm;
    }
    m.insert(seq, (meanings, pos));
}

pub fn clear_meanings_cache() {
    MEANINGS_CACHE.lock().unwrap().clear();
}

// ============================================================================
// ReadingsCache — batch kanji/kana ord=0 reads for fill_segment_path
// ============================================================================

#[derive(Default)]
pub struct ReadingsCache {
    kanji_readings: HashMap<i64, String>,
    kana_readings: HashMap<i64, String>,
}

impl ReadingsCache {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn preload(&mut self, conn: &Connection, seqs: &std::collections::HashSet<i64>) {
        if seqs.is_empty() {
            return;
        }
        let ph = seqs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let params: Vec<rusqlite::types::Value> = seqs.iter().map(|i| (*i).into()).collect();
        for (table, map) in [
            ("kanji_text", &mut self.kanji_readings),
            ("kana_text", &mut self.kana_readings),
        ] {
            let sql = format!(
                "SELECT seq, text FROM {} WHERE seq IN ({}) AND ord = 0",
                table, ph
            );
            if let Ok(mut stmt) = conn.prepare_cached(&sql) {
                if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(params.clone()), |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                }) {
                    for r in rows.flatten() {
                        map.insert(r.0, r.1);
                    }
                }
            }
        }
    }
    pub fn get_kanji(&self, seq: i64) -> Option<&str> {
        self.kanji_readings.get(&seq).map(|s| s.as_str())
    }
    pub fn get_kana(&self, seq: i64) -> &str {
        self.kana_readings
            .get(&seq)
            .map(|s| s.as_str())
            .unwrap_or("")
    }
    /// kanji preferred, else kana.
    pub fn get_source_text(&self, from_seq: i64) -> Option<String> {
        if let Some(k) = self.kanji_readings.get(&from_seq) {
            return Some(k.clone());
        }
        self.kana_readings.get(&from_seq).cloned()
    }
}

// ============================================================================
// collect_seqs_from_path
// ============================================================================

/// `collect_seqs_from_path` — word seqs + component seqs + conj from_seqs.
pub fn collect_seqs_from_path(path: &[std::rc::Rc<PathNode>]) -> std::collections::HashSet<i64> {
    let mut seqs = std::collections::HashSet::new();
    for node in path {
        match &**node {
            PathNode::List(l) => {
                for s in &l.segments {
                    collect_segment_seqs(s, &mut seqs);
                }
            }
            PathNode::Seg(s) => collect_segment_seqs(s, &mut seqs),
            PathNode::Syn(_) => {}
        }
    }
    seqs
}

fn collect_segment_seqs(segment: &Segment, seqs: &mut std::collections::HashSet<i64>) {
    match &segment.word {
        crate::types::Word::Counter(c) => {
            if let Some(s) = c.seq() {
                seqs.insert(s);
            }
            return;
        }
        crate::types::Word::Compound(cw) => {
            if cw.seq() != 0 {
                seqs.insert(cw.seq());
            }
            seqs.insert(cw.primary.seq());
            for w in &cw.words {
                let s = w.seq();
                if s != 0 {
                    seqs.insert(s);
                }
            }
        }
        crate::types::Word::Simple(w) => {
            seqs.insert(w.seq());
        }
    }
    for cd in &segment.info.conj {
        if cd.from_seq != 0 {
            seqs.insert(cd.from_seq);
        }
    }
}

/// `word_info_reading_str` — kanji 【kana】 w/ suru-merge for compounds.
pub fn word_info_reading_str(wi: &crate::output::types::WordInfo) -> String {
    use crate::output::types::WordType;
    if wi.type_ == WordType::Kanji || wi.counter.is_some() {
        let mut kana = wi.kana.join("/");
        if wi.is_compound && !wi.components.is_empty() {
            kana = merge_suru_kana(wi, &kana);
        }
        reading_str(Some(&wi.text), &kana)
    } else {
        reading_str(None, &wi.text)
    }
}

fn merge_suru_kana(wi: &crate::output::types::WordInfo, kana: &str) -> String {
    if wi.components.len() < 2 {
        return kana.to_string();
    }
    let primary = &wi.components[0];
    let suffix = &wi.components[1];
    // Python: `if primary.conjugations and primary.conjugations != 'root'`
    if let Some(crate::types::Conj::Ids(ids)) = &primary.conjugations {
        if !ids.is_empty() {
            return kana.to_string();
        }
    }
    let suffix_kana = suffix.kana_str();
    if suffix_kana.starts_with('し') || suffix_kana.starts_with('す') {
        let primary_kana = primary.kana_str();
        let expected_prefix = format!("{} {}", primary_kana, suffix_kana.chars().next().unwrap());
        if !primary_kana.is_empty() && kana.starts_with(&expected_prefix) {
            return kana.replacen(&format!("{} ", primary_kana), primary_kana, 1);
        }
    }
    kana.to_string()
}

/// `has_conjugable_pos` — any pos tag in NONPAST_POS set.
pub fn has_conjugable_pos(conn: &Connection, seq: Option<i64>) -> bool {
    let seq = match seq {
        Some(s) => s,
        None => return false,
    };
    const NONPAST_POS: &[&str] = &[
        "v1", "v1-s", "v1s", "v5aru", "v5b", "v5g", "v5k", "v5k-s", "v5m", "v5n", "v5r", "v5r-i",
        "v5s", "v5t", "v5u", "v5u-s", "v5uru", "vk", "vs-i", "vs-s", "vz", "adj-i", "adj-ix",
        "cop", "cop-da", "aux-v",
    ];
    let ph = NONPAST_POS
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT 1 FROM sense_prop sp JOIN sense s ON sp.sense_id = s.id \
         WHERE s.seq = ?1 AND sp.tag = 'pos' AND sp.text IN ({}) LIMIT 1",
        ph
    );
    let mut stmt = match conn.prepare_cached(&sql) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut params: Vec<rusqlite::types::Value> = vec![seq.into()];
    params.extend(
        NONPAST_POS
            .iter()
            .map(|p| rusqlite::types::Value::from(p.to_string())),
    );
    stmt.query_row(rusqlite::params_from_iter(params), |_| Ok(()))
        .is_ok()
}

// ============================================================================
// Senses / glosses
// ============================================================================

/// `get_senses_raw` — [{ord, gloss, props}] for a seq.
pub fn get_senses_raw(
    conn: &Connection,
    seq: i64,
) -> Vec<(i64, String, HashMap<String, Vec<String>>)> {
    let mut stmt = match conn.prepare_cached(
        "SELECT s.ord, group_concat(g.text, '; ') FROM sense s \
         LEFT JOIN gloss g ON g.sense_id = s.id WHERE s.seq = ?1 \
         GROUP BY s.id ORDER BY s.ord",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let glosses: Vec<(i64, String)> = stmt
        .query_map([seq], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default();

    let mut pmt = match conn.prepare_cached(
        "SELECT s.ord, sp.tag, sp.text FROM sense s \
         JOIN sense_prop sp ON sp.sense_id = s.id \
         WHERE s.seq = ?1 AND sp.tag IN ('pos','s_inf','stagk','stagr','field') \
         ORDER BY s.ord, sp.tag, sp.ord",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let props: Vec<(i64, String, String)> = pmt
        .query_map([seq], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default();

    let mut sense_list: Vec<(i64, String, HashMap<String, Vec<String>>)> = glosses
        .into_iter()
        .map(|(o, g)| (o, g, HashMap::new()))
        .collect();
    for (sord, tag, text) in props {
        if let Some(sense) = sense_list.iter_mut().find(|s| s.0 == sord) {
            sense.2.entry(tag).or_default().push(text);
        }
    }
    sense_list
}

/// `get_senses` — [{pos:"[a,b]", gloss, props}].
pub fn get_senses(
    conn: &Connection,
    seq: i64,
) -> Vec<(String, String, HashMap<String, Vec<String>>)> {
    get_senses_raw(conn, seq)
        .into_iter()
        .map(|(_, gloss, props)| {
            let pos_list = props.get("pos").cloned().unwrap_or_default();
            let pos_str = if pos_list.is_empty() {
                "[]".to_string()
            } else {
                format!("[{}]", pos_list.join(","))
            };
            (pos_str, gloss, props)
        })
        .collect()
}

/// `get_root_seq` — first conjugation.from_seq for seq.
pub fn get_root_seq(conn: &Connection, seq: i64) -> Option<i64> {
    conn.prepare_cached("SELECT \"from\" FROM conjugation WHERE seq = ?1 LIMIT 1")
        .and_then(|mut s| s.query_row([seq], |r| r.get(0)))
        .ok()
}

/// `get_senses_str` — formatted numbered senses.
pub fn get_senses_str(conn: &Connection, seq: i64) -> String {
    let mut lines = Vec::new();
    let mut rpos = "[]".to_string();
    for (i, (pos, gloss, props)) in get_senses(conn, seq).into_iter().enumerate() {
        if pos != "[]" {
            rpos = pos;
        }
        let rinf = props.get("s_inf").map(|v| v.join("; "));
        let rfield = props.get("field").map(|v| v.join(","));
        let mut parts = vec![format!("{}. {}", i + 1, rpos)];
        if let Some(f) = rfield {
            parts.push(format!("{{{}}}", f));
        }
        if let Some(r) = rinf {
            parts.push(format!("《{}》", r));
        }
        parts.push(gloss);
        lines.push(parts.join(" "));
    }
    lines.join("\n")
}

/// `get_senses_json` — [{pos, gloss, field?, info?}] with pos_list filter.
pub fn get_senses_json(conn: &Connection, seq: i64, pos_list: Option<&[&str]>) -> Vec<Value> {
    let mut result = Vec::new();
    let mut rpos = "[]".to_string();
    for (pos, gloss, props) in get_senses(conn, seq) {
        if pos != "[]" {
            rpos = pos.clone();
        }
        if let Some(pl) = pos_list {
            let lpos: Vec<&str> = if pos == "[]" {
                Vec::new()
            } else {
                pos[1..pos.len() - 1].split(',').collect()
            };
            if !lpos.iter().any(|p| pl.contains(p)) {
                continue;
            }
        }
        let mut js = json!({"pos": rpos, "gloss": gloss});
        if let Some(fields) = props.get("field") {
            if !fields.is_empty() {
                js["field"] = json!(format!("{{{}}}", fields.join(",")));
            }
        }
        if let Some(info) = props.get("s_inf") {
            if !info.is_empty() {
                js["info"] = json!(info.join("; "));
            }
        }
        result.push(js);
    }
    result
}

/// `conj_prop_json`.
pub fn conj_prop_json(prop: &crate::db::rows::ConjPropRow) -> Value {
    let mut js = json!({
        "pos": prop.pos,
        "type": conj_type_name(prop.conj_type),
    });
    if prop.neg.unwrap_or(false) {
        js["neg"] = json!(true);
    }
    if prop.fml.unwrap_or(false) {
        js["fml"] = json!(true);
    }
    js
}

/// `conj_info_json`.
pub fn conj_info_json(
    conn: &Connection,
    seq: i64,
    conjugations: Option<&[i64]>,
    _text: Option<&str>,
) -> Vec<Value> {
    let mut result = Vec::new();
    let conjs: Vec<(i64, i64)> = {
        let (sql, params): (String, Vec<rusqlite::types::Value>) = match conjugations {
            Some(ids) if !ids.is_empty() => {
                let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let mut p: Vec<rusqlite::types::Value> = vec![seq.into()];
                p.extend(ids.iter().map(|i| (*i).into()));
                (
                    format!(
                        "SELECT id, \"from\" FROM conjugation WHERE seq = ?1 AND id IN ({})",
                        ph
                    ),
                    p,
                )
            }
            _ => (
                "SELECT id, \"from\" FROM conjugation WHERE seq = ?1".to_string(),
                vec![seq.into()],
            ),
        };
        conn.prepare_cached(&sql)
            .and_then(|mut s| {
                s.query_map(rusqlite::params_from_iter(params), |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
    };

    for (conj_id, from_seq) in conjs {
        let props: Vec<crate::db::rows::ConjPropRow> = conn
            .prepare_cached(
                "SELECT id, conj_id, conj_type, pos, neg, fml FROM conj_prop WHERE conj_id = ?1",
            )
            .and_then(|mut s| {
                s.query_map([conj_id], |r| {
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
        if props.is_empty() {
            continue;
        }
        result.push(json!({
            "prop": props.iter().map(conj_prop_json).collect::<Vec<_>>(),
            "reading": get_entry_reading(conn, from_seq),
            "gloss": get_senses_json(conn, from_seq, None),
            "readok": true,
        }));
    }

    if result.is_empty() {
        if let Some((from_seq, conj_type, pos, neg, fml)) = special_conj_info(seq) {
            let mut p = json!({
                "pos": pos,
                "type": conj_type_name(conj_type),
            });
            if neg {
                p["neg"] = json!(true);
            }
            if fml {
                p["fml"] = json!(true);
            }
            result.push(json!({
                "prop": [p],
                "reading": get_entry_reading(conn, from_seq),
                "gloss": get_senses_json(conn, from_seq, None),
                "readok": true,
            }));
        }
    }
    result
}

/// `_split_copula_compound_for_output`.
pub fn split_copula_compound_for_output(
    wi: crate::output::types::WordInfo,
) -> Vec<crate::output::types::WordInfo> {
    if !wi.is_compound || wi.components.len() != 2 {
        return vec![wi];
    }
    let primary = wi.components[0].clone();
    let suffix = wi.components[1].clone();
    let suffix_kana = suffix.kana_str().to_string();
    if suffix_kana != "です" && suffix_kana != "でした" {
        return vec![wi];
    }
    // Python: `if primary.conjugations and primary.conjugations != 'root'`
    if let Some(crate::types::Conj::Ids(ids)) = &primary.conjugations {
        if !ids.is_empty() {
            return vec![wi];
        }
    }
    vec![primary, suffix]
}

/// `populate_meanings` — fill meanings/pos on each wi with a seq.
pub fn populate_meanings(conn: &Connection, word_infos: &mut [crate::output::types::WordInfo]) {
    use std::collections::HashSet;
    let mut seqs: HashSet<i64> = HashSet::new();
    for wi in word_infos.iter() {
        if wi.type_ == crate::output::types::WordType::Gap {
            continue;
        }
        if let Some(s) = wi.first_seq() {
            seqs.insert(s);
        }
    }
    if seqs.is_empty() {
        return;
    }

    // Cached first
    let mut meanings_by_seq: HashMap<i64, Vec<String>> = HashMap::new();
    let mut pos_by_seq: HashMap<i64, Option<String>> = HashMap::new();
    let mut uncached: Vec<i64> = Vec::new();
    for s in &seqs {
        match get_cached_meanings(*s) {
            Some((m, p)) => {
                meanings_by_seq.insert(*s, m);
                pos_by_seq.insert(*s, p);
            }
            None => uncached.push(*s),
        }
    }

    if !uncached.is_empty() {
        let ph = uncached.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let params: Vec<rusqlite::types::Value> = uncached.iter().map(|i| (*i).into()).collect();
        // senses+glosses
        let sql = format!(
            "SELECT s.seq, s.ord, group_concat(g.text, '; ') FROM sense s \
             LEFT JOIN gloss g ON g.sense_id = s.id WHERE s.seq IN ({}) \
             GROUP BY s.id ORDER BY s.seq, s.ord",
            ph
        );
        if let Ok(mut st) = conn.prepare_cached(&sql) {
            if let Ok(rows) = st.query_map(rusqlite::params_from_iter(params.clone()), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            }) {
                for r in rows.flatten() {
                    if let Some(g) = r.2 {
                        meanings_by_seq.entry(r.0).or_default().push(g);
                    } else {
                        meanings_by_seq.entry(r.0).or_default();
                    }
                }
            }
        }
        // pos (ord=0 senses)
        let sql = format!(
            "SELECT s.seq, sp.text FROM sense s \
             JOIN sense_prop sp ON sp.sense_id = s.id \
             WHERE s.seq IN ({}) AND sp.tag = 'pos' AND s.ord = 0 \
             ORDER BY s.seq, sp.ord",
            ph
        );
        let mut pos_tags: HashMap<i64, Vec<String>> = HashMap::new();
        if let Ok(mut st) = conn.prepare_cached(&sql) {
            if let Ok(rows) = st.query_map(rusqlite::params_from_iter(params), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            }) {
                for r in rows.flatten() {
                    pos_tags.entry(r.0).or_default().push(r.1);
                }
            }
        }
        for (seq, tags) in pos_tags {
            pos_by_seq.insert(seq, Some(format!("[{}]", tags.join(","))));
        }
        for s in &uncached {
            cache_meanings(
                *s,
                meanings_by_seq.get(s).cloned().unwrap_or_default(),
                pos_by_seq.get(s).cloned().flatten(),
            );
        }
    }

    for wi in word_infos.iter_mut() {
        if wi.type_ == crate::output::types::WordType::Gap {
            continue;
        }
        if let Some(s) = wi.first_seq() {
            wi.meanings = meanings_by_seq.get(&s).cloned().unwrap_or_default();
            wi.pos = pos_by_seq.get(&s).cloned().flatten();
        }
    }
}
