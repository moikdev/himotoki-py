//! Scoring — port of `himotoki/scoring/calc_score.py` (ichiran calc-score).
//!
//! Numeric notes vs Python:
//!   * scores are f64; `score // ratio` is floor division (Python `//` on the
//!     mixed int/float chain floors, so we `.floor()`).
//!   * `score_mod` is `&[f64]` — Python's `callable` branch is dead code
//!     (all callers pass float or list-of-float); `apply_score_mod` sums.

use std::collections::HashSet;

use rusqlite::Connection;

use crate::cache::{get_cached_entry, get_non_arch_posi, is_arch, is_prefer_kana};
use crate::chars::{count_char_class, has_kanji, mora_length};
use crate::conj::{get_conj_data, get_word_conj_data};
use crate::constants::{SKIP_CONJ_FORMS, WEAK_CONJ_FORMS};
use crate::db::rows::{ConjPropRow, EntryRow};
use crate::grammar::splits::get_split;
use crate::types::{Conj, ConjData, ScoreInfo, SplitInfo, Word};

// ============================================================================
// Constants (calc_score.py:30-129)
// ============================================================================

pub const MAX_WORD_LENGTH: usize = 50;
pub const SCORE_CUTOFF: f64 = 5.0;
pub const GAP_PENALTY: f64 = -500.0;
pub const IDENTICAL_WORD_SCORE_CUTOFF: f64 = 0.5;

/// length, power -> multiplier; coeff tables 1-based (index 0 unused).
static LENGTH_COEFF_STRONG: &[i64] = &[0, 1, 8, 24, 40, 60];
static LENGTH_COEFF_WEAK: &[i64] = &[0, 1, 4, 9, 16, 25, 36];
static LENGTH_COEFF_TAIL: &[i64] = &[0, 4, 9, 16, 24];
static LENGTH_COEFF_LTAIL: &[i64] = &[0, 4, 12, 18, 24];

lazy_set! {
    pub static COPULAE: HashSet<i64> = { 2089020, 1628500 }  // だ, です
    pub static SKIP_WORDS: HashSet<i64> = {
        2458040, 2822120, 2013800, 2108590, 2029040, 2428180, 2654250,
        2561100, 2210270, 2210710, 2257550, 2210320, 2017560, 2394890,
        2194000, 2568000, 2537250, 2760890, 2831062, 2831063, 2029030,
        2568020, 900000, 11553382, 11566952, 2862334, 1881690, 2860921,
        2862924, 10551045, 10456682, 1465580, 1751450, 2657130, 2838238,
        2260200, 2254970, 1774770, 10253059,
    }
    pub static FINAL_PRT: HashSet<i64> = {
        2017770, 2425930, 2130430, 2029130, 2834812, 2718360, 2201380,
        2722170, 2751630,
    }
    pub static SEMI_FINAL_PRT: HashSet<i64> = {
        2017770, 2425930, 2130430, 2029130, 2834812, 2718360, 2201380,
        2722170, 2751630,   // = FINAL_PRT
        2029120, 2086640, 2029110, 2029080, 2029100,
    }
    pub static NON_FINAL_PRT: HashSet<i64> = { 2139720 }  // ん
    pub static NO_KANJI_BREAK_PENALTY: HashSet<i64> = {
        1169870, 1198360, 1277450, 2028980, 1423000, 1164690, 1587040, 2827864,
    }
}

/// seq → flat score bonus.
pub fn seq_score_bonus(seq: i64) -> i64 {
    match seq {
        10044695 => 10, // でしょう
        _ => 0,
    }
}

// `SUPPRESS_SINGLE_TOKEN_SEQS` (lookup/constants.py).
lazy_set! {
    pub static SUPPRESS_SINGLE_TOKEN_SEQS: HashSet<i64> = { 2825978 }
}

// ============================================================================
// Helpers
// ============================================================================

/// `length_multiplier` — len^power until len_lim, linear after.
pub fn length_multiplier(length: i64, power: f64, len_lim: i64) -> f64 {
    if length <= len_lim {
        (length as f64).powf(power)
    } else {
        (length * len_lim) as f64 * (len_lim as f64).powf(power - 1.0) / len_lim as f64
    }
}

/// `length_multiplier_coeff` — coefficient table lookup with linear tail.
pub fn length_multiplier_coeff(length: i64, coeff_class: &str) -> i64 {
    let coeffs = match coeff_class {
        "strong" => LENGTH_COEFF_STRONG,
        "weak" => LENGTH_COEFF_WEAK,
        "tail" => LENGTH_COEFF_TAIL,
        "ltail" => LENGTH_COEFF_LTAIL,
        _ => return length,
    };
    if 0 < length && (length as usize) < coeffs.len() {
        return coeffs[length as usize];
    }
    let last_coeff = coeffs[coeffs.len() - 1];
    let last_idx = (coeffs.len() - 1) as i64;
    if last_idx > 0 {
        length * (last_coeff / last_idx)
    } else {
        length
    }
}

/// `matches_conj_form` — (conj_type, neg, fml) or (pos, conj_type, neg, fml)
/// patterns where None = wildcard.
pub fn matches_conj_form(prop: &ConjPropRow, forms: &[crate::constants::SkipForm]) -> bool {
    for form in forms {
        let (pos_pat, ct, neg, fml): (Option<&str>, i64, Option<bool>, Option<bool>) = match form {
            crate::constants::SkipForm::Cnf(c, n, f) => (None, *c, *n, *f),
            crate::constants::SkipForm::PosCnf(p, c, n, f) => (Some(*p), *c, *n, *f),
        };
        if let Some(p) = pos_pat {
            if p != prop.pos {
                continue;
            }
        }
        if prop.conj_type != ct {
            continue;
        }
        if let Some(n) = neg {
            if prop.neg != Some(n) {
                continue;
            }
        }
        if let Some(f) = fml {
            if prop.fml != Some(f) {
                continue;
            }
        }
        return true;
    }
    false
}

/// `skip_by_conj_data` — true if ALL conj data matches skip patterns.
pub fn skip_by_conj_data(conj_data: &[ConjData]) -> bool {
    if conj_data.is_empty() {
        return false;
    }
    conj_data.iter().all(|cd| {
        cd.prop
            .as_ref()
            .map(|p| matches_conj_form(p, SKIP_CONJ_FORMS))
            .unwrap_or(false)
    })
}

/// `is_weak_conj_form`.
pub fn is_weak_conj_form(conj_data: &[ConjData]) -> bool {
    if conj_data.is_empty() {
        return false;
    }
    conj_data.iter().all(|cd| {
        cd.prop
            .as_ref()
            .map(|p| matches_conj_form(p, WEAK_CONJ_FORMS))
            .unwrap_or(false)
    })
}

/// `compare_common` — lower common is better; 0 is special (most common),
/// None worst. True if c1 sorts before c2.
pub fn compare_common(c1: Option<i64>, c2: Option<i64>) -> bool {
    if c2.is_none() {
        return c1.is_some();
    }
    if c2 == Some(0) {
        return c1.map(|v| v > 0).unwrap_or(false);
    }
    if let Some(v1) = c1 {
        if v1 > 0 {
            return v1 < c2.unwrap();
        }
    }
    false
}

/// `kanji_break_penalty`.
pub fn kanji_break_penalty(
    kanji_break: &[usize],
    score: f64,
    info: Option<&ScoreInfo>,
    text: &str,
) -> f64 {
    if kanji_break.is_empty() {
        return score;
    }
    let end = if kanji_break.len() > 1 {
        "both"
    } else if kanji_break[0] == 0 {
        "beg"
    } else {
        "end"
    };
    let mut bonus: f64 = 0.0;
    let ratio = 2.0;

    if let Some(info) = info {
        if !info.seq_set.is_disjoint(&NO_KANJI_BREAK_PENALTY) {
            return score;
        }
        if end == "beg" && text.starts_with('す') {
            return score;
        }
        if end == "beg" && info.posi.contains("num") {
            bonus += 5.0;
        } else if end == "beg" && (info.posi.contains("suf") || info.posi.contains("n-suf")) {
            bonus += 10.0;
        } else if end == "end" && info.posi.contains("pref") {
            bonus += 12.0;
        }
    }

    if score >= SCORE_CUTOFF {
        return (SCORE_CUTOFF).max((score / ratio).floor() + bonus);
    }
    score
}

/// `apply_score_mod` — sum(score * mod * length) over each element.
/// (Python's callable branch is dead code — every call site passes a float
/// or list of floats.)
pub fn apply_score_mod(score_mod: &[f64], score: f64, length: i64) -> f64 {
    score_mod.iter().map(|m| score * m * length as f64).sum()
}

// ============================================================================
// Conjugation-source text data (get_original_text_data)
// ============================================================================

fn lookup_orig_text(
    conn: &Connection,
    from_seq: i64,
    src_text: &str,
) -> Option<(Option<i64>, i64)> {
    // kanji_text if src_text has kanji else kana_text; return (common, ord)
    let sql = if has_kanji(src_text) {
        "SELECT common, ord FROM kanji_text WHERE seq = ?1 AND text = ?2 LIMIT 1"
    } else {
        "SELECT common, ord FROM kana_text WHERE seq = ?1 AND text = ?2 LIMIT 1"
    };
    conn.prepare_cached(sql)
        .and_then(|mut s| {
            s.query_row(rusqlite::params![from_seq, src_text], |r| {
                Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, i64>(1)?))
            })
        })
        .ok()
}

fn get_original_text_data_recursive(
    conn: &Connection,
    conj_data: &[ConjData],
    texts: &[String],
) -> Vec<(Option<i64>, i64)> {
    let mut result = Vec::new();
    for cd in conj_data {
        let src_texts: Vec<String> = cd
            .src_map
            .iter()
            .filter(|(t, _)| texts.contains(t))
            .map(|(_, s)| s.clone())
            .collect();
        if src_texts.is_empty() {
            continue;
        }
        if cd.via.is_none() {
            for src_text in &src_texts {
                if let Some(co) = lookup_orig_text(conn, cd.from_seq, src_text) {
                    result.push(co);
                }
            }
        } else {
            let via_conj = get_conj_data(conn, cd.via.unwrap(), Some(cd.from_seq), None, None);
            if !via_conj.is_empty() {
                result.extend(get_original_text_data_recursive(
                    conn, &via_conj, &src_texts,
                ));
            }
        }
    }
    result
}

/// `get_original_text_data` — (common, ord) pairs from source texts.
fn get_original_text_data(
    conn: &Connection,
    word: &Word,
    conj_data: &[ConjData],
) -> Vec<(Option<i64>, i64)> {
    get_original_text_data_recursive(conn, conj_data, &[word.text().to_string()])
}

// ============================================================================
// determine_primary_full
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn determine_primary_full_impl(
    conn: &Connection,
    entry: Option<&EntryRow>,
    word: &Word,
    _posi: &HashSet<String>,
    common_p: bool,
    kanji_p: bool,
    prefer_kana: bool,
    conj_types_p: bool,
    cop_da_p: bool,
    pronoun_p: bool,
    ord_val: i64,
) -> bool {
    let entry = match entry {
        Some(e) => e,
        None => return true,
    };

    if prefer_kana && conj_types_p && !kanji_p {
        if !entry.primary_nokanji {
            return true;
        }
        if word.reading().and_then(|r| r.nokanji()).unwrap_or(false) {
            return true;
        }
        if ord_val == 0 && common_p {
            let wc = word.common();
            if wc == Some(0) || wc.map(|c| c < 10).unwrap_or(false) {
                return true;
            }
        }
    }

    if (ord_val == 0 || cop_da_p)
        && (kanji_p || conj_types_p)
            && ((kanji_p && !prefer_kana) || (common_p && pronoun_p) || entry.n_kanji == 0)
        {
            return true;
        }

    if prefer_kana && kanji_p && ord_val == 0 {
        // uk on the ord=0 sense blocks primacy
        let first_sense_uk: bool = conn
            .prepare_cached(
                "SELECT 1 FROM sense_prop sp JOIN sense s ON sp.sense_id = s.id \
                 WHERE s.seq = ?1 AND s.ord = 0 AND sp.tag = 'misc' AND sp.text = 'uk' LIMIT 1",
            )
            .and_then(|mut s| s.query_row([entry.seq], |_| Ok(())))
            .is_ok();
        if !first_sense_uk {
            return true;
        }
    }

    false
}

// ============================================================================
// calc_score
// ============================================================================

/// `calc_score` — returns (score, info).
pub fn calc_score(
    conn: &Connection,
    word: &Word,
    final_: bool,
    use_length: Option<i64>,
    score_mod: &[f64],
    kanji_break: Option<&[usize]>,
) -> (f64, ScoreInfo) {
    // --- CompoundWord: score the base word, then graft compound conj data ---
    if let Word::Compound(cw) = word {
        let base_word = Word::Simple(cw.get_score_base().clone());
        let compound_score_mod = cw.score_mod.clone();
        let compound_use_length = mora_length(&cw.text) as i64;

        let (mut score, mut info) = calc_score(
            conn,
            &base_word,
            false,
            Some(compound_use_length),
            &compound_score_mod,
            None,
        );

        info.conj = get_word_conj_data(conn, word);

        if let Some(kb) = kanji_break {
            score = kanji_break_penalty(kb, score, Some(&info), base_word.text());
        }
        return (score, info);
    }

    let ctr_mode = word.is_counter();

    let seq = word.seq().unwrap_or(0);
    let mut ord_val = word.ord();
    let mut common = word.common();
    let mut common_p = common.is_some();

    // Fast paths (before entry lookup)
    if seq != 0 && SKIP_WORDS.contains(&seq) {
        return (0.0, ScoreInfo::default());
    }
    if seq != 0 && SUPPRESS_SINGLE_TOKEN_SEQS.contains(&seq) {
        return (0.0, ScoreInfo::default());
    }
    if !final_ && seq != 0 && FINAL_PRT.contains(&seq) {
        return (0.0, ScoreInfo::default());
    }

    let entry = if ctr_mode {
        None
    } else if seq != 0 {
        get_cached_entry(conn, seq)
    } else {
        None
    };
    if entry.is_none() && !ctr_mode {
        return (0.0, ScoreInfo::default());
    }

    let text = word.text().to_string();
    let mut score: f64 = 1.0;
    #[allow(unused_assignments)]
    let mut prop_score: f64 = 0.0;

    let kanji_p = word.word_type() == "kanji";
    let katakana_p = !kanji_p && count_char_class(&text, "katakana") > 0;
    let n_kanji = count_char_class(&text, "kanji") as i64;
    let word_len = mora_length(&text).max(1) as i64;

    // Conjugation info
    let conj_only = !ctr_mode && !matches!(word.conjugations(), Conj::Unset | Conj::Root);
    let root_p = ctr_mode || (!conj_only && entry.as_ref().map(|e| e.root_p).unwrap_or(false));

    let mut conj_data: Vec<ConjData> = if ctr_mode {
        Vec::new()
    } else if conj_only {
        match word.conjugations() {
            Conj::Ids(ids) => get_conj_data(conn, seq, None, Some(ids), Some(std::slice::from_ref(&text))),
            _ => Vec::new(),
        }
    } else if !word.is_root() {
        get_conj_data(conn, seq, None, None, Some(std::slice::from_ref(&text)))
    } else {
        Vec::new()
    };

    // Secondary conjugations: if NOT all have via, strip via'd entries.
    let mut secondary_conj_p = false;
    if !conj_data.is_empty() {
        if conj_data.iter().all(|cd| cd.via.is_some()) {
            secondary_conj_p = true;
        } else {
            conj_data.retain(|cd| cd.via.is_none());
        }
    }

    let conj_of: Vec<i64> = conj_data.iter().map(|cd| cd.from_seq).collect();
    let conj_props: Vec<&ConjPropRow> =
        conj_data.iter().filter_map(|cd| cd.prop.as_ref()).collect();
    let conj_types: Vec<i64> = conj_props.iter().map(|cp| cp.conj_type).collect();

    let conj_types_p = root_p
        || use_length.is_some()
        || !conj_props
            .iter()
            .all(|p| matches_conj_form(p, WEAK_CONJ_FORMS));

    // seq_set / sp_seq_set / posi
    let mut seq_set: HashSet<i64> = HashSet::new();
    if seq != 0 {
        seq_set.insert(seq);
    }
    seq_set.extend(conj_of.iter().copied());
    let sp_seq_set: Vec<i64> = if seq != 0 && root_p && use_length.is_none() {
        vec![seq]
    } else {
        seq_set.iter().copied().collect()
    };

    let (prefer_kana, is_arch_p, posi): (bool, bool, HashSet<String>) = if ctr_mode {
        let mut p = HashSet::new();
        p.insert("ctr".to_string());
        (false, false, p)
    } else {
        (
            is_prefer_kana(conn, &sp_seq_set),
            is_arch(conn, &sp_seq_set.iter().copied().collect()),
            get_non_arch_posi(conn, &seq_set),
        )
    };

    let common_of_src = common; // original, before conj-of override
    let particle_p = posi.contains("prt");
    let semi_final_particle_p = seq != 0 && SEMI_FINAL_PRT.contains(&seq);
    let non_final_particle_p = seq != 0 && NON_FINAL_PRT.contains(&seq);
    let pronoun_p = posi.contains("pn");
    let cop_da_p = !seq_set.is_disjoint(&COPULAE);

    // Length classification
    let has_3_or_9 = conj_types.iter().any(|t| *t == 3 || *t == 9);
    let len_threshold: i64 = if kanji_p && !prefer_kana {
        if (root_p && conj_data.is_empty()) || (use_length.is_some() && conj_types.contains(&13)) {
            2
        } else if common_p && common.map(|c| c > 0 && c < 10).unwrap_or(false) {
            2
        } else if has_3_or_9 && use_length.is_none() {
            4
        } else {
            3
        }
    } else {
        if common_p && common.map(|c| c > 0 && c < 10).unwrap_or(false) {
            2
        } else if has_3_or_9 && use_length.is_none() {
            4
        } else {
            3
        }
    };
    let long_p = word_len > len_threshold;

    let no_common_bonus =
        particle_p || !conj_types_p || (!long_p && posi.len() == 1 && posi.contains("int"));

    // Skip-word checks on full seq_set (only for direct matches)
    if use_length.is_none() && !seq_set.is_disjoint(&SKIP_WORDS) {
        return (0.0, ScoreInfo::default());
    }
    if seq != 0 && SKIP_WORDS.contains(&seq) {
        return (0.0, ScoreInfo::default());
    }
    if !root_p && skip_by_conj_data(&conj_data) {
        return (0.0, ScoreInfo::default());
    }

    // Inherited commonness/ord from conjugation source (before primary check)
    let mut common_of = common_of_src;
    if !conj_data.is_empty() && !(ord_val == 0 && common_p) {
        let orig_texts = get_original_text_data(conn, word, &conj_data);
        if !orig_texts.is_empty() {
            if !common_p {
                let conj_of_common: Vec<i64> = orig_texts.iter().filter_map(|(c, _)| *c).collect();
                if !conj_of_common.is_empty() {
                    common = Some(0);
                    common_p = true;
                    // Python: sorted(conj_of_common, key=lambda c: (c or 1000, c == 0))[0]
                    common_of = conj_of_common
                        .iter()
                        .copied()
                        .min_by_key(|c| (if *c == 0 { 1000 } else { *c }, *c == 0));
                }
            }
            let conj_of_ord = orig_texts.iter().map(|(_, o)| *o).min().unwrap();
            if conj_of_ord < ord_val {
                ord_val = conj_of_ord;
            }
        }
    }

    // Primary reading
    let primary_p = if !is_arch_p {
        determine_primary_full_impl(
            conn,
            entry.as_ref(),
            word,
            &posi,
            common_p,
            kanji_p,
            prefer_kana,
            conj_types_p,
            cop_da_p,
            pronoun_p,
            ord_val,
        )
    } else {
        false
    };

    // Base score
    if primary_p {
        if long_p {
            score += 10.0;
        } else if secondary_conj_p && !kanji_p {
            score += 2.0;
        } else if common_p && conj_types_p {
            score += 5.0;
        } else if prefer_kana || entry.as_ref().map(|e| e.n_kanji == 0).unwrap_or(true) {
            score += 3.0;
        } else {
            score += 2.0;
        }
    }

    // Particle bonus
    if particle_p && (final_ || !semi_final_particle_p) {
        score += 2.0;
        if common_p {
            score += 2.0 + word_len as f64;
        }
        if final_ && !non_final_particle_p {
            if primary_p {
                score += 5.0;
            } else if semi_final_particle_p {
                score += 2.0;
            }
        }
    }

    // Commonness bonus
    if common_p && !no_common_bonus {
        let common_bonus: f64 = if secondary_conj_p && use_length.is_none() {
            if kanji_p && primary_p {
                4.0
            } else {
                2.0
            }
        } else if long_p || cop_da_p || (root_p && (kanji_p || (primary_p && word_len > 2))) {
            if common == Some(0) {
                10.0
            } else if !primary_p {
                (15.0 - common.unwrap_or(0) as f64).max(10.0)
            } else {
                (20.0 - common.unwrap_or(0) as f64).max(10.0)
            }
        } else if kanji_p {
            8.0
        } else if primary_p {
            4.0
        } else if word_len > 2 || common.map(|c| c > 0 && c < 10).unwrap_or(false) {
            3.0
        } else {
            2.0
        };
        let common_bonus = if common_bonus >= 10.0 && conj_types.contains(&10) {
            common_bonus - 4.0
        } else {
            common_bonus
        };
        score += common_bonus;
    }

    // Length / kanji bonuses
    if long_p {
        score = score.max(word_len as f64);
    }
    if kanji_p {
        score = score.max(if is_arch_p { 3.0 } else { 5.0 });
        if long_p && (n_kanji > 1 || word_len > 4) {
            score += 2.0;
        }
    }
    if ctr_mode {
        score = score.max(5.0);
    }

    // Entry-specific bonus
    if seq != 0 {
        score += seq_score_bonus(seq) as f64;
    }

    // prop_score + length multiplier
    prop_score = score;
    let length_class = if kanji_p || katakana_p {
        "strong"
    } else {
        "weak"
    };
    score = prop_score
        * (length_multiplier_coeff(word_len, length_class) as f64
            + if n_kanji > 1 {
                (n_kanji - 1) as f64 * 5.0
            } else {
                0.0
            });

    // Split scoring
    let mut split_info: Option<SplitInfo> = None;
    if !ctr_mode {
        let conj_of_opt: Option<Vec<i64>> = if conj_of.is_empty() {
            None
        } else {
            Some(conj_of.clone())
        };
        if let Some(split_result) = get_split(conn, word, conj_of_opt.as_deref()) {
            if split_result.has_modifier(":score") {
                score += split_result.score_bonus;
                split_info = Some(SplitInfo::Score(split_result.score_bonus));
            } else if split_result.has_modifier(":pscore") {
                let new_prop_score = (prop_score + split_result.score_bonus).max(1.0);
                score = if prop_score > 0.0 {
                    (score * new_prop_score / prop_score).ceil()
                } else {
                    score
                };
                prop_score = new_prop_score;
                split_info = Some(SplitInfo::Pscore(split_result.score_bonus));
            } else {
                let mut split_score = split_result.score_bonus;
                let mut part_scores = Vec::new();
                let nparts = split_result.parts.len();
                for (i, part) in split_result.parts.iter().enumerate() {
                    let is_last = i == nparts - 1;
                    let part_use_length = if is_last {
                        use_length.map(|ul| {
                            let preceding: i64 = split_result.parts[..nparts - 1]
                                .iter()
                                .map(|p| mora_length(&p.text) as i64)
                                .sum();
                            ul - preceding
                        })
                    } else {
                        None
                    };
                    let part_mod: Vec<f64> = if is_last {
                        score_mod.to_vec()
                    } else {
                        vec![0.0]
                    };
                    let (part_score, _) = calc_score(
                        conn,
                        &part.word,
                        final_ && is_last,
                        part_use_length,
                        &part_mod,
                        None,
                    );
                    part_scores.push(part_score);
                    split_score += part_score;
                }
                score = split_score;
                split_info = Some(SplitInfo::Split {
                    bonus: split_result.score_bonus,
                    part_scores,
                });
            }
        }
    }

    // use_length bonus
    let mut use_length_bonus: f64 = 0.0;
    if let Some(ul) = use_length {
        let tail_len = ul - word_len;
        let tail_class = if word_len > 3 && (kanji_p || katakana_p) {
            "ltail"
        } else {
            "tail"
        };
        use_length_bonus = prop_score * length_multiplier_coeff(tail_len, tail_class) as f64;
        if !score_mod.is_empty() {
            use_length_bonus += apply_score_mod(score_mod, prop_score, tail_len);
        }
        score += use_length_bonus;
    }

    // info
    let mut info = ScoreInfo {
        posi: posi.clone(),
        seq_set: seq_set.clone(),
        conj: conj_data.clone(),
        common: if common_p { common_of } else { None },
        prop_score,
        kanji_break: kanji_break.map(|k| k.to_vec()).unwrap_or_default(),
        use_length_bonus,
        split_info,
        kpcl: [kanji_p || katakana_p, primary_p, common_p, long_p],
        counter: false,
        ..Default::default()
    };
    if ctr_mode {
        info.counter = true;
    }

    // kanji break penalty
    let mut final_score = score;
    if let Some(kb) = kanji_break {
        final_score = kanji_break_penalty(kb, score, Some(&info), &text);
    }

    (final_score, info)
}

/// `cull_segments` — drop segments below IDENTICAL_WORD_SCORE_CUTOFF * max,
/// but never cull compounds.
pub fn cull_segments(segments: Vec<crate::types::Segment>) -> Vec<crate::types::Segment> {
    if segments.is_empty() {
        return segments;
    }
    let mut segs = segments;
    segs.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let key = |s: &crate::types::Segment| s.info.common.unwrap_or(i64::MAX);
                key(a).cmp(&key(b))
            })
    });
    let max_score = segs.iter().map(|s| s.score).fold(f64::MIN, f64::max);
    let cutoff = max_score * IDENTICAL_WORD_SCORE_CUTOFF;
    segs.retain(|s| s.score >= cutoff || s.word.is_compound());
    segs
}

/// `gen_score` — score a segment in place; preserve counter flag.
pub fn gen_score(
    conn: &Connection,
    segment: &mut crate::types::Segment,
    final_: bool,
    kanji_break: Option<&[usize]>,
) {
    let (score, mut info) = calc_score(conn, &segment.word, final_, None, &[], kanji_break);
    if segment.info.counter {
        info.counter = true;
    }
    segment.score = score;
    segment.info = info;
}

/// `gap_penalty` — per-char penalty for uncovered text.
pub fn gap_penalty(start: usize, end: usize) -> f64 {
    (end - start) as f64 * GAP_PENALTY
}
