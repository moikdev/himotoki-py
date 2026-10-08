//! Word lookup — port of `himotoki/lookup/find_word.py`.

use rusqlite::Connection;

use crate::cache::{word_cache_get, word_cache_put};
use crate::chars::{as_hiragana, is_kana};
use crate::conj::get_word_conj_data;
use crate::db::rows::{KanaTextRow, KanjiTextRow};
use crate::score::{MAX_WORD_LENGTH, SUPPRESS_SINGLE_TOKEN_SEQS};
use crate::types::{Conj, Reading, Word, WordMatch};

fn conn_id(conn: &Connection) -> String {
    conn.path().unwrap_or_default().to_string()
}

/// `find_word` — exact surface match on kanji_text or kana_text.
pub fn find_word(conn: &Connection, word: &str, root_only: bool) -> Vec<WordMatch> {
    let db_id = conn_id(conn);
    if let Some(hit) = word_cache_get(&db_id, word, root_only) {
        return hit;
    }
    if word.chars().count() > MAX_WORD_LENGTH {
        return Vec::new();
    }
    let kana = is_kana(word);
    let (table, extra) = if kana {
        ("kana_text", "best_kanji")
    } else {
        ("kanji_text", "best_kana")
    };
    let sql = if root_only {
        format!(
            "SELECT t.id, t.seq, t.text, t.ord, t.common, t.{extra} \
             FROM {table} t JOIN entry e ON t.seq = e.seq \
             WHERE t.text = ?1 AND e.root_p = 1"
        )
    } else {
        format!("SELECT id, seq, text, ord, common, {extra} FROM {table} WHERE text = ?1")
    };
    let mut matches: Vec<WordMatch> = Vec::new();
    if let Ok(mut stmt) = conn.prepare_cached(&sql) {
        let kana = kana;
        let rows = stmt.query_map([word], move |r| {
            if kana {
                Ok(WordMatch::new(Reading::Kana(KanaTextRow {
                    id: r.get(0)?,
                    seq: r.get(1)?,
                    text: r.get(2)?,
                    ord: r.get(3)?,
                    common: r.get(4)?,
                    best_kanji: r.get(5)?,
                    ..Default::default()
                })))
            } else {
                Ok(WordMatch::new(Reading::Kanji(KanjiTextRow {
                    id: r.get(0)?,
                    seq: r.get(1)?,
                    text: r.get(2)?,
                    ord: r.get(3)?,
                    common: r.get(4)?,
                    best_kana: r.get(5)?,
                    ..Default::default()
                })))
            }
        });
        if let Ok(rows) = rows {
            matches = rows
                .flatten()
                .filter(|w| !SUPPRESS_SINGLE_TOKEN_SEQS.contains(&w.seq()))
                .collect();
        }
    }
    word_cache_put(&db_id, word, root_only, matches.clone());
    matches
}

/// `find_word_as_hiragana` — katakana→hiragana root-only lookup.
pub fn find_word_as_hiragana(
    conn: &Connection,
    word: &str,
    exclude_seqs: &std::collections::HashSet<i64>,
) -> Vec<WordMatch> {
    let hiragana = as_hiragana(word);
    if hiragana == word {
        return Vec::new();
    }
    find_word(conn, &hiragana, true)
        .into_iter()
        .filter(|w| !exclude_seqs.contains(&w.seq()))
        .collect()
}

/// `find_word_full` — find_word + suffix compounds (+ optional hiragana).
/// Returns a mixed list: simple matches then CompoundWords.
pub fn find_word_full(conn: &Connection, word: &str, as_hiragana_lookup: bool) -> Vec<Word> {
    let simple_words = find_word(conn, word, false);
    let mut results: Vec<Word> = simple_words.iter().cloned().map(Word::Simple).collect();
    let suffix_words =
        crate::grammar::suffixes::find_word_suffix(conn, word, &simple_words, None, None, 0);
    results.extend(suffix_words);
    if as_hiragana_lookup {
        let exclude: std::collections::HashSet<i64> =
            simple_words.iter().map(|w| w.seq()).collect();
        results.extend(
            find_word_as_hiragana(conn, word, &exclude)
                .into_iter()
                .map(Word::Simple),
        );
    }
    results
}

/// `find_word_with_conj_prop` — filter matches by a ConjData predicate.
/// Sets `conjugations` on each kept match (CompoundWord: last component).
pub fn find_word_with_conj_prop<F>(
    conn: &Connection,
    word: &str,
    filter_fn: F,
    allow_root: bool,
) -> Vec<Word>
where
    F: Fn(&crate::types::ConjData) -> bool,
{
    let mut results = Vec::new();
    for mut w in find_word_full(conn, word, false) {
        let conj_data = get_word_conj_data(conn, &w);
        let filtered: Vec<&crate::types::ConjData> =
            conj_data.iter().filter(|cd| filter_fn(cd)).collect();
        let conj_ids: Vec<i64> = filtered
            .iter()
            .filter_map(|cd| cd.prop.as_ref().map(|p| p.conj_id))
            .collect();
        if !filtered.is_empty() || (conj_data.is_empty() && allow_root) {
            let conj = if conj_ids.is_empty() {
                Conj::Unset
            } else {
                Conj::Ids(conj_ids)
            };
            match &mut w {
                Word::Simple(m) => m.conjugations = conj,
                Word::Compound(c) => {
                    if let Some(last) = c.words.last_mut() {
                        last.conjugations = conj;
                    }
                }
                Word::Counter(_) => {}
            }
            results.push(w);
        }
    }
    results
}

/// `find_word_with_conj_type`.
pub fn find_word_with_conj_type(conn: &Connection, word: &str, conj_types: &[i64]) -> Vec<Word> {
    find_word_with_conj_prop(
        conn,
        word,
        |cd| {
            cd.prop
                .as_ref()
                .map(|p| conj_types.contains(&p.conj_type))
                .unwrap_or(false)
        },
        false,
    )
}
