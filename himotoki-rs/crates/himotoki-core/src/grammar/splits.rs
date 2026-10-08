//! Split definitions — port of `himotoki/grammar/splits.py` (dict-split.lisp).
//!
//! `_split_map`/`_segsplit_map` become LazyLock<HashMap> of boxed fns.
//! `is_kana` selects kanji_text vs kana_text for `find_word_seq`/`conj_of`.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use rusqlite::Connection;

use crate::chars::is_kana;
use crate::db::rows::{KanaTextRow, KanjiTextRow};
use crate::types::{Reading, Segment, Word, WordMatch};

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone)]
pub struct SplitPart {
    pub word: Word,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SplitResult {
    pub parts: Vec<SplitPart>,
    pub score_bonus: f64,
    pub modifiers: HashSet<String>,
}

impl SplitResult {
    pub fn has_modifier(&self, m: &str) -> bool {
        self.modifiers.contains(m)
    }
}

type SplitFn = Box<dyn Fn(&Connection, &Word) -> Option<SplitResult> + Send + Sync>;

static SPLIT_MAP: LazyLock<HashMap<i64, SplitFn>> = LazyLock::new(build_split_map);
static SEGSPLIT_MAP: LazyLock<HashMap<i64, SplitFn>> = LazyLock::new(build_segsplit_map);

// ============================================================================
// Lookup helpers (find_word_seq / find_word_conj_of)
// ============================================================================

fn find_word_seq(conn: &Connection, text: &str, seqs: &[i64]) -> Vec<WordMatch> {
    if seqs.is_empty() {
        return Vec::new();
    }
    let ph = seqs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let params: Vec<rusqlite::types::Value> = std::iter::once(text.to_string().into())
        .chain(seqs.iter().map(|s| (*s).into()))
        .collect();
    if is_kana(text) {
        let sql = format!(
            "SELECT id, seq, text, ord, common, best_kanji FROM kana_text WHERE text = ?1 AND seq IN ({})",
            ph
        );
        let mut stmt = match conn.prepare_cached(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(rusqlite::params_from_iter(params), |r| {
            Ok(KanaTextRow {
                id: r.get(0)?,
                seq: r.get(1)?,
                text: r.get(2)?,
                ord: r.get(3)?,
                common: r.get(4)?,
                best_kanji: r.get(5)?,
                ..Default::default()
            })
        })
        .map(|rows| {
            rows.flatten()
                .map(|r| WordMatch::new(Reading::Kana(r)))
                .collect()
        })
        .unwrap_or_default()
    } else {
        let sql = format!(
            "SELECT id, seq, text, ord, common, best_kana FROM kanji_text WHERE text = ?1 AND seq IN ({})",
            ph
        );
        let mut stmt = match conn.prepare_cached(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(rusqlite::params_from_iter(params), |r| {
            Ok(KanjiTextRow {
                id: r.get(0)?,
                seq: r.get(1)?,
                text: r.get(2)?,
                ord: r.get(3)?,
                common: r.get(4)?,
                best_kana: r.get(5)?,
                ..Default::default()
            })
        })
        .map(|rows| {
            rows.flatten()
                .map(|r| WordMatch::new(Reading::Kanji(r)))
                .collect()
        })
        .unwrap_or_default()
    }
}

/// `find_word_conj_of` — direct match OR match via conjugation.from in seqs.
pub fn find_word_conj_of(conn: &Connection, text: &str, seqs: &[i64]) -> Vec<WordMatch> {
    if seqs.is_empty() {
        return Vec::new();
    }
    let ph = seqs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut params: Vec<rusqlite::types::Value> = std::iter::once(text.to_string().into())
        .chain(seqs.iter().map(|s| (*s).into()))
        .collect();
    let (table, extra_col) = if is_kana(text) {
        ("kana_text", "best_kanji")
    } else {
        ("kanji_text", "best_kana")
    };
    // Direct
    let sql = format!(
        "SELECT id, seq, text, ord, common, {} FROM {} WHERE text = ?1 AND seq IN ({})",
        extra_col, table, ph
    );
    let mut out: Vec<WordMatch> = Vec::new();
    if let Ok(mut stmt) = conn.prepare_cached(&sql) {
        let mk = |kana: bool| {
            move |r: &rusqlite::Row| -> rusqlite::Result<WordMatch> {
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
            }
        };
        let kana = is_kana(text);
        if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(params.clone()), mk(kana)) {
            out.extend(rows.flatten());
        }
    }
    // Via conjugation.from
    let sql2 = format!(
        "SELECT t.id, t.seq, t.text, t.ord, t.common, t.{extra} FROM {table} t \
         JOIN conjugation c ON t.seq = c.seq WHERE t.text = ?1 AND c.\"from\" IN ({ph})",
        table = table,
        extra = extra_col,
        ph = ph
    );
    // params: text + seqs
    params.clear();
    params.push(text.to_string().into());
    params.extend(seqs.iter().map(|s| (*s).into()));
    if let Ok(mut stmt) = conn.prepare_cached(&sql2) {
        let kana = is_kana(text);
        let rows = stmt.query_map(rusqlite::params_from_iter(params), move |r| {
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
            out.extend(rows.flatten());
        }
    }
    out
}

// ============================================================================
// Split macro ports
// ============================================================================

/// Part def: (seqs, length, conjugated). length None = rest of string.
#[allow(dead_code)]
type PartDef = (Vec<i64>, Option<usize>, bool);

/// `def_simple_split` — generic (seq,len,conjugated) parts splitter.
/// Unused by the built-in table but kept for API parity with splits.py.
#[allow(dead_code)]
fn def_simple_split(score: f64, parts: Vec<PartDef>) -> SplitFn {
    Box::new(move |conn: &Connection, reading: &Word| {
        let text: Vec<char> = reading.text().chars().collect();
        let mut offset = 0usize;
        let mut result_parts = Vec::new();
        for (part_seqs, part_length, conjugated) in &parts {
            let part_text: String = match part_length {
                None => text[offset..].iter().collect(),
                Some(len) => text
                    .get(offset..offset + len)
                    .map(|s| s.iter().collect())
                    .unwrap_or_default(),
            };
            if part_text.is_empty() {
                return None;
            }
            let words = if *conjugated {
                find_word_conj_of(conn, &part_text, part_seqs)
            } else {
                find_word_seq(conn, &part_text, part_seqs)
            };
            if words.is_empty() {
                return None;
            }
            result_parts.push(SplitPart {
                word: Word::Simple(words[0].clone()),
                text: part_text,
            });
            if let Some(len) = part_length {
                offset += len;
            }
        }
        Some(SplitResult {
            parts: result_parts,
            score_bonus: score,
            modifiers: HashSet::new(),
        })
    })
}

fn def_de_split(seq_a: i64, score: f64) -> SplitFn {
    Box::new(move |conn: &Connection, reading: &Word| {
        let text = reading.text();
        if !text.ends_with('で') {
            return None;
        }
        let main_text: String = text.chars().take(text.chars().count() - 1).collect();
        let de_text = "で";
        let main_words = find_word_seq(conn, &main_text, &[seq_a]);
        if main_words.is_empty() {
            return None;
        }
        let de_words = find_word_seq(conn, de_text, &[2028980]);
        if de_words.is_empty() {
            return None;
        }
        Some(SplitResult {
            parts: vec![
                SplitPart {
                    word: Word::Simple(main_words[0].clone()),
                    text: main_text,
                },
                SplitPart {
                    word: Word::Simple(de_words[0].clone()),
                    text: de_text.into(),
                },
            ],
            score_bonus: score,
            modifiers: HashSet::new(),
        })
    })
}

fn def_toori_split(seq_a: i64, score: f64, seq_b: i64) -> SplitFn {
    Box::new(move |conn: &Connection, reading: &Word| {
        if reading.word_type() != "kanji" {
            return None;
        }
        let text: Vec<char> = reading.text().chars().collect();
        if text.len() < 3 {
            return None;
        }
        let main_text: String = text[..text.len() - 2].iter().collect();
        let toori_text: String = text[text.len() - 2..].iter().collect();
        let main_words = find_word_seq(conn, &main_text, &[seq_a]);
        if main_words.is_empty() {
            return None;
        }
        let toori_words = find_word_seq(conn, &toori_text, &[seq_b]);
        if toori_words.is_empty() {
            return None;
        }
        Some(SplitResult {
            parts: vec![
                SplitPart {
                    word: Word::Simple(main_words[0].clone()),
                    text: main_text,
                },
                SplitPart {
                    word: Word::Simple(toori_words[0].clone()),
                    text: toori_text,
                },
            ],
            score_bonus: score,
            modifiers: HashSet::new(),
        })
    })
}

fn do_split(seq_b: i64, score: f64) -> SplitFn {
    Box::new(move |conn: &Connection, reading: &Word| {
        let text = reading.text();
        let do_w = find_word_seq(conn, "ど", &[2252690]);
        let rest_text: String = text.chars().skip(1).collect();
        let rest = find_word_seq(conn, &rest_text, &[seq_b]);
        if !do_w.is_empty() && !rest.is_empty() {
            return Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(do_w[0].clone()),
                        text: "ど".into(),
                    },
                    SplitPart {
                        word: Word::Simple(rest[0].clone()),
                        text: rest_text,
                    },
                ],
                score_bonus: score,
                modifiers: HashSet::new(),
            });
        }
        None
    })
}

fn shi_split(seq_b: i64, score: f64) -> SplitFn {
    Box::new(move |conn: &Connection, reading: &Word| {
        let text = reading.text();
        let shi = find_word_conj_of(conn, "し", &[1157170]);
        let rest_text: String = text.chars().skip(1).collect();
        let rest = find_word_conj_of(conn, &rest_text, &[seq_b]);
        if !shi.is_empty() && !rest.is_empty() {
            return Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(shi[0].clone()),
                        text: "し".into(),
                    },
                    SplitPart {
                        word: Word::Simple(rest[0].clone()),
                        text: rest_text,
                    },
                ],
                score_bonus: score,
                modifiers: HashSet::new(),
            });
        }
        None
    })
}

// ============================================================================
// init_splits → build maps
// ============================================================================

fn build_split_map() -> HashMap<i64, SplitFn> {
    let mut m: HashMap<i64, SplitFn> = HashMap::new();

    // -de expressions
    for (seq, seq_a) in [
        (1163700, 1576150),
        (1611020, 1577100),
        (1004800, 1628530),
        (2810720, 1004820),
        (1006840, 1006880),
        (1530610, 1530600),
        (1245390, 1245290),
        (2719270, 1445430),
        (1189420, 2416780),
        (1272220, 1592990),
        (1311360, 1311350),
        (1368500, 1368490),
        (1395670, 1395660),
        (1417790, 1417780),
        (1454270, 1454260),
        (1479100, 1679020),
        (1510140, 1680900),
        (1518550, 1529560),
        (1531420, 1531410),
        (1597400, 1585205),
        (1679990, 2582460),
        (1682060, 2085340),
        (1736650, 1611710),
        (1865020, 1590150),
        (1878880, 2423450),
        (2126220, 1802920),
        (2136520, 2005870),
        (2513590, 2513650),
        (2771850, 2563780),
        (2810800, 1587590),
        (1343110, 1343100),
        (1270210, 1001640),
    ] {
        m.insert(seq, def_de_split(seq_a, 20.0));
    }

    // -通り expressions
    for (seq, seq_a, seq_b) in [
        (1260990, 1260670, 1432930),
        (1414570, 2082450, 1432930),
        (1424950, 1620400, 1432930),
        (1424960, 1423310, 1432930),
        (1820790, 1250090, 1432930),
        (1489800, 1489340, 1432930),
        (1523010, 1522150, 1432930),
        (1808080, 1604890, 1432930),
        (1368820, 1580640, 1432930),
        (1550490, 1550190, 1432930),
        (1619440, 2069220, 1432930),
        (1164910, 2821500, 1432920),
        (1462720, 1461140, 1432920),
    ] {
        m.insert(seq, def_toori_split(seq_a, 50.0, seq_b));
    }

    // ど- prefix splits
    for (seq, seq_b) in [
        (2142710, 1185200),
        (2803190, 1595630),
        (2142680, 1290210),
        (2523480, 1442750),
    ] {
        m.insert(seq, do_split(seq_b, 30.0));
    }

    // し- splits
    for (seq, seq_b) in [
        (1005700, 1156990),
        (1005830, 1370760),
        (1157200, 2772730),
        (1157220, 1195970),
        (1157230, 1284430),
        (1157280, 1370090),
        (1157310, 1405800),
        (1304890, 1256520),
        (1304960, 1307550),
        (1305110, 1338180),
        (1305280, 1599390),
        (1305290, 1212670),
        (1594300, 1596510),
        (1594310, 1406680),
        (1594460, 1372620),
        (1594580, 1277100),
        (2518250, 1332760),
        (1157240, 1600260),
        (1304820, 1207610),
        (2858937, 1406690),
    ] {
        m.insert(seq, shi_split(seq_b, 30.0));
    }

    // Complex splits
    m.insert(
        1529550,
        Box::new(|conn, reading| {
            // なくなる
            let text: Vec<char> = reading.text().chars().collect();
            if text.len() < 3 {
                return None;
            }
            let t1: String = text[..2].iter().collect();
            let t2: String = text[2..].iter().collect();
            let naku = find_word_conj_of(conn, &t1, &[1529520]);
            let naru = find_word_conj_of(conn, &t2, &[1375610]);
            if naku.is_empty() || naru.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(naku[0].clone()),
                        text: t1,
                    },
                    SplitPart {
                        word: Word::Simple(naru[0].clone()),
                        text: t2,
                    },
                ],
                score_bonus: 30.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m.insert(
        1922760,
        Box::new(|conn, reading| {
            // という
            let text = reading.text();
            let rest: String = text.chars().skip(1).collect();
            let to = find_word_seq(conn, "と", &[1008490]);
            let iu = find_word_conj_of(conn, &rest, &[1587040]);
            if to.is_empty() || iu.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(to[0].clone()),
                        text: "と".into(),
                    },
                    SplitPart {
                        word: Word::Simple(iu[0].clone()),
                        text: rest,
                    },
                ],
                score_bonus: 20.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m.insert(
        2755350,
        Box::new(|conn, _reading| {
            // じゃない
            let ja = find_word_seq(conn, "じゃ", &[2089020]);
            let nai = find_word_conj_of(conn, "ない", &[1529520]);
            if ja.is_empty() || nai.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(ja[0].clone()),
                        text: "じゃ".into(),
                    },
                    SplitPart {
                        word: Word::Simple(nai[0].clone()),
                        text: "ない".into(),
                    },
                ],
                score_bonus: 10.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m.insert(
        1009470,
        Box::new(|conn, _reading| {
            // なら
            let nara = find_word_conj_of(conn, "なら", &[2089020]);
            if nara.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![SplitPart {
                    word: Word::Simple(nara[0].clone()),
                    text: "なら".into(),
                }],
                score_bonus: 1.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m.insert(
        1591050,
        Box::new(|conn, reading| {
            // 気がつく
            let text: Vec<char> = reading.text().chars().collect();
            if text.len() < 3 {
                return None;
            }
            let rest: String = text[2..].iter().collect();
            let ki = find_word_seq(conn, "気", &[1221520]);
            let ga = find_word_seq(conn, "が", &[2028930]);
            let tsuku = find_word_conj_of(conn, &rest, &[1495740]);
            if ki.is_empty() || ga.is_empty() || tsuku.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(ki[0].clone()),
                        text: "気".into(),
                    },
                    SplitPart {
                        word: Word::Simple(ga[0].clone()),
                        text: "が".into(),
                    },
                    SplitPart {
                        word: Word::Simple(tsuku[0].clone()),
                        text: rest,
                    },
                ],
                score_bonus: 100.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m.insert(
        1221750,
        Box::new(|conn, _reading| {
            // 気のせい
            let ki = find_word_seq(conn, "気", &[1221520]);
            let no = find_word_seq(conn, "の", &[1469800]);
            let sei = find_word_seq(conn, "せい", &[1610040]);
            if ki.is_empty() || no.is_empty() || sei.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(ki[0].clone()),
                        text: "気".into(),
                    },
                    SplitPart {
                        word: Word::Simple(no[0].clone()),
                        text: "の".into(),
                    },
                    SplitPart {
                        word: Word::Simple(sei[0].clone()),
                        text: "せい".into(),
                    },
                ],
                score_bonus: 100.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m
}

fn build_segsplit_map() -> HashMap<i64, SplitFn> {
    let mut m: HashMap<i64, SplitFn> = HashMap::new();

    m.insert(
        1008570,
        Box::new(|conn, reading| {
            // ところが
            let text: Vec<char> = reading.text().chars().collect();
            if text.len() < 2 {
                return None;
            }
            let main: String = text[..text.len() - 1].iter().collect();
            let tokoro = find_word_seq(conn, &main, &[1343100]);
            let ga = find_word_seq(conn, "が", &[2028930]);
            if tokoro.is_empty() || ga.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(tokoro[0].clone()),
                        text: main,
                    },
                    SplitPart {
                        word: Word::Simple(ga[0].clone()),
                        text: "が".into(),
                    },
                ],
                score_bonus: -10.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m.insert(
        1343110,
        Box::new(|conn, reading| {
            // ところで
            let text: Vec<char> = reading.text().chars().collect();
            if text.len() < 2 {
                return None;
            }
            let main: String = text[..text.len() - 1].iter().collect();
            let tokoro = find_word_seq(conn, &main, &[1343100]);
            let de = find_word_seq(conn, "で", &[2028980]);
            if tokoro.is_empty() || de.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(tokoro[0].clone()),
                        text: main,
                    },
                    SplitPart {
                        word: Word::Simple(de[0].clone()),
                        text: "で".into(),
                    },
                ],
                score_bonus: -10.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m.insert(
        2028950,
        Box::new(|conn, _reading| {
            // とは
            let to = find_word_seq(conn, "と", &[1008490]);
            let ha = find_word_seq(conn, "は", &[2028920]);
            if to.is_empty() || ha.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(to[0].clone()),
                        text: "と".into(),
                    },
                    SplitPart {
                        word: Word::Simple(ha[0].clone()),
                        text: "は".into(),
                    },
                ],
                score_bonus: -5.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m.insert(
        1008450,
        Box::new(|conn, _reading| {
            // では
            let de = find_word_seq(conn, "で", &[2028980]);
            let ha = find_word_seq(conn, "は", &[2028920]);
            if de.is_empty() || ha.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(de[0].clone()),
                        text: "で".into(),
                    },
                    SplitPart {
                        word: Word::Simple(ha[0].clone()),
                        text: "は".into(),
                    },
                ],
                score_bonus: -5.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m.insert(
        1007310,
        Box::new(|conn, _reading| {
            // だから
            let da = find_word_seq(conn, "だ", &[2089020]);
            let kara = find_word_seq(conn, "から", &[1002980]);
            if da.is_empty() || kara.is_empty() {
                return None;
            }
            Some(SplitResult {
                parts: vec![
                    SplitPart {
                        word: Word::Simple(da[0].clone()),
                        text: "だ".into(),
                    },
                    SplitPart {
                        word: Word::Simple(kara[0].clone()),
                        text: "から".into(),
                    },
                ],
                score_bonus: -5.0,
                modifiers: HashSet::new(),
            })
        }),
    );

    m
}

// ============================================================================
// get_split / get_segsplit
// ============================================================================

/// `get_split` — look up a split for word (direct or via conjugation sources).
pub fn get_split(conn: &Connection, word: &Word, conj_of: Option<&[i64]>) -> Option<SplitResult> {
    let seq = word.seq()?;
    if let Some(f) = SPLIT_MAP.get(&seq) {
        if let Some(r) = f(conn, word) {
            if r.parts.iter().all(|p| !matches!(p.word, Word::Counter(_))) {
                return Some(r);
            }
        }
    }
    if let Some(conj_of) = conj_of {
        for cseq in conj_of {
            if let Some(f) = SPLIT_MAP.get(cseq) {
                if let Some(r) = f(conn, word) {
                    if r.parts.iter().all(|p| !matches!(p.word, Word::Counter(_))) {
                        return Some(r);
                    }
                }
            }
        }
    }
    None
}

/// `get_segsplit` — expand a scored segment via segsplit map.
pub fn get_segsplit(conn: &Connection, segment: &Segment) -> Option<Segment> {
    let word = &segment.word;
    let seq = word.seq()?;
    let seq_set: Vec<i64> = segment.info.seq_set.iter().copied().collect();
    let conj_of: Vec<i64> = if seq_set.len() > 1 {
        seq_set[1..].to_vec()
    } else {
        Vec::new()
    };

    let try_fn =
        |s: i64| -> Option<SplitResult> { SEGSPLIT_MAP.get(&s).and_then(|f| f(conn, word)) };

    let result = try_fn(seq).or_else(|| conj_of.iter().find_map(|c| try_fn(*c)));
    result.map(|r| create_split_segment(segment, r))
}

/// `_create_split_segment` — merge split parts into a CompoundWord segment.
fn create_split_segment(original: &Segment, split: SplitResult) -> Segment {
    use crate::types::CompoundWord;
    let text: String = split.parts.iter().map(|p| p.text.as_str()).collect();
    let kana_parts: Vec<String> = split
        .parts
        .iter()
        .map(|p| match &p.word {
            Word::Simple(w) => w.reading.text().to_string(),
            _ => p.text.clone(),
        })
        .collect();
    let words: Vec<WordMatch> = split
        .parts
        .iter()
        .filter_map(|p| p.word.as_simple().cloned())
        .collect();
    let primary = words.first().cloned().unwrap_or_else(|| {
        WordMatch::new(Reading::Kana(crate::db::rows::KanaTextRow {
            text: text.clone(),
            ..Default::default()
        }))
    });
    let compound = CompoundWord {
        text: text.clone(),
        kana: kana_parts.join(" "),
        primary,
        words,
        score_mod: vec![split.score_bonus],
        score_base: None,
        is_abbrev: false,
    };
    let mut new_seg = original.clone();
    new_seg.word = Word::Compound(Box::new(compound));
    new_seg.text_cache = Some(text);
    new_seg.score = original.score + split.score_bonus;
    new_seg
}
