//! Word-surface index — replaces the Python marisa trie (`word_index.py`).
//!
//! Python only ever calls `trie_has_prefix(...)`'s sibling `trie_match` as an
//! exact-membership gate (trie_has_prefix is dead code there), so we use an
//! `fst::Set` of all kanji_text/kana_text surfaces, built from the DB and
//! cached as `himotoki.fst` beside it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fst::{Set, SetBuilder};

pub struct WordIndex {
    set: Set<Vec<u8>>,
}

impl WordIndex {
    pub fn load_or_build(db_path: &Path, conn: &rusqlite::Connection) -> Self {
        let fst_path = fst_path_for(db_path);
        if let Ok(bytes) = std::fs::read(&fst_path) {
            if let Ok(set) = Set::new(bytes) {
                if index_fresh(db_path, &fst_path) {
                    return WordIndex { set };
                }
            }
        }
        let set = build_from_db(conn, &fst_path).unwrap_or_default();
        WordIndex { set }
    }

    pub fn none() -> Self {
        WordIndex {
            set: Set::from_iter(Vec::<String>::new()).unwrap(),
        }
    }

    /// `word_match` / `trie_match` — exact surface membership.
    /// `None` index → "may exist" per plan (caller treats Option<WordIndex>).
    pub fn contains(&self, text: &str) -> bool {
        self.set.contains(text)
    }
}

fn fst_path_for(db_path: &Path) -> PathBuf {
    db_path.with_extension("fst")
}

fn index_fresh(db_path: &Path, fst_path: &Path) -> bool {
    match (
        std::fs::metadata(db_path).and_then(|m| m.modified()),
        std::fs::metadata(fst_path).and_then(|m| m.modified()),
    ) {
        (Ok(db_m), Ok(fst_m)) => fst_m >= db_m,
        _ => false,
    }
}

fn build_from_db(conn: &rusqlite::Connection, fst_path: &Path) -> rusqlite::Result<Set<Vec<u8>>> {
    let mut words = BTreeSet::new();
    for table in ["kanji_text", "kana_text"] {
        let mut stmt = conn.prepare(&format!("SELECT DISTINCT text FROM {}", table))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for r in rows.flatten() {
            words.insert(r);
        }
    }
    let builder = SetBuilder::memory();
    let mut builder = builder;
    let mut iter = words.into_iter().peekable();
    // FST requires sorted insert — BTreeSet iteration is sorted.
    let mut keys = Vec::with_capacity(iter.len());
    while let Some(w) = iter.next() {
        keys.push(w);
    }
    builder.extend_iter(keys).map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(e))
    })?;
    let bytes = builder.into_inner().map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(e))
    })?;
    let _ = std::fs::write(fst_path, &bytes);
    Set::new(bytes).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}
