//! Conjugation lookup — port of `himotoki/lookup/conj_data.py`.

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::constants::conj_type_name;
use crate::db::rows::{ConjPropRow, ConjugationRow};
use crate::types::{Conj, ConjData, Word};

/// (seq, from_seq) pairs never treated as conjugations (conj_data.py:21).
static BLOCKED_CONJUGATIONS: &[(i64, i64)] = &[(2029090, 2820690)];

type CacheKey = (i64, Option<i64>, Option<Vec<i64>>, Option<Vec<String>>);
static CONJ_DATA_CACHE: Mutex<Option<HashMap<CacheKey, Vec<ConjData>>>> = Mutex::new(None);

fn cache_get(key: &CacheKey) -> Option<Vec<ConjData>> {
    let guard = CONJ_DATA_CACHE.lock().unwrap();
    guard.as_ref()?.get(key).cloned()
}
fn cache_put(key: CacheKey, val: Vec<ConjData>) {
    let mut guard = CONJ_DATA_CACHE.lock().unwrap();
    guard.get_or_insert_with(HashMap::new).insert(key, val);
}

/// `clear_scoring_caches` counterpart — drop the conj-data cache.
pub fn clear_conj_cache() {
    *CONJ_DATA_CACHE.lock().unwrap() = None;
}

/// `get_conj_data` — fetch conjugation records for an entry.
pub fn get_conj_data(
    conn: &Connection,
    seq: i64,
    from_seq: Option<i64>,
    conj_ids: Option<&[i64]>,
    texts: Option<&[String]>,
) -> Vec<ConjData> {
    let key: CacheKey = (
        seq,
        from_seq,
        conj_ids.map(|v| {
            let mut v = v.to_vec();
            v.sort_unstable();
            v
        }),
        texts.map(|t| {
            let mut t = t.to_vec();
            t.sort();
            t
        }),
    );
    if let Some(hit) = cache_get(&key) {
        return hit;
    }

    // SELECT id, seq, "from", via FROM conjugation WHERE seq = ? ...
    let mut sql = String::from("SELECT id, seq, \"from\", via FROM conjugation WHERE seq = ?1");
    let mut params: Vec<rusqlite::types::Value> = vec![seq.into()];
    if let Some(fs) = from_seq {
        sql.push_str(" AND \"from\" = ?");
        params.push(fs.into());
    }
    if let Some(ids) = conj_ids {
        if !ids.is_empty() {
            sql.push_str(&format!(
                " AND id IN ({})",
                ids.iter().map(|_| "?").collect::<Vec<_>>().join(",")
            ));
            params.extend(ids.iter().map(|i| (*i).into()));
        }
    }
    let conjs: Vec<ConjugationRow> = (|| -> rusqlite::Result<Vec<ConjugationRow>> {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
            Ok(ConjugationRow {
                id: r.get(0)?,
                seq: r.get(1)?,
                from_seq: r.get(2)?,
                via: r.get(3)?,
            })
        })?;
        rows.collect()
    })()
    .unwrap_or_default();

    let conjs: Vec<ConjugationRow> = conjs
        .into_iter()
        .filter(|c| !BLOCKED_CONJUGATIONS.contains(&(c.seq, c.from_seq)))
        .collect();
    if conjs.is_empty() {
        cache_put(key, Vec::new());
        return Vec::new();
    }

    let conj_id_list: Vec<i64> = conjs.iter().map(|c| c.id).collect();
    let ph = conj_id_list.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let params_v: Vec<rusqlite::types::Value> =
        conj_id_list.iter().map(|i| (*i).into()).collect();

    // src readings: SELECT conj_id, text, source_text FROM conj_source_reading
    let mut src_sql = format!(
        "SELECT conj_id, text, source_text FROM conj_source_reading WHERE conj_id IN ({})",
        ph
    );
    let mut src_params = params_v.clone();
    if let Some(t) = texts {
        if !t.is_empty() {
            src_sql.push_str(&format!(
                " AND text IN ({})",
                t.iter().map(|_| "?").collect::<Vec<_>>().join(",")
            ));
            src_params.extend(t.iter().map(|s| s.clone().into()));
        }
    }
    let mut src_by_conj: HashMap<i64, Vec<(String, String)>> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare(&src_sql) {
        if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(src_params), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        }) {
            for r in rows.flatten() {
                src_by_conj.entry(r.0).or_default().push((r.1, r.2));
            }
        }
    }

    // props
    let prop_sql = format!(
        "SELECT id, conj_id, conj_type, pos, neg, fml FROM conj_prop WHERE conj_id IN ({})",
        ph
    );
    let mut props_by_conj: HashMap<i64, Vec<ConjPropRow>> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare(&prop_sql) {
        if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(params_v), |r| {
            Ok(ConjPropRow {
                id: r.get(0)?,
                conj_id: r.get(1)?,
                conj_type: r.get(2)?,
                pos: r.get(3)?,
                neg: r.get(4)?,
                fml: r.get(5)?,
            })
        }) {
            for r in rows.flatten() {
                props_by_conj.entry(r.conj_id).or_default().push(r);
            }
        }
    }

    let mut result = Vec::new();
    for conj in &conjs {
        let src_map = src_by_conj.get(&conj.id).cloned().unwrap_or_default();
        if texts.is_some() && src_map.is_empty() {
            continue;
        }
        if let Some(props) = props_by_conj.get(&conj.id) {
            for prop in props {
                result.push(ConjData {
                    seq: conj.seq,
                    from_seq: conj.from_seq,
                    via: conj.via,
                    prop: Some(prop.clone()),
                    src_map: src_map.clone(),
                });
            }
        }
    }
    cache_put(key, result.clone());
    result
}

/// `get_word_conj_data` — for compounds, conjugation of the LAST component.
pub fn get_word_conj_data(conn: &Connection, word: &Word) -> Vec<ConjData> {
    match word {
        Word::Compound(c) => {
            if let Some(last) = c.words.last() {
                return get_word_conj_data(conn, &Word::Simple(last.clone()));
            }
            Vec::new()
        }
        Word::Counter(_) => Vec::new(),
        Word::Simple(w) => {
            let seq = w.seq();
            match &w.conjugations {
                Conj::Ids(ids) => get_conj_data(
                    conn,
                    seq,
                    None,
                    Some(ids),
                    Some(&[w.text().to_string()]),
                ),
                Conj::Root => Vec::new(),
                Conj::Unset => get_conj_data(
                    conn,
                    seq,
                    None,
                    None,
                    Some(&[w.text().to_string()]),
                ),
            }
        }
    }
}

/// `get_conj_type_name`.
pub fn get_conj_type_name(conn: &Connection, word: &Word) -> Option<String> {
    let conj_data = get_word_conj_data(conn, word);
    let cd = conj_data.first()?;
    cd.prop.as_ref().map(|p| conj_type_name(p.conj_type))
}

/// `get_conj_neg`.
pub fn get_conj_neg(conn: &Connection, word: &Word) -> bool {
    get_word_conj_data(conn, word)
        .first()
        .and_then(|cd| cd.prop.as_ref())
        .and_then(|p| p.neg)
        .unwrap_or(false)
}

/// `get_conj_fml`.
pub fn get_conj_fml(conn: &Connection, word: &Word) -> bool {
    get_word_conj_data(conn, word)
        .first()
        .and_then(|cd| cd.prop.as_ref())
        .and_then(|p| p.fml)
        .unwrap_or(false)
}

/// `get_source_text` — dictionary form matching word.text in src_map.
pub fn get_source_text(conn: &Connection, word: &Word) -> Option<String> {
    let conj_data = get_word_conj_data(conn, word);
    let word_text = word.text();
    for cd in &conj_data {
        for (text, src_text) in &cd.src_map {
            if text == word_text {
                return Some(src_text.clone());
            }
        }
    }
    None
}
