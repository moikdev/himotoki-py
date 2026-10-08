//! Suffix handlers — port of `himotoki/grammar/suffix_handlers.rs`.
//!
//! Each handler takes (conn, root, suffix, kf) and returns the primary words
//! the suffix attaches to (Simple matches or recursive CompoundWords).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use rusqlite::Connection;

use crate::constants::*;
use crate::types::Word;

use super::suffixes::{find_word_suffix, SuffixKanaForm};

pub type SuffixHandler =
    Arc<dyn Fn(&Connection, &str, &str, Option<&SuffixKanaForm>) -> Vec<Word> + Send + Sync>;

// ============================================================================
// Lookup helpers used by handlers
// ============================================================================

fn fwct(conn: &Connection, word: &str, types: &[i64]) -> Vec<Word> {
    crate::lookup::find_word_with_conj_type(conn, word, types)
}

/// `find_word_with_pos` — words having any of the given pos tags.
fn find_word_with_pos(conn: &Connection, word: &str, posi: &[&str]) -> Vec<Word> {
    let words = crate::lookup::find_word(conn, word, false);
    let ph = posi.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT 1 FROM sense_prop WHERE seq = ?1 AND tag='pos' AND text IN ({}) LIMIT 1",
        ph
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let params: Vec<rusqlite::types::Value> =
        posi.iter().map(|s| rusqlite::types::Value::from((*s).to_string())).collect();
    words
        .into_iter()
        .filter(|w| {
            let mut p: Vec<rusqlite::types::Value> = vec![w.seq().into()];
            p.extend(params.iter().cloned());
            stmt.query_row(rusqlite::params_from_iter(p), |_| Ok(()))
                .is_ok()
        })
        .map(Word::Simple)
        .collect()
}

/// `find_word_with_neg_prop` — words with a negative conjugation.
fn find_word_with_neg_prop(conn: &Connection, word: &str) -> Vec<Word> {
    crate::lookup::find_word_with_conj_prop(
        conn,
        word,
        |cd| cd.prop.as_ref().and_then(|p| p.neg).unwrap_or(false),
        false,
    )
}

/// `_find_word_with_neg_prop_filtered` — neg-prop minus blocked from_seqs.
fn neg_prop_filtered(
    conn: &Connection,
    word: &str,
    blocked: &std::collections::HashSet<i64>,
    allow_root: bool,
) -> Vec<Word> {
    crate::lookup::find_word_with_conj_prop(
        conn,
        word,
        |cd| {
            cd.prop.as_ref().and_then(|p| p.neg).unwrap_or(false)
                && !blocked.contains(&cd.from_seq)
        },
        allow_root,
    )
}

fn find_word_suffix_simple(conn: &Connection, word: &str) -> Vec<Word> {
    find_word_suffix(conn, word, &[], None, None, 0)
}

// ============================================================================
// Handlers
// ============================================================================

fn h_tai(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if root == "い" {
        return Vec::new();
    }
    fwct(conn, root, &[13])
}

fn h_ren(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    fwct(conn, root, &[13])
}

fn h_neg(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    fwct(conn, root, &[13, CONJ_NEGATIVE_STEM])
}

/// chau/to contracted handlers share te-form reconstruction.
fn contracted_te(root: &str, suffix: &str, c: char, t: char) -> Option<String> {
    match suffix.chars().next() {
        Some(f) if f == c => Some(format!("{}て", root)),
        Some(f) if f == t => Some(format!("{}で", root)),
        _ => None,
    }
}

fn h_chau(conn: &Connection, root: &str, suffix: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if suffix.is_empty() {
        return Vec::new();
    }
    match contracted_te(root, suffix, 'ち', 'じ') {
        Some(te_form) => fwct(conn, &te_form, &[3]),
        None => Vec::new(),
    }
}

fn h_to(conn: &Connection, root: &str, suffix: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if suffix.is_empty() {
        return Vec::new();
    }
    match contracted_te(root, suffix, 'と', 'ど') {
        Some(te_form) => fwct(conn, &te_form, &[3]),
        None => Vec::new(),
    }
}

fn h_te(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if root == "で" || !(root.ends_with('て') || root.ends_with('で')) {
        return Vec::new();
    }
    fwct(conn, root, &[3])
}

fn h_teiru(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if root == "いて" || !(root.ends_with('て') || root.ends_with('で')) {
        return Vec::new();
    }
    let results = fwct(conn, root, &[3]);
    if !results.is_empty() {
        return results;
    }
    find_word_suffix_simple(conn, root)
}

fn h_suru(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    find_word_with_pos(conn, root, &["vs"])
}

fn h_sou(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if ["な", "よ", "よさ", "に", "き"].contains(&root) {
        return Vec::new();
    }
    if root.ends_with("なさ") {
        let patched = format!("{}い", &root[..root.len() - 'さ'.len_utf8()]);
        return neg_prop_no_block(conn, &patched);
    }
    let mut results: Vec<Word> =
        fwct(conn, root, &[13, CONJ_ADJECTIVE_STEM, CONJ_ADVERBIAL])
            .into_iter()
            .filter(|w| w.seq() != Some(10195060))
            .collect();
    if results.is_empty() {
        results = find_word_with_pos(conn, root, &["adj-na"]);
    }
    results
}

/// find_word_with_neg_prop without blocked filtering (matches Python call in
/// _handler_sou / _handler_sugiru).
fn neg_prop_no_block(conn: &Connection, word: &str) -> Vec<Word> {
    find_word_with_neg_prop(conn, word)
}

fn h_sugiru(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if root == "い" {
        return Vec::new();
    }
    if root.ends_with("なさ") || root.ends_with("無さ") {
        let patched = format!("{}い", &root[..root.len() - 'さ'.len_utf8()]);
        return neg_prop_no_block(conn, &patched);
    }
    let mut results = fwct(conn, root, &[13]);
    results.extend(find_word_with_pos(conn, &format!("{}い", root), &["adj-i"]));
    results.extend(find_word_with_pos(conn, root, &["adj-na"]));
    results
}

fn h_sa(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    let mut r = fwct(conn, root, &[CONJ_ADJECTIVE_STEM]);
    r.extend(find_word_with_pos(conn, root, &["adj-na"]));
    r
}

fn h_adv(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    fwct(conn, root, &[CONJ_ADVERBIAL])
}

fn h_kudasai(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if !(root.ends_with('て') || root.ends_with('で')) {
        return Vec::new();
    }
    fwct(conn, root, &[3])
}

fn h_garu(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if ["な", "い", "よ"].contains(&root) {
        return Vec::new();
    }
    let mut result = fwct(conn, root, &[CONJ_ADJECTIVE_STEM]);
    if result.is_empty() && root.ends_with('た') {
        result.extend(find_word_suffix_simple(conn, &format!("{}い", root)));
    }
    if root.ends_with('そ') {
        // NB: Python calls find_word_with_suffix() which doesn't exist
        // (latent NameError). Intent: sou-suffix match on root+う.
        let patched = format!("{}う", &root[..root.len() - 'そ'.len_utf8()]);
        result.extend(h_sou(conn, &patched, "そう", None));
    }
    result
}

fn h_nade(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    find_word_with_pos(conn, root, &["adj-na"])
}

fn h_ra(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if root.ends_with('ら') {
        return Vec::new();
    }
    find_word_with_pos(conn, root, &["pn"])
}

fn h_ppoi(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    let mut r = fwct(conn, root, &[13]);
    r.extend(find_word_with_pos(conn, root, &["n"]));
    r.extend(find_word_with_pos(conn, root, &["adj-na"]));
    r
}

fn h_mi(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    let mut r = fwct(conn, root, &[CONJ_ADJECTIVE_STEM]);
    r.extend(find_word_with_pos(conn, root, &["adj-na"]));
    r
}

fn h_tachi(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    let mut r = find_word_with_pos(conn, root, &["n"]);
    r.extend(find_word_with_pos(conn, root, &["pn"]));
    r
}

fn h_rashii(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    let mut r = fwct(conn, root, &[2]);
    r.extend(fwct(conn, &format!("{}ら", root), &[11]));
    r.extend(find_word_with_pos(conn, root, &["n"]));
    r.extend(find_word_with_pos(conn, root, &["adj-na"]));
    r
}

fn h_desu(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if root.ends_with("ない") || root.ends_with("なかった") {
        return find_word_with_neg_prop(conn, root);
    }
    if root.chars().count() < 2 {
        return Vec::new();
    }
    find_word_with_pos(conn, root, &["adj-na"])
}

fn h_tosuru(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    fwct(conn, root, &[9])
}

fn h_kurai(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    let mut r = fwct(conn, root, &[2]);
    r.extend(fwct(conn, root, &[13]));
    r.extend(find_word_with_pos(conn, root, &["v1"]));
    r.extend(find_word_with_pos(conn, root, &["v5"]));
    r.extend(find_word_with_pos(conn, root, &["adj-i"]));
    r.extend(find_word_with_pos(conn, root, &["adj-na"]));
    r
}

fn h_iadj(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    fwct(conn, root, &[CONJ_ADJECTIVE_STEM])
}

// --- abbreviation handlers ---

fn h_abbr_nai(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    neg_prop_filtered(conn, &format!("{}ない", root), &BLOCKED_NAI_SEQS, true)
}

fn h_abbr_nai_n(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    neg_prop_filtered(conn, &format!("{}ない", root), &BLOCKED_NAI_SEQS, false)
}

fn h_abbr_nx(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    if root == "せ" {
        return crate::grammar::splits::find_word_conj_of(conn, "しない", &[SEQ_SURU])
            .into_iter()
            .map(Word::Simple)
            .collect();
    }
    crate::lookup::find_word_with_conj_prop(
        conn,
        &format!("{}ない", root),
        |cd| {
            cd.prop.as_ref().and_then(|p| p.neg).unwrap_or(false)
                && !BLOCKED_NAI_X_SEQS.contains(&cd.from_seq)
        },
        false,
    )
}

fn h_abbr_nakereba(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    crate::lookup::find_word_full(conn, &format!("{}なければ", root), false)
}

fn h_abbr_shimasho(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    crate::lookup::find_word_full(conn, &format!("{}ましょう", root), false)
}

fn h_abbr_dewanai(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    crate::lookup::find_word_full(conn, &format!("{}ではない", root), false)
}

fn h_abbr_eba(conn: &Connection, root: &str, suffix: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    let full = match suffix {
        "ちゃ" => "てば",
        "りゃ" => "れば",
        "きゃ" => "けば",
        "ぎゃ" => "げば",
        "にゃ" => "ねば",
        "びゃ" => "べば",
        "みゃ" => "めば",
        "しゃ" => "せば",
        _ => return Vec::new(),
    };
    crate::lookup::find_word_full(conn, &format!("{}{}", root, full), false)
}

fn h_abbr_ii(conn: &Connection, root: &str, _s: &str, _kf: Option<&SuffixKanaForm>) -> Vec<Word> {
    crate::lookup::find_word_full(conn, &format!("{}いい", root), false)
}

// ============================================================================
// Handler map
// ============================================================================

static HANDLERS: LazyLock<HashMap<&'static str, SuffixHandler>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, SuffixHandler> = HashMap::new();
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            m.insert($name, Arc::new($f) as SuffixHandler)
        };
    }
    reg!("tai", h_tai);
    reg!("ren", h_ren);
    reg!("ren+", h_ren);
    reg!("ren-", h_ren);
    reg!("neg", h_neg);
    reg!("te", h_te);
    reg!("teiru", h_teiru);
    reg!("teiru+", h_teiru);
    reg!("te+space", h_te);
    reg!("suru", h_suru);
    reg!("sou", h_sou);
    reg!("sou+", h_sou);
    reg!("sugiru", h_sugiru);
    reg!("sa", h_sa);
    reg!("adv", h_adv);
    reg!("kudasai", h_kudasai);
    reg!("teii", h_kudasai); // _handler_teii == _handler_kudasai body
    reg!("garu", h_garu);
    reg!("teren", h_ren);
    reg!("ra", h_ra);
    reg!("rashii", h_rashii);
    reg!("desu", h_desu);
    reg!("tosuru", h_tosuru);
    reg!("kurai", h_kurai);
    reg!("iadj", h_iadj);
    reg!("mi", h_mi);
    reg!("nade", h_nade);
    reg!("ppoi", h_ppoi);
    reg!("tachi", h_tachi);
    reg!("chau", h_chau);
    reg!("to", h_to);
    reg!("nai", h_abbr_nai);
    reg!("nai-x", h_abbr_nx);
    reg!("nai-n", h_abbr_nai_n);
    reg!("nakereba", h_abbr_nakereba);
    reg!("shimashou", h_abbr_shimasho);
    reg!("dewanai", h_abbr_dewanai);
    reg!("teba", h_abbr_eba);
    reg!("reba", h_abbr_eba);
    reg!("keba", h_abbr_eba);
    reg!("geba", h_abbr_eba);
    reg!("neba", h_abbr_eba);
    reg!("beba", h_abbr_eba);
    reg!("meba", h_abbr_eba);
    reg!("seba", h_abbr_eba);
    reg!("ii", h_abbr_ii);
    m
});

pub fn get_handler(keyword: &str) -> Option<SuffixHandler> {
    HANDLERS.get(keyword).cloned()
}
