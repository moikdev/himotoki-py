//! Scoring caches — port of `himotoki/scoring/caches.py`.
//!
//! Python uses LRUCache instances; eviction is purely a memory bound and never
//! changes results, so plain `Mutex<HashMap>` statics are parity-equivalent.
//!
//! Caches (module-level):
//!   CONJ_DATA — in conj.rs (co-located with get_conj_data)
//!   POS_SEQ   — seq → posi set
//!   UK        — frozenset(seqs) → prefer-kana bool
//!   WORD      — (db_path, word, root_only) → WordMatch list
//!   ENTRY     — seq → EntryRow
//!   ARCHAIC   — populated once via build_archaic_cache
//!   READINGS  — per-call ReadingsCache for output layer (word_info preload)

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use rusqlite::Connection;

use crate::db::rows::{EntryRow, KanaTextRow, KanjiTextRow};
use crate::types::{Reading, WordMatch};

static POS_SEQ_CACHE: Mutex<Option<HashMap<i64, HashSet<String>>>> = Mutex::new(None);
static UK_CACHE: Mutex<Option<HashMap<Vec<i64>, bool>>> = Mutex::new(None);
static WORD_CACHE: Mutex<Option<HashMap<(String, String, bool), Vec<WordMatch>>>> =
    Mutex::new(None);
static ENTRY_CACHE: Mutex<Option<HashMap<i64, EntryRow>>> = Mutex::new(None);
static ARCHAIC_CACHE: OnceLock<HashSet<i64>> = OnceLock::new();

/// `clear_scoring_caches` — drop everything (DB switch).
pub fn clear_scoring_caches() {
    *POS_SEQ_CACHE.lock().unwrap() = None;
    *UK_CACHE.lock().unwrap() = None;
    *WORD_CACHE.lock().unwrap() = None;
    *ENTRY_CACHE.lock().unwrap() = None;
    crate::conj::clear_conj_cache();
}

// ============================================================================
// Entry cache
// ============================================================================

/// `get_cached_entry` — EntryRow for seq or None.
pub fn get_cached_entry(conn: &Connection, seq: i64) -> Option<EntryRow> {
    {
        let guard = ENTRY_CACHE.lock().unwrap();
        if let Some(e) = guard.as_ref().and_then(|m| m.get(&seq)) {
            return Some(e.clone());
        }
    }
    let entry: Option<EntryRow> = conn
        .query_row(
            "SELECT seq, root_p, n_kanji, n_kana, primary_nokanji FROM entry WHERE seq = ?1",
            [seq],
            |r| {
                Ok(EntryRow {
                    seq: r.get(0)?,
                    root_p: r.get::<_, i64>(1)? != 0,
                    n_kanji: r.get(2)?,
                    n_kana: r.get(3)?,
                    primary_nokanji: r.get::<_, i64>(4)? != 0,
                })
            },
        )
        .ok();
    if let Some(e) = &entry {
        ENTRY_CACHE
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(seq, e.clone());
    }
    entry
}

/// `preload_scoring_caches` — batch-fill ENTRY/UK/POS caches for seqs.
pub fn preload_scoring_caches(conn: &Connection, seqs: &HashSet<i64>) {
    if seqs.is_empty() {
        return;
    }
    // Entries
    let missing: Vec<i64> = {
        let guard = ENTRY_CACHE.lock().unwrap();
        let cached: HashSet<i64> = guard
            .as_ref()
            .map(|m| m.keys().copied().collect())
            .unwrap_or_default();
        seqs.iter()
            .copied()
            .filter(|s| !cached.contains(s))
            .collect()
    };
    if !missing.is_empty() {
        let ph = missing.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT seq, root_p, n_kanji, n_kana, primary_nokanji FROM entry WHERE seq IN ({})",
            ph
        );
        if let Ok(mut stmt) = conn.prepare_cached(&sql) {
            let params: Vec<rusqlite::types::Value> = missing.iter().map(|i| (*i).into()).collect();
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(params), |r| {
                Ok(EntryRow {
                    seq: r.get(0)?,
                    root_p: r.get::<_, i64>(1)? != 0,
                    n_kanji: r.get(2)?,
                    n_kana: r.get(3)?,
                    primary_nokanji: r.get::<_, i64>(4)? != 0,
                })
            }) {
                let mut guard = ENTRY_CACHE.lock().unwrap();
                let m = guard.get_or_insert_with(HashMap::new);
                for r in rows.flatten() {
                    m.insert(r.seq, r);
                }
            }
        }
    }

    // UK: Python caches per-single-seq frozensets
    let uk_missing: Vec<i64> = {
        let guard = UK_CACHE.lock().unwrap();
        seqs.iter()
            .copied()
            .filter(|s| {
                !guard
                    .as_ref()
                    .map(|m| m.contains_key(&vec![*s]))
                    .unwrap_or(false)
            })
            .collect()
    };
    if !uk_missing.is_empty() {
        let ph = uk_missing.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT DISTINCT seq FROM sense_prop WHERE seq IN ({}) AND tag='misc' AND text='uk'",
            ph
        );
        let mut uk_seqs: HashSet<i64> = HashSet::new();
        if let Ok(mut stmt) = conn.prepare_cached(&sql) {
            let params: Vec<rusqlite::types::Value> =
                uk_missing.iter().map(|i| (*i).into()).collect();
            if let Ok(rows) =
                stmt.query_map(rusqlite::params_from_iter(params), |r| r.get::<_, i64>(0))
            {
                for r in rows.flatten() {
                    uk_seqs.insert(r);
                }
            }
        }
        let mut guard = UK_CACHE.lock().unwrap();
        let m = guard.get_or_insert_with(HashMap::new);
        for seq in &uk_missing {
            m.insert(vec![*seq], uk_seqs.contains(seq));
        }
    }

    // POS tags (excluding archaic senses)
    let pos_missing: Vec<i64> = {
        let guard = POS_SEQ_CACHE.lock().unwrap();
        seqs.iter()
            .copied()
            .filter(|s| !guard.as_ref().map(|m| m.contains_key(s)).unwrap_or(false))
            .collect()
    };
    if !pos_missing.is_empty() {
        let ph = pos_missing
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT seq, text FROM sense_prop WHERE seq IN ({}) AND tag='pos' \
             AND sense_id NOT IN (SELECT sense_id FROM sense_prop WHERE tag='misc' AND text IN ('arch','obsc','rare'))",
            ph
        );
        let mut by_seq: HashMap<i64, HashSet<String>> = HashMap::new();
        if let Ok(mut stmt) = conn.prepare_cached(&sql) {
            let params: Vec<rusqlite::types::Value> =
                pos_missing.iter().map(|i| (*i).into()).collect();
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(params), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            }) {
                for r in rows.flatten() {
                    by_seq.entry(r.0).or_default().insert(r.1);
                }
            }
        }
        let mut guard = POS_SEQ_CACHE.lock().unwrap();
        let m = guard.get_or_insert_with(HashMap::new);
        for seq in pos_missing {
            m.entry(seq)
                .or_insert_with(|| by_seq.remove(&seq).unwrap_or_default());
        }
    }
}

// ============================================================================
// Archaic detection
// ============================================================================

/// `build_archaic_cache` — seqs where EVERY sense has arch/obsc/rare,
/// plus seqs conjugated from them.
fn build_archaic_cache(conn: &Connection) -> HashSet<i64> {
    let mut arch: HashSet<i64> = HashSet::new();
    let sql = "SELECT seq FROM sense GROUP BY seq HAVING COUNT(id) = SUM(\
               id IN (SELECT sense_id FROM sense_prop WHERE tag='misc' AND text IN ('arch','obsc','rare')))";
    if let Ok(mut stmt) = conn.prepare_cached(sql) {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, i64>(0)) {
            for r in rows.flatten() {
                arch.insert(r);
            }
        }
    }
    if !arch.is_empty() {
        let ph = arch.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT DISTINCT seq FROM conjugation WHERE \"from\" IN ({})",
            ph
        );
        if let Ok(mut stmt) = conn.prepare_cached(&sql) {
            let params: Vec<rusqlite::types::Value> = arch.iter().map(|i| (*i).into()).collect();
            if let Ok(rows) =
                stmt.query_map(rusqlite::params_from_iter(params), |r| r.get::<_, i64>(0))
            {
                for r in rows.flatten() {
                    arch.insert(r);
                }
            }
        }
    }
    arch
}

/// `is_arch` — all seqs archaic.
pub fn is_arch(conn: &Connection, seq_set: &HashSet<i64>) -> bool {
    let cache = ARCHAIC_CACHE.get_or_init(|| build_archaic_cache(conn));
    seq_set.iter().all(|s| cache.contains(s))
}

/// `is_prefer_kana` — any seq in set has a 'uk' misc tag. Cached by sorted seq vec.
pub fn is_prefer_kana(conn: &Connection, seq_set: &[i64]) -> bool {
    let mut key: Vec<i64> = seq_set.to_vec();
    key.sort_unstable();
    key.dedup();
    {
        let guard = UK_CACHE.lock().unwrap();
        if let Some(v) = guard.as_ref().and_then(|m| m.get(&key)) {
            return *v;
        }
    }
    if key.is_empty() {
        return false;
    }
    let ph = key.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT 1 FROM sense_prop WHERE seq IN ({}) AND tag='misc' AND text='uk' LIMIT 1",
        ph
    );
    let params: Vec<rusqlite::types::Value> = key.iter().map(|i| (*i).into()).collect();
    let result = conn
        .prepare_cached(&sql)
        .and_then(|mut s| s.query_row(rusqlite::params_from_iter(params), |_| Ok(())))
        .is_ok();
    UK_CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(key, result);
    result
}

/// `get_non_arch_posi` — union of per-seq posi sets, excluding archaic senses.
pub fn get_non_arch_posi(conn: &Connection, seq_set: &HashSet<i64>) -> HashSet<String> {
    let missing: Vec<i64> = {
        let guard = POS_SEQ_CACHE.lock().unwrap();
        seq_set
            .iter()
            .copied()
            .filter(|s| !guard.as_ref().map(|m| m.contains_key(s)).unwrap_or(false))
            .collect()
    };
    if !missing.is_empty() {
        let ph = missing.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT seq, text FROM sense_prop WHERE seq IN ({}) AND tag='pos' \
             AND sense_id NOT IN (SELECT sense_id FROM sense_prop WHERE tag='misc' AND text IN ('arch','obsc','rare'))",
            ph
        );
        let mut by_seq: HashMap<i64, HashSet<String>> =
            missing.iter().map(|s| (*s, HashSet::new())).collect();
        if let Ok(mut stmt) = conn.prepare_cached(&sql) {
            let params: Vec<rusqlite::types::Value> = missing.iter().map(|i| (*i).into()).collect();
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(params), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            }) {
                for r in rows.flatten() {
                    by_seq.entry(r.0).or_default().insert(r.1);
                }
            }
        }
        let mut guard = POS_SEQ_CACHE.lock().unwrap();
        let m = guard.get_or_insert_with(HashMap::new);
        for (seq, posi) in by_seq {
            m.insert(seq, posi);
        }
    }
    let guard = POS_SEQ_CACHE.lock().unwrap();
    let mut result = HashSet::new();
    if let Some(m) = guard.as_ref() {
        for seq in seq_set {
            if let Some(posi) = m.get(seq) {
                result.extend(posi.iter().cloned());
            }
        }
    }
    result
}

// ============================================================================
// Word cache (find_word)
// ============================================================================

pub fn word_cache_get(db_id: &str, word: &str, root_only: bool) -> Option<Vec<WordMatch>> {
    let guard = WORD_CACHE.lock().unwrap();
    guard
        .as_ref()?
        .get(&(db_id.to_string(), word.to_string(), root_only))
        .cloned()
}

pub fn word_cache_put(db_id: &str, word: &str, root_only: bool, matches: Vec<WordMatch>) {
    WORD_CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert((db_id.to_string(), word.to_string(), root_only), matches);
}

// ============================================================================
// ReadingsCache — per-call batch cache for the output layer
// ============================================================================

/// `ReadingsCache` — grouped readings by seq (word_info.py preload).
#[derive(Default)]
pub struct ReadingsCache {
    pub kanji: HashMap<i64, Vec<KanjiTextRow>>,
    pub kana: HashMap<i64, Vec<KanaTextRow>>,
}

impl ReadingsCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// `cache.preload(session, seqs)`.
    pub fn preload(&mut self, conn: &Connection, seqs: &HashSet<i64>) -> rusqlite::Result<()> {
        if seqs.is_empty() {
            return Ok(());
        }
        let ph = seqs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let params: Vec<rusqlite::types::Value> = seqs.iter().map(|s| (*s).into()).collect();
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT id, seq, text, ord, common, best_kana, nokanji FROM kanji_text WHERE seq IN ({}) ORDER BY seq, ord",
            ph
        ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params.clone()), |r| {
            Ok(KanjiTextRow {
                id: r.get(0)?,
                seq: r.get(1)?,
                text: r.get(2)?,
                ord: r.get(3)?,
                common: r.get(4)?,
                best_kana: r.get(5)?,
                nokanji: r.get::<_, Option<i64>>(6)?.map(|v| v != 0),
                ..Default::default()
            })
        })?;
        for r in rows.flatten() {
            self.kanji.entry(r.seq).or_default().push(r);
        }
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT id, seq, text, ord, common, best_kanji, nokanji FROM kana_text WHERE seq IN ({}) ORDER BY seq, ord",
            ph
        ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
            Ok(KanaTextRow {
                id: r.get(0)?,
                seq: r.get(1)?,
                text: r.get(2)?,
                ord: r.get(3)?,
                common: r.get(4)?,
                best_kanji: r.get(5)?,
                nokanji: r.get::<_, Option<i64>>(6)?.map(|v| v != 0),
                ..Default::default()
            })
        })?;
        for r in rows.flatten() {
            self.kana.entry(r.seq).or_default().push(r);
        }
        Ok(())
    }

    pub fn get_kanji(&self, seq: i64) -> &[KanjiTextRow] {
        self.kanji.get(&seq).map(|v| v.as_slice()).unwrap_or(&[])
    }
    pub fn get_kana(&self, seq: i64) -> &[KanaTextRow] {
        self.kana.get(&seq).map(|v| v.as_slice()).unwrap_or(&[])
    }
    /// All readings for seq (kanji then kana).
    pub fn get_all(&self, seq: i64) -> Vec<Reading> {
        let mut out: Vec<Reading> = Vec::new();
        out.extend(self.get_kanji(seq).iter().cloned().map(Reading::Kanji));
        out.extend(self.get_kana(seq).iter().cloned().map(Reading::Kana));
        out
    }
}

/// `GenScoreCache` — counter calc score by seq (per-call).
#[derive(Default)]
pub struct GenScoreCache {
    pub map: HashMap<Option<i64>, f64>,
}
