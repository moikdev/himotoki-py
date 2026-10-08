//! Rust port of `himotoki/loading/conjugations.py`.
//!
//! Generates conjugation entries/readings/props from conjo.csv rules over
//! the JMdict-loaded tables. Python parallelizes generation (imap_unordered),
//! so generated seq numbers are inherently nondeterministic there; this port
//! processes seqs in sorted order — deterministic and content-equivalent.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

// Custom conj types (constants.py)
const CONJ_ADVERBIAL: i64 = 50;
const CONJ_ADJECTIVE_STEM: i64 = 51;
const CONJ_CAUSATIVE_SU: i64 = 53; // constants.py value (conjo.csv uses 14)
const CONJ_ADJECTIVE_LITERARY: i64 = 54;

pub const POS_WITH_CONJ_RULES: &[&str] = &[
    "adj-i", "adj-ix", "cop", "v1", "v1-s", "v5aru", "v5b", "v5g", "v5k", "v5k-s", "v5m", "v5n",
    "v5r", "v5r-i", "v5s", "v5t", "v5u", "v5u-s", "vk", "vs-s", "vs-i",
];
pub const DO_NOT_CONJUGATE_POS: &[&str] = &["n", "vs", "adj-na"];
pub const DO_NOT_CONJUGATE_SEQ: &[i64] = &[2765070, 2835284];
pub const COP_CONJUGATE_SEQ: &[i64] = &[2089020]; // だ only
pub const SECONDARY_CONJUGATION_TYPES_FROM: &[i64] = &[5, 6, 7, 8, 14];
pub const SECONDARY_CONJUGATION_TYPES: &[i64] = &[2, 3, 4, 9, 10, 11, 12, 13];

// ============================================================================
// CSV loading
// ============================================================================

#[derive(Debug, Clone)]
pub struct ConjugationRule {
    pub pos: i64,
    pub conj: i64,
    pub neg: bool,
    pub fml: bool,
    pub onum: i64,
    pub stem: i64,
    pub okuri: String,
    pub euphr: String,
    pub euphk: String,
    pub pos2: Option<String>,
}

/// `load_pos_index` — kwpos.csv → pos name → (id, description).
pub fn load_pos_index(path: &Path) -> Result<HashMap<String, (i64, String)>> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut out = HashMap::new();
    for rec in rdr.records() {
        let rec = rec?;
        if rec.len() >= 3 {
            let id: i64 = rec[0]
                .parse()
                .with_context(|| format!("kwpos id {0}", &rec[0]))?;
            out.insert(rec[1].to_string(), (id, rec[2].to_string()));
        }
    }
    Ok(out)
}

/// `load_conj_descriptions` — conj.csv → conj_id → description (+ errata hook).
pub fn load_conj_descriptions(path: &Path) -> Result<HashMap<i64, String>> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut out = HashMap::new();
    for rec in rdr.records() {
        let rec = rec?;
        if rec.len() >= 2 {
            let id: i64 = rec[0]
                .parse()
                .with_context(|| format!("conj id {0}", &rec[0]))?;
            out.insert(id, rec[1].to_string());
        }
    }
    // errata_conj_description_hook
    out.insert(CONJ_ADVERBIAL, "Adverbial".into());
    out.insert(CONJ_ADJECTIVE_STEM, "Adjective Stem".into());
    out.insert(52, "Negative Stem".into());
    out.insert(CONJ_CAUSATIVE_SU, "Causative (~su)".into());
    out.insert(CONJ_ADJECTIVE_LITERARY, "Old/literary form".into());
    Ok(out)
}

/// `load_conj_rules` — conjo.csv → pos_id → rules, plus errata hook rules.
pub fn load_conj_rules(
    path: &Path,
    pos_index: &HashMap<String, (i64, String)>,
) -> Result<HashMap<i64, Vec<ConjugationRule>>> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut rules: HashMap<i64, Vec<ConjugationRule>> = HashMap::new();
    for rec in rdr.records() {
        let rec = rec?;
        if rec.len() >= 9 {
            let rule = ConjugationRule {
                pos: rec[0].parse()?,
                conj: rec[1].parse()?,
                neg: rec[2].eq_ignore_ascii_case("t") || &rec[2] == "true",
                fml: rec[3].eq_ignore_ascii_case("t") || &rec[3] == "true",
                onum: rec[4].parse()?,
                stem: rec[5].parse()?,
                okuri: rec[6].to_string(),
                euphr: rec[7].to_string(),
                euphk: rec[8].to_string(),
                pos2: rec.get(9).map(|s| s.to_string()).filter(|s| !s.is_empty()),
            };
            rules.entry(rule.pos).or_default().push(rule);
        }
    }
    errata_conj_rules_hook(&mut rules, pos_index);
    Ok(rules)
}

/// `errata_conj_rules_hook` — adj-i/adj-ix extra rules (adverbial/stem/literary).
fn errata_conj_rules_hook(
    rules: &mut HashMap<i64, Vec<ConjugationRule>>,
    pos_index: &HashMap<String, (i64, String)>,
) {
    let adj_i_id = pos_index.get("adj-i").map(|(id, _)| *id).unwrap_or(1);
    let adj_ix_id = pos_index.get("adj-ix").map(|(id, _)| *id).unwrap_or(7);
    let mk = |pos: i64, conj: i64, okuri: &str, euphr: &str| ConjugationRule {
        pos,
        conj,
        neg: false,
        fml: false,
        onum: 1,
        stem: 1,
        okuri: okuri.to_string(),
        euphr: euphr.to_string(),
        euphk: String::new(),
        pos2: None,
    };
    rules.entry(adj_i_id).or_default().extend([
        mk(adj_i_id, CONJ_ADVERBIAL, "く", ""),
        mk(adj_i_id, CONJ_ADJECTIVE_STEM, "", ""),
        mk(adj_i_id, CONJ_ADJECTIVE_LITERARY, "き", ""),
    ]);
    rules.entry(adj_ix_id).or_default().extend([
        mk(adj_ix_id, CONJ_ADVERBIAL, "く", "よ"),
        mk(adj_ix_id, CONJ_ADJECTIVE_STEM, "", "よ"),
        mk(adj_ix_id, CONJ_ADJECTIVE_LITERARY, "き", "よ"),
    ]);
}

// ============================================================================
// construct_conjugation — rule application
// ============================================================================

fn is_kana(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| {
            ('\u{3040}'..='\u{309F}').contains(&c) || ('\u{30A0}'..='\u{30FF}').contains(&c)
        })
}

fn is_kana_char(c: char) -> bool {
    ('\u{3040}'..='\u{309F}').contains(&c) || ('\u{30A0}'..='\u{30FF}').contains(&c)
}

fn get_kana_suffix_length(word: &str) -> usize {
    word.chars().rev().take_while(|c| is_kana_char(*c)).count()
}

/// `construct_conjugation` — apply a rule to a word.
pub fn construct_conjugation(word: &str, rule: &ConjugationRule) -> String {
    let iskana = is_kana(word);
    let kana_suffix_len = get_kana_suffix_length(word);
    let use_kana_rules =
        iskana || (kana_suffix_len > 0 && kana_suffix_len > rule.stem as usize);

    let mut stem = rule.stem as usize;
    if use_kana_rules && !rule.euphr.is_empty() {
        stem += 1;
    } else if !use_kana_rules && !rule.euphk.is_empty() {
        stem += 1;
    }

    let chars: Vec<char> = word.chars().collect();
    let base: String = if stem > 0 && chars.len() > stem {
        chars[..chars.len() - stem].iter().collect()
    } else if stem > 0 {
        // word shorter than stem — Python's word[:-stem] would over-slice;
        // reproduce leniently (empty base)
        String::new()
    } else {
        word.to_string()
    };
    let euph = if use_kana_rules {
        &rule.euphr
    } else {
        &rule.euphk
    };
    format!("{}{}{}", base, euph, rule.okuri)
}

// ============================================================================
// Entry data prefetch
// ============================================================================

struct EntryData {
    posi: Vec<String>,
    /// (text, ord, kanji_flag) — conjugate_p readings, or all if none flagged
    readings: Vec<(String, i64, bool)>,
    all_readings: HashSet<String>,
}

/// `_prefetch_entry_data` — per-seq posi/readings/all_readings maps.
/// Scans the three tables once instead of chunked IN queries (same result).
fn prefetch_entry_data(conn: &Connection, seqs: &HashSet<i64>) -> Result<HashMap<i64, EntryData>> {
    let mut pos_by_seq: HashMap<i64, HashSet<String>> = HashMap::new();
    {
        let mut st = conn.prepare_cached("SELECT seq, text FROM sense_prop WHERE tag='pos'")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for r in rows.flatten() {
            if seqs.contains(&r.0) {
                pos_by_seq.entry(r.0).or_default().insert(r.1);
            }
        }
    }

    let mut kanji_rows: HashMap<i64, Vec<(String, i64, bool)>> = HashMap::new();
    {
        let mut st = conn.prepare_cached("SELECT seq, text, ord, conjugate_p FROM kanji_text")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?;
        for r in rows.flatten() {
            if seqs.contains(&r.0) {
                kanji_rows.entry(r.0).or_default().push((r.1, r.2, r.3));
            }
        }
    }
    let mut kana_rows: HashMap<i64, Vec<(String, i64, bool)>> = HashMap::new();
    {
        let mut st = conn.prepare_cached("SELECT seq, text, ord, conjugate_p FROM kana_text")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?;
        for r in rows.flatten() {
            if seqs.contains(&r.0) {
                kana_rows.entry(r.0).or_default().push((r.1, r.2, r.3));
            }
        }
    }

    let mut out = HashMap::new();
    for &seq in seqs {
        let posi: Vec<String> = pos_by_seq
            .get(&seq)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        let kanji = kanji_rows.get(&seq).cloned().unwrap_or_default();
        let kana = kana_rows.get(&seq).cloned().unwrap_or_default();

        let mut readings: Vec<(String, i64, bool)> = Vec::new();
        let mut all_readings: HashSet<String> = HashSet::new();
        for (text, ord, cp) in &kanji {
            all_readings.insert(text.clone());
            if *cp {
                readings.push((text.clone(), *ord, true));
            }
        }
        for (text, ord, cp) in &kana {
            all_readings.insert(text.clone());
            if *cp {
                readings.push((text.clone(), *ord, false));
            }
        }
        // Fallback: no conjugatable readings → all readings
        if readings.is_empty() {
            for (text, ord, _) in &kanji {
                readings.push((text.clone(), *ord, true));
            }
            for (text, ord, _) in &kana {
                readings.push((text.clone(), *ord, false));
            }
        }

        out.insert(
            seq,
            EntryData {
                posi,
                readings,
                all_readings,
            },
        );
    }
    Ok(out)
}

// ============================================================================
// Conjugation generation (pure)
// ============================================================================

/// One generated conjugation ready for insertion.
/// readings: (conj_text, kanji_flag, orig_text, ord, onum)
pub struct ConjDataOut {
    pub from_seq: i64,
    pub via: Option<i64>,
    pub pos: String,
    pub conj_type: i64,
    pub neg: Option<bool>,
    pub fml: Option<bool>,
    pub readings: Vec<(String, bool, String, i64, i64)>,
}

/// Shared matrix-generation for primary and secondary passes.
/// `posi` = POS names to apply; `lookup_seq` = seq whose readings conjugate.
fn gen_matrix(
    posi: &[String],
    seq_for_cop: i64,
    readings: &[(String, i64, bool)],
    conj_types: Option<&[i64]>,
    pos_index: &HashMap<String, (i64, String)>,
    pos_by_id: &HashMap<i64, String>,
    conj_rules: &HashMap<i64, Vec<ConjugationRule>>,
) -> HashMap<(i64, i64), [[Vec<(String, bool, String, i64, i64)>; 2]; 2]> {
    let mut conj_matrix: HashMap<(i64, i64), [[Vec<(String, bool, String, i64, i64)>; 2]; 2]> =
        HashMap::new();

    for pos in posi {
        if DO_NOT_CONJUGATE_POS.contains(&pos.as_str()) {
            continue;
        }
        if pos == "cop" && !COP_CONJUGATE_SEQ.contains(&seq_for_cop) {
            continue;
        }
        let pos_id = match pos_index.get(pos) {
            Some((id, _)) => *id,
            None => continue,
        };
        let rules = match conj_rules.get(&pos_id) {
            Some(r) if !r.is_empty() => r,
            _ => continue,
        };
        for (text, ord_num, kanji_flag) in readings {
            for rule in rules {
                let conj_id = rule.conj;
                if let Some(ct) = conj_types {
                    if !ct.contains(&conj_id) {
                        continue;
                    }
                }
                let conj_text = construct_conjugation(text, rule);
                let ni = rule.neg as usize;
                let fi = rule.fml as usize;
                conj_matrix
                    .entry((pos_id, conj_id))
                    .or_insert_with(|| [[Vec::new(), Vec::new()], [Vec::new(), Vec::new()]])[ni]
                    [fi]
                    .push((conj_text, *kanji_flag, text.clone(), *ord_num, rule.onum));
            }
        }
    }
    // drop unused
    let _ = pos_by_id;
    conj_matrix
}

/// `_generate_conjugations_for_entry` — primary pass.
fn generate_for_entry(
    seq: i64,
    data: &EntryData,
    pos_index: &HashMap<String, (i64, String)>,
    pos_by_id: &HashMap<i64, String>,
    conj_rules: &HashMap<i64, Vec<ConjugationRule>>,
) -> Vec<ConjDataOut> {
    if data.readings.is_empty() {
        return Vec::new();
    }
    let matrix = gen_matrix(
        &data.posi,
        seq,
        &data.readings,
        None,
        pos_index,
        pos_by_id,
        conj_rules,
    );
    matrix_to_out(&matrix, seq, None, &data.all_readings, pos_by_id)
}

/// `_generate_secondary_conjugations_for_entry` — secondary pass.
fn generate_secondary_for_entry(
    seq_from: i64,
    via_seq: i64,
    posi: &[String],
    conj_types: &[i64],
    data: &EntryData,
    pos_index: &HashMap<String, (i64, String)>,
    pos_by_id: &HashMap<i64, String>,
    conj_rules: &HashMap<i64, Vec<ConjugationRule>>,
) -> Vec<ConjDataOut> {
    if data.readings.is_empty() {
        return Vec::new();
    }
    let matrix = gen_matrix(
        posi,
        via_seq,
        &data.readings,
        Some(conj_types),
        pos_index,
        pos_by_id,
        conj_rules,
    );
    matrix_to_out(
        &matrix,
        seq_from,
        Some(via_seq),
        &data.all_readings,
        pos_by_id,
    )
}

fn matrix_to_out(
    matrix: &HashMap<(i64, i64), [[Vec<(String, bool, String, i64, i64)>; 2]; 2]>,
    from_seq: i64,
    via: Option<i64>,
    original_readings: &HashSet<String>,
    pos_by_id: &HashMap<i64, String>,
) -> Vec<ConjDataOut> {
    let mut results = Vec::new();
    // Deterministic order: Python's dict order varies; sort by key.
    let mut keys: Vec<(i64, i64)> = matrix.keys().copied().collect();
    keys.sort_unstable();
    for (pos_id, conj_id) in keys {
        let cells = &matrix[&(pos_id, conj_id)];
        let has_neg = !cells[1][0].is_empty() || !cells[1][1].is_empty();
        let has_fml = !cells[0][1].is_empty() || !cells[1][1].is_empty();
        let pos = match pos_by_id.get(&pos_id) {
            Some(p) => p.clone(),
            None => continue,
        };
        for ii in 0..4 {
            let neg = ii >= 2;
            let fml = ii % 2 == 1;
            let readings_list: Vec<(String, bool, String, i64, i64)> = cells[neg as usize]
                [fml as usize]
                .iter()
                .filter(|r| !original_readings.contains(&r.0))
                .cloned()
                .collect();
            if readings_list.is_empty() {
                continue;
            }
            results.push(ConjDataOut {
                from_seq,
                via,
                pos: pos.clone(),
                conj_type: conj_id,
                neg: if has_neg { Some(neg) } else { None },
                fml: if has_fml { Some(fml) } else { None },
                readings: readings_list,
            });
        }
    }
    results
}

// ============================================================================
// Reading→seq index (existing-entry reuse)
// ============================================================================

type ReadingKey = (Vec<String>, Vec<String>);

fn reading_key(kanji: &[String], kana: &[String]) -> ReadingKey {
    let mut k = kanji.to_vec();
    k.sort();
    k.dedup();
    let mut a = kana.to_vec();
    a.sort();
    a.dedup();
    (k, a)
}

/// `_build_reading_to_seq_index` — (kanji_set, kana_set) → min seq.
fn build_reading_to_seq_index(conn: &Connection) -> Result<HashMap<ReadingKey, i64>> {
    let mut kanji_by_seq: HashMap<i64, HashSet<String>> = HashMap::new();
    let mut kana_by_seq: HashMap<i64, HashSet<String>> = HashMap::new();
    {
        let mut st = conn.prepare_cached("SELECT seq, text FROM kanji_text")?;
        for r in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (seq, text) = r?;
            kanji_by_seq.entry(seq).or_default().insert(text);
        }
    }
    {
        let mut st = conn.prepare_cached("SELECT seq, text FROM kana_text")?;
        for r in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (seq, text) = r?;
            kana_by_seq.entry(seq).or_default().insert(text);
        }
    }
    let all_seqs: HashSet<i64> = kanji_by_seq
        .keys()
        .chain(kana_by_seq.keys())
        .copied()
        .collect();
    let mut index = HashMap::new();
    for seq in all_seqs {
        let mut k: Vec<String> = kanji_by_seq
            .get(&seq)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        k.sort();
        k.dedup();
        let mut a: Vec<String> = kana_by_seq
            .get(&seq)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        a.sort();
        a.dedup();
        let key = (k, a);
        match index.get(&key) {
            Some(&existing) if existing <= seq => {}
            _ => {
                index.insert(key, seq);
            }
        }
    }
    Ok(index)
}

// ============================================================================
// Bulk insert — `_bulk_insert_conjugations`
// ============================================================================

fn bulk_insert_conjugations(
    conn: &Connection,
    all_conj_data: &[ConjDataOut],
    start_seq: i64,
    reading_index: &mut HashMap<ReadingKey, i64>,
) -> Result<(usize, usize)> {
    let next_conj_id: i64 =
        conn.query_row("SELECT COALESCE(MAX(id),0) FROM conjugation", [], |r| {
            r.get::<_, i64>(0)
        })? + 1;
    let next_prop_id: i64 =
        conn.query_row("SELECT COALESCE(MAX(id),0) FROM conj_prop", [], |r| {
            r.get::<_, i64>(0)
        })? + 1;
    let next_sr_id: i64 = conn.query_row(
        "SELECT COALESCE(MAX(id),0) FROM conj_source_reading",
        [],
        |r| r.get::<_, i64>(0),
    )? + 1;

    let mut next_seq = start_seq;
    let mut conj_id_counter = next_conj_id;
    let mut prop_id = next_prop_id;
    let mut sr_id = next_sr_id;

    let mut seen_conjs: HashMap<(i64, i64, Option<i64>), i64> = HashMap::new();
    let mut seen_props: HashSet<(i64, i64, String, Option<bool>, Option<bool>)> = HashSet::new();
    let mut seen_srs: HashSet<(i64, String, String)> = HashSet::new();

    let mut new_entries = 0usize;
    let mut reused_entries = 0usize;

    // Prepared inserts
    let mut ins_entry = conn.prepare_cached(
        "INSERT INTO entry (seq,content,root_p,n_kanji,n_kana,primary_nokanji) \
         VALUES (?1,'',0,?2,?3,?4)",
    )?;
    let mut ins_kanji = conn.prepare_cached(
        "INSERT INTO kanji_text (seq,text,ord,common,common_tags,conjugate_p,nokanji,best_kana) \
         VALUES (?1,?2,?3,NULL,'',?4,0,NULL)",
    )?;
    let mut ins_kana = conn.prepare_cached(
        "INSERT INTO kana_text (seq,text,ord,common,common_tags,conjugate_p,nokanji,best_kanji) \
         VALUES (?1,?2,?3,NULL,'',?4,0,NULL)",
    )?;
    let mut ins_conj =
        conn.prepare_cached("INSERT INTO conjugation (id,seq,\"from\",via) VALUES (?1,?2,?3,?4)")?;
    let mut ins_prop = conn.prepare_cached(
        "INSERT INTO conj_prop (id,conj_id,conj_type,pos,neg,fml) VALUES (?1,?2,?3,?4,?5,?6)",
    )?;
    let mut ins_sr = conn.prepare_cached(
        "INSERT INTO conj_source_reading (id,conj_id,text,source_text) VALUES (?1,?2,?3,?4)",
    )?;

    for cd in all_conj_data {
        // Sort by (ord, onum)
        let mut sorted: Vec<&(String, bool, String, i64, i64)> = cd.readings.iter().collect();
        sorted.sort_by_key(|r| (r.3, r.4));

        let mut source_readings = Vec::new();
        let mut kanji_readings = Vec::new();
        let mut kana_readings = Vec::new();
        for (conj_text, kanji_flag, orig_text, _ord, _onum) in &sorted {
            source_readings.push((conj_text.clone(), orig_text.clone()));
            if *kanji_flag {
                kanji_readings.push(conj_text.clone());
            } else {
                kana_readings.push(conj_text.clone());
            }
        }
        if kanji_readings.is_empty() && kana_readings.is_empty() {
            continue;
        }
        kanji_readings.dedup();
        kana_readings.dedup();
        // dict.fromkeys dedup preserving order — dedup() on non-sorted only
        // removes consecutive dups; do order-preserving dedup properly:
        fn ord_dedup(v: Vec<String>) -> Vec<String> {
            let mut seen = HashSet::new();
            v.into_iter().filter(|x| seen.insert(x.clone())).collect()
        }
        let kanji_readings = ord_dedup(kanji_readings);
        let kana_readings = ord_dedup(kana_readings);

        let mut exclude = HashSet::new();
        exclude.insert(cd.from_seq);
        if let Some(v) = cd.via {
            exclude.insert(v);
        }
        let key = reading_key(&kanji_readings, &kana_readings);
        let existing_seq = reading_index.get(&key).filter(|s| !exclude.contains(s));

        let (seq, _created_new) = match existing_seq {
            Some(&s) => {
                reused_entries += 1;
                (s, false)
            }
            None => {
                let s = next_seq;
                next_seq += 1;
                new_entries += 1;
                let conjugate_p = SECONDARY_CONJUGATION_TYPES_FROM.contains(&cd.conj_type);
                ins_entry.execute(params![
                    s,
                    kanji_readings.len() as i64,
                    kana_readings.len() as i64,
                    kanji_readings.is_empty(),
                ])?;
                for (ord_num, text) in kanji_readings.iter().enumerate() {
                    ins_kanji.execute(params![s, text, ord_num as i64, conjugate_p])?;
                }
                for (ord_num, text) in kana_readings.iter().enumerate() {
                    ins_kana.execute(params![s, text, ord_num as i64, conjugate_p])?;
                }
                reading_index.insert(key, s);
                (s, true)
            }
        };

        let conj_key = (seq, cd.from_seq, cd.via);
        let conj_id = match seen_conjs.get(&conj_key) {
            Some(&id) => id,
            None => {
                let id = conj_id_counter;
                conj_id_counter += 1;
                seen_conjs.insert(conj_key, id);
                ins_conj.execute(params![id, seq, cd.from_seq, cd.via])?;
                id
            }
        };

        let prop_key = (conj_id, cd.conj_type, cd.pos.clone(), cd.neg, cd.fml);
        if !seen_props.contains(&prop_key) {
            seen_props.insert(prop_key);
            ins_prop.execute(params![
                prop_id,
                conj_id,
                cd.conj_type,
                cd.pos,
                cd.neg,
                cd.fml
            ])?;
            prop_id += 1;
        }

        for (text, source_text) in &source_readings {
            let sr_key = (conj_id, text.clone(), source_text.clone());
            if !seen_srs.contains(&sr_key) {
                seen_srs.insert(sr_key);
                ins_sr.execute(params![sr_id, conj_id, text, source_text])?;
                sr_id += 1;
            }
        }
    }

    Ok((new_entries, reused_entries))
}

// ============================================================================
// Public drivers
// ============================================================================

fn load_all_csvs(
    data_dir: &Path,
) -> Result<(
    HashMap<String, (i64, String)>,
    HashMap<i64, String>,
    HashMap<i64, Vec<ConjugationRule>>,
)> {
    let pos_index = load_pos_index(&data_dir.join("kwpos.csv"))?;
    let conj_desc = load_conj_descriptions(&data_dir.join("conj.csv"))?;
    let conj_rules = load_conj_rules(&data_dir.join("conjo.csv"), &pos_index)?;
    let _ = conj_desc;
    Ok((pos_index, conj_desc, conj_rules))
}

/// `load_conjugations` — primary pass over all conjugatable-POS entries.
/// Returns (conj_records, new_entries, reused_entries).
pub fn load_conjugations(
    conn: &mut Connection,
    data_dir: &Path,
    mut progress: impl FnMut(usize, usize),
) -> Result<(usize, usize, usize)> {
    let (pos_index, _descs, conj_rules) = load_all_csvs(data_dir)?;
    let pos_by_id: HashMap<i64, String> = pos_index
        .iter()
        .map(|(name, (id, _))| (*id, name.clone()))
        .collect();

    let mut reading_index = build_reading_to_seq_index(conn)?;

    // seqs with conjugatable POS, minus exclusions — sorted for determinism
    let pos_set: HashSet<&str> = POS_WITH_CONJ_RULES.iter().copied().collect();
    let skip_set: HashSet<i64> = DO_NOT_CONJUGATE_SEQ.iter().copied().collect();
    let mut seqs: Vec<i64> = {
        let mut st = conn.prepare_cached("SELECT DISTINCT seq FROM sense_prop WHERE tag='pos'")?;
        let rows = st.query_map([], |r| r.get::<_, i64>(0))?;
        let mut v = Vec::new();
        for r in rows.flatten() {
            if !skip_set.contains(&r) {
                v.push(r);
            }
        }
        v
    };
    // Filter to seqs having at least one conjugatable pos
    {
        let seq_set: HashSet<i64> = seqs.iter().copied().collect();
        let mut keep: HashSet<i64> = HashSet::new();
        let mut st = conn.prepare_cached("SELECT seq, text FROM sense_prop WHERE tag='pos'")?;
        for r in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (seq, text) = r?;
            if seq_set.contains(&seq) && pos_set.contains(text.as_str()) {
                keep.insert(seq);
            }
        }
        seqs.retain(|s| keep.contains(s));
    }
    seqs.sort_unstable();

    let total = seqs.len();
    let seq_set: HashSet<i64> = seqs.iter().copied().collect();
    let entry_data = prefetch_entry_data(conn, &seq_set)?;

    let mut all_conj_data = Vec::new();
    for (i, &seq) in seqs.iter().enumerate() {
        if let Some(data) = entry_data.get(&seq) {
            all_conj_data.extend(generate_for_entry(
                seq,
                data,
                &pos_index,
                &pos_by_id,
                &conj_rules,
            ));
        }
        if (i + 1) % 5000 == 0 {
            progress(i + 1, total);
        }
    }

    let start_seq: i64 =
        conn.query_row("SELECT COALESCE(MAX(seq),0)+1 FROM entry", [], |r| r.get(0))?;

    let n_records = all_conj_data.len();
    let tx = conn.transaction()?;
    let (new_entries, reused) =
        bulk_insert_conjugations(&tx, &all_conj_data, start_seq, &mut reading_index)?;
    tx.commit()?;
    progress(total, total);
    Ok((n_records, new_entries, reused))
}

/// `load_secondary_conjugations` — passive-of-causative etc.
pub fn load_secondary_conjugations(
    conn: &mut Connection,
    data_dir: &Path,
    mut progress: impl FnMut(usize, usize),
) -> Result<(usize, usize, usize)> {
    let (pos_index, _descs, conj_rules) = load_all_csvs(data_dir)?;
    let pos_by_id: HashMap<i64, String> = pos_index
        .iter()
        .map(|(name, (id, _))| (*id, name.clone()))
        .collect();

    // Rebuild index including generated conj entries
    let mut reading_index = build_reading_to_seq_index(conn)?;

    // to_conj: DISTINCT (conj.from, conj.seq, prop.conj_type) for
    // secondariable types, non vs-i/vs-s, via IS NULL, neg/fml unset-or-false
    let mut to_conj: Vec<(i64, i64, i64)> = Vec::new();
    {
        let mut st = conn.prepare_cached(
            "SELECT DISTINCT c.\"from\", c.seq, cp.conj_type \
             FROM conjugation c JOIN conj_prop cp ON cp.conj_id = c.id \
             WHERE cp.conj_type IN (5,6,7,8,14) \
               AND cp.pos NOT IN ('vs-i','vs-s') \
               AND c.via IS NULL \
               AND (cp.neg IS NULL OR cp.neg = 0) \
               AND (cp.fml IS NULL OR cp.fml = 0)",
        )?;
        for r in st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })? {
            to_conj.push(r?);
        }
    }
    to_conj.sort_unstable();
    let total = to_conj.len();

    let via_seqs: HashSet<i64> = to_conj.iter().map(|(_, s, _)| *s).collect();
    let entry_data = prefetch_entry_data(conn, &via_seqs)?;

    let mut all_conj_data = Vec::new();
    for (i, (seq_from, via_seq, conj_type)) in to_conj.iter().enumerate() {
        let pos: &[&str] = if *conj_type == CONJ_CAUSATIVE_SU {
            &["v5s"]
        } else {
            &["v1"]
        };
        let posi: Vec<String> = pos.iter().map(|s| s.to_string()).collect();
        if let Some(data) = entry_data.get(via_seq) {
            all_conj_data.extend(generate_secondary_for_entry(
                *seq_from,
                *via_seq,
                &posi,
                SECONDARY_CONJUGATION_TYPES,
                data,
                &pos_index,
                &pos_by_id,
                &conj_rules,
            ));
        }
        if (i + 1) % 5000 == 0 {
            progress(i + 1, total);
        }
    }

    let start_seq: i64 =
        conn.query_row("SELECT COALESCE(MAX(seq),0)+1 FROM entry", [], |r| r.get(0))?;
    let n_records = all_conj_data.len();
    let tx = conn.transaction()?;
    let (new_entries, reused) =
        bulk_insert_conjugations(&tx, &all_conj_data, start_seq, &mut reading_index)?;
    tx.commit()?;
    progress(total, total);
    Ok((n_records, new_entries, reused))
}
