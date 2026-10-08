//! Rust port of `himotoki/loading/errata.py` — post-load data corrections.
//! Runs after JMdict + conjugations, same order as `add_errata`.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::errata_data as D;

// ============================================================================
// Helpers (errata.py lines 26–161)
// ============================================================================

/// `add_sense_prop` — find sense (seq, ord), append (tag, text) if absent.
fn add_sense_prop(
    conn: &Connection,
    seq: i64,
    sense_ord: i64,
    tag: &str,
    text: &str,
) -> Result<()> {
    let sense_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM sense WHERE seq=?1 AND ord=?2",
            params![seq, sense_ord],
            |r| r.get(0),
        )
        .optional()?;
    let Some(sense_id) = sense_id else {
        eprintln!("warn: sense not found seq={seq} ord={sense_ord}");
        return Ok(());
    };
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM sense_prop WHERE sense_id=?1 AND tag=?2 AND text=?3",
            params![sense_id, tag, text],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_some() {
        return Ok(());
    }
    // Python quirk: `scalar() or -1` maps a real max of 0 to -1 too.
    let raw: Option<i64> = conn.query_row(
        "SELECT MAX(ord) FROM sense_prop WHERE sense_id=?1 AND tag=?2",
        params![sense_id, tag],
        |r| r.get::<_, Option<i64>>(0),
    )?;
    let new_ord = match raw {
        Some(m) if m > 0 => m + 1,
        _ => 0,
    };
    conn.execute(
        "INSERT INTO sense_prop (sense_id,seq,tag,text,ord) VALUES (?1,?2,?3,?4,?5)",
        params![sense_id, seq, tag, text, new_ord],
    )?;
    Ok(())
}

/// `delete_sense_prop`
fn delete_sense_prop(conn: &Connection, seq: i64, tag: &str, text: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM sense_prop WHERE seq=?1 AND tag=?2 AND text=?3",
        params![seq, tag, text],
    )?;
    Ok(())
}

/// `set_common` — UPDATE common on kana_text or kanji_text.
fn set_common(
    conn: &Connection,
    table: &str,
    seq: i64,
    text: &str,
    common: Option<i64>,
) -> Result<()> {
    let sql = match table {
        "kana_text" => "UPDATE kana_text SET common=?3 WHERE seq=?1 AND text=?2",
        "kanji_text" => "UPDATE kanji_text SET common=?3 WHERE seq=?1 AND text=?2",
        other => anyhow::bail!("Unknown table: {other}"),
    };
    conn.execute(sql, params![seq, text, common])?;
    Ok(())
}

/// `add_reading` — insert a kana_text row (conjugate_p=1, nokanji=0).
fn add_reading(conn: &Connection, seq: i64, text: &str, common: Option<i64>) -> Result<()> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM kana_text WHERE seq=?1 AND text=?2 LIMIT 1",
            params![seq, text],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_some() {
        return Ok(());
    }
    let raw: Option<i64> = conn.query_row(
        "SELECT MAX(ord) FROM kana_text WHERE seq=?1",
        params![seq],
        |r| r.get::<_, Option<i64>>(0),
    )?;
    let new_ord = match raw {
        Some(m) if m > 0 => m + 1,
        _ => 0,
    };
    conn.execute(
        "INSERT INTO kana_text (seq,text,ord,common,common_tags,conjugate_p,nokanji,best_kanji) \
         VALUES (?1,?2,?3,?4,'',1,0,NULL)",
        params![seq, text, new_ord, common],
    )?;
    Ok(())
}

/// `delete_reading` — removes from BOTH kana_text and kanji_text.
fn delete_reading(conn: &Connection, seq: i64, text: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM kana_text WHERE seq=?1 AND text=?2",
        params![seq, text],
    )?;
    conn.execute(
        "DELETE FROM kanji_text WHERE seq=?1 AND text=?2",
        params![seq, text],
    )?;
    Ok(())
}

/// `set_primary_nokanji`
fn set_primary_nokanji(conn: &Connection, seq: i64, value: bool) -> Result<()> {
    conn.execute(
        "UPDATE entry SET primary_nokanji=?2 WHERE seq=?1",
        params![seq, value],
    )?;
    Ok(())
}

/// `delete_conjugation` — drop conj row + its props and source readings.
fn delete_conjugation(conn: &Connection, seq: i64, from_seq: i64) -> Result<()> {
    let ids: Vec<i64> = {
        let mut st =
            conn.prepare_cached("SELECT id FROM conjugation WHERE seq=?1 AND \"from\"=?2")?;
        let rows = st.query_map(params![seq, from_seq], |r| r.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if ids.is_empty() {
        return Ok(());
    }
    for id in &ids {
        conn.execute("DELETE FROM conj_prop WHERE conj_id=?1", params![id])?;
        conn.execute(
            "DELETE FROM conj_source_reading WHERE conj_id=?1",
            params![id],
        )?;
        conn.execute("DELETE FROM conjugation WHERE id=?1", params![id])?;
    }
    Ok(())
}

fn next_seq(conn: &Connection) -> Result<i64> {
    Ok(
        conn.query_row("SELECT COALESCE(MAX(seq),0)+1 FROM entry", [], |r| {
            r.get::<_, i64>(0)
        })?,
    )
}

// ============================================================================
// Conjugation errata
// ============================================================================

/// `add_gozaimasu_conjs` — manual conj forms for ございます/ございません.
fn add_gozaimasu_conjs(conn: &Connection) -> Result<()> {
    let base_seqs = [1612690i64, 2253080];
    // (conj_type, pos, fml, suffix)
    let patterns: [(i64, &str, bool, &str); 6] = [
        (1, "exp", true, "せん"),
        (2, "exp", false, "した"),
        (3, "exp", false, "して"),
        (9, "exp", false, "しょう"),
        (11, "exp", false, "したら"),
        (12, "exp", false, "したり"),
    ];
    let mut seq_next = next_seq(conn)?;
    for base_seq in base_seqs {
        let readings: Vec<String> = {
            let mut st = conn.prepare_cached("SELECT text FROM kana_text WHERE seq=?1")?;
            let v = st
                .query_map(params![base_seq], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        if readings.is_empty() {
            continue;
        }
        for &(conj_type, pos, fml, suffix) in &patterns {
            for reading in &readings {
                if !reading.ends_with('す') {
                    continue;
                }
                let mut chars: Vec<char> = reading.chars().collect();
                chars.pop();
                let conj_text: String = chars.iter().collect::<String>() + suffix;

                let existing: Option<i64> = conn
                    .query_row(
                        "SELECT seq FROM kana_text WHERE text=?1 LIMIT 1",
                        params![conj_text],
                        |r| r.get(0),
                    )
                    .optional()?;
                let conj_seq = match existing {
                    Some(s) => s,
                    None => {
                        let s = seq_next;
                        seq_next += 1;
                        conn.execute(
                            "INSERT INTO entry (seq,content,root_p,n_kanji,n_kana,primary_nokanji) \
                             VALUES (?1,'',0,0,0,0)",
                            params![s],
                        )?;
                        conn.execute(
                            "INSERT INTO kana_text (seq,text,ord,common,common_tags,conjugate_p,nokanji,best_kanji) \
                             VALUES (?1,?2,0,0,'',1,0,NULL)",
                            params![s, conj_text],
                        )?;
                        s
                    }
                };
                let existing_conj: Option<i64> = conn
                    .query_row(
                        "SELECT id FROM conjugation WHERE seq=?1 AND \"from\"=?2 LIMIT 1",
                        params![conj_seq, base_seq],
                        |r| r.get(0),
                    )
                    .optional()?;
                if existing_conj.is_some() {
                    continue;
                }
                conn.execute(
                    "INSERT INTO conjugation (seq,\"from\",via) VALUES (?1,?2,NULL)",
                    params![conj_seq, base_seq],
                )?;
                let conj_id = conn.last_insert_rowid();
                conn.execute(
                    "INSERT INTO conj_prop (conj_id,conj_type,pos,neg,fml) VALUES (?1,?2,?3,?4,?5)",
                    params![conj_id, conj_type, pos, conj_type == 1, fml],
                )?;
                conn.execute(
                    "INSERT INTO conj_source_reading (conj_id,text,source_text) VALUES (?1,?2,?3)",
                    params![conj_id, conj_text, reading],
                )?;
            }
        }
    }
    Ok(())
}

/// `add_custom_suru_verbs` — おかけ/お掛け entry missing from JMdict.
fn add_custom_suru_verbs(conn: &Connection) -> Result<()> {
    // (kana, kanji, glosses)
    let custom: [(&str, &str, &[&str]); 1] = [(
        "おかけ",
        "お掛け",
        &["to cause", "to sit", "to spend (time)"],
    )];
    let mut seq_next = next_seq(conn)?;
    for (kana_text, kanji_text, glosses) in custom {
        let existing: Option<i64> = conn
            .query_row(
                "SELECT seq FROM kana_text WHERE text=?1 LIMIT 1",
                params![kana_text],
                |r| r.get(0),
            )
            .optional()?;
        if existing.is_some() {
            continue;
        }
        let seq = seq_next;
        seq_next += 1;
        conn.execute(
            "INSERT INTO entry (seq,content,root_p,n_kanji,n_kana,primary_nokanji) \
             VALUES (?1,'',1,1,1,1)",
            params![seq],
        )?;
        conn.execute(
            "INSERT INTO kanji_text (seq,text,ord,common,common_tags,conjugate_p,nokanji,best_kana) \
             VALUES (?1,?2,0,0,'',1,0,NULL)",
            params![seq, kanji_text],
        )?;
        conn.execute(
            "INSERT INTO kana_text (seq,text,ord,common,common_tags,conjugate_p,nokanji,best_kanji) \
             VALUES (?1,?2,0,0,'',1,1,NULL)",
            params![seq, kana_text],
        )?;
        conn.execute("INSERT INTO sense (seq,ord) VALUES (?1,0)", params![seq])?;
        let sense_id = conn.last_insert_rowid();
        for (tag, text, ord) in [("pos", "vs", 0i64), ("misc", "uk", 0), ("misc", "hum", 1)] {
            conn.execute(
                "INSERT INTO sense_prop (sense_id,seq,tag,text,ord) VALUES (?1,?2,?3,?4,?5)",
                params![sense_id, seq, tag, text, ord],
            )?;
        }
        for (i, g) in glosses.iter().enumerate() {
            conn.execute(
                "INSERT INTO gloss (sense_id,text,ord) VALUES (?1,?2,?3)",
                params![sense_id, g, i as i64],
            )?;
        }
    }
    Ok(())
}

/// `add_synthetic_suffix_entries` — seq 900000 たそう.
fn add_synthetic_suffix_entries(conn: &Connection) -> Result<()> {
    let synthetic: [(i64, &str, &str); 1] = [(
        900000,
        "たそう",
        "looking like wanting to... (tai+sou suffix)",
    )];
    for (seq, text, description) in synthetic {
        let exists: Option<i64> = conn
            .query_row("SELECT seq FROM entry WHERE seq=?1", params![seq], |r| {
                r.get(0)
            })
            .optional()?;
        if exists.is_some() {
            continue;
        }
        conn.execute(
            "INSERT INTO entry (seq,content,root_p,n_kanji,n_kana,primary_nokanji) \
             VALUES (?1,'',1,0,1,1)",
            params![seq],
        )?;
        conn.execute(
            "INSERT INTO kana_text (seq,text,ord,common,common_tags,conjugate_p,nokanji,best_kanji) \
             VALUES (?1,?2,0,0,'',0,1,NULL)",
            params![seq, text],
        )?;
        conn.execute("INSERT INTO sense (seq,ord) VALUES (?1,0)", params![seq])?;
        let sense_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO sense_prop (sense_id,seq,tag,text,ord) VALUES (?1,?2,'pos','suf',0)",
            params![sense_id, seq],
        )?;
        conn.execute(
            "INSERT INTO gloss (sense_id,text,ord) VALUES (?1,?2,0)",
            params![sense_id, description],
        )?;
    }
    Ok(())
}

/// `add_deha_ja_readings` — じゃ variants for では-forms of だ (2089020).
fn add_deha_ja_readings(conn: &Connection) -> Result<()> {
    const DA_SEQ: i64 = 2089020;
    let deha_list: Vec<(i64, String)> = {
        let mut st = conn.prepare_cached(
            "SELECT DISTINCT c.seq, k.text FROM conjugation c \
             JOIN kana_text k ON k.seq = c.seq \
             WHERE c.\"from\"=?1 AND k.text LIKE 'では%'",
        )?;
        let v = st
            .query_map(params![DA_SEQ], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        v
    };
    for (seq, deha_text) in &deha_list {
        // 'じゃ' + text minus its first 2 chars
        let ja_text = format!("じゃ{}", deha_text.chars().skip(2).collect::<String>());
        add_reading(conn, *seq, &ja_text, None)?;
    }

    let src: Vec<(i64, String, String)> = {
        let mut st = conn.prepare_cached(
            "SELECT sr.conj_id, sr.text, sr.source_text FROM conj_source_reading sr \
             JOIN conjugation c ON c.id = sr.conj_id \
             WHERE c.\"from\"=?1 AND sr.text LIKE 'では%'",
        )?;
        let v = st
            .query_map(params![DA_SEQ], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        v
    };
    for (conj_id, text, source_text) in &src {
        let ja_text = format!("じゃ{}", text.chars().skip(2).collect::<String>());
        let exists: Option<i64> = conn
            .query_row(
                "SELECT id FROM conj_source_reading WHERE conj_id=?1 AND text=?2 LIMIT 1",
                params![conj_id, ja_text],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_some() {
            continue;
        }
        let ja_source = if source_text.starts_with("では") {
            format!("じゃ{}", source_text.chars().skip(2).collect::<String>())
        } else {
            source_text.clone()
        };
        conn.execute(
            "INSERT INTO conj_source_reading (conj_id,text,source_text) VALUES (?1,?2,?3)",
            params![conj_id, ja_text, ja_source],
        )?;
    }
    Ok(())
}

// ============================================================================
// Apply passes (table-driven)
// ============================================================================

fn apply_uk_adjustments(conn: &Connection) -> Result<()> {
    for &seq in D::DELETE_UK_ENTRIES {
        delete_sense_prop(conn, seq, "misc", "uk")?;
    }
    for &seq in D::ADDITIONAL_DELETE_UK_ENTRIES {
        delete_sense_prop(conn, seq, "misc", "uk")?;
    }
    for &(seq, ord) in D::ADD_UK_ENTRIES {
        add_sense_prop(conn, seq, ord, "misc", "uk")?;
    }
    for &(seq, ord) in D::ADDITIONAL_ADD_UK_ENTRIES {
        add_sense_prop(conn, seq, ord, "misc", "uk")?;
    }
    Ok(())
}

fn apply_common_adjustments(conn: &Connection) -> Result<()> {
    for &(tbl, seq, text, common) in D::COMMON_ADJUSTMENTS {
        set_common(conn, tbl, seq, text, common)?;
    }
    for &(tbl, seq, text, common) in D::ADDITIONAL_COMMON_ADJUSTMENTS {
        set_common(conn, tbl, seq, text, common)?;
    }
    Ok(())
}

fn apply_reading_adjustments(conn: &Connection) -> Result<()> {
    for &(seq, text, common) in D::ADD_READINGS {
        add_reading(conn, seq, text, common)?;
    }
    for &(seq, text, common) in D::ADDITIONAL_ADD_READINGS {
        add_reading(conn, seq, text, common)?;
    }
    for &(seq, text) in D::DELETE_READINGS {
        delete_reading(conn, seq, text)?;
    }
    for &(seq, text) in D::ADDITIONAL_DELETE_READINGS {
        delete_reading(conn, seq, text)?;
    }
    Ok(())
}

fn apply_pos_adjustments(conn: &Connection) -> Result<()> {
    add_sense_prop(conn, 2425930, 0, "pos", "prt")?;
    add_sense_prop(conn, 2457930, 0, "pos", "prt")?;
    delete_sense_prop(conn, 2629920, "pos", "adv-to")?;
    for &(seq, tag, text) in D::ADDITIONAL_POS_DELETIONS {
        delete_sense_prop(conn, seq, tag, text)?;
    }
    for &(seq, ord, pos) in D::ADDITIONAL_POS_ADDITIONS {
        add_sense_prop(conn, seq, ord, "pos", pos)?;
    }
    Ok(())
}

fn apply_counter_pos_adjustments(conn: &Connection) -> Result<()> {
    for &(seq, ord, pos) in D::COUNTER_POS_ENTRIES {
        add_sense_prop(conn, seq, ord, "pos", pos)?;
    }
    for &(seq, pos) in D::DELETE_COUNTER_POS_ENTRIES {
        delete_sense_prop(conn, seq, "pos", pos)?;
    }
    Ok(())
}

fn apply_primary_nokanji_adjustments(conn: &Connection) -> Result<()> {
    for &seq in D::PRIMARY_NOKANJI_CLEAR {
        set_primary_nokanji(conn, seq, false)?;
    }
    Ok(())
}

fn apply_conjugation_deletions(conn: &Connection) -> Result<()> {
    for &(seq, from_seq) in D::CONJUGATION_DELETIONS {
        delete_conjugation(conn, seq, from_seq)?;
    }
    Ok(())
}

fn apply_misc_adjustments(conn: &Connection) -> Result<()> {
    for &(seq, tag, text) in D::DELETE_ARCH_ENTRIES {
        delete_sense_prop(conn, seq, tag, text)?;
    }
    for &(seq, ord, text) in D::ADD_OBSC_ENTRIES {
        add_sense_prop(conn, seq, ord, "misc", text)?;
    }
    for &(seq, tag, text) in D::DELETE_RARE_ENTRIES {
        delete_sense_prop(conn, seq, tag, text)?;
    }
    Ok(())
}

// ============================================================================
// `add_errata` — driver (same order as Python)
// ============================================================================

pub fn add_errata(conn: &Connection) -> Result<()> {
    add_custom_suru_verbs(conn)?;
    add_synthetic_suffix_entries(conn)?;
    add_gozaimasu_conjs(conn)?;
    add_deha_ja_readings(conn)?;
    apply_uk_adjustments(conn)?;
    apply_common_adjustments(conn)?;
    apply_reading_adjustments(conn)?;
    apply_pos_adjustments(conn)?;
    apply_counter_pos_adjustments(conn)?;
    apply_primary_nokanji_adjustments(conn)?;
    apply_conjugation_deletions(conn)?;
    apply_misc_adjustments(conn)?;
    Ok(())
}
