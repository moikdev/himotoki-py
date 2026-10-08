//! Suffix handling — port of `himotoki/grammar/suffixes.py`.
//!
//! `_suffix_cache` maps suffix text → (handler-key, kana_form) pairs, built
//! once per DB by `init_suffixes`. `kf` carries a `conj` marker ('root'|'conj')
//! matching Python's `kf._conj_type` attribute.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::chars::count_char_class;
use crate::constants::*;
use crate::db::rows::KanaTextRow;
use crate::types::{adjoin_onto, adjoin_word, Reading, Word, WordMatch};

pub const MAX_SUFFIX_DEPTH: usize = 5;

thread_local! {
    static SUFFIX_DEPTH: Cell<usize> = const { Cell::new(0) };
}

// ============================================================================
// Suffix cache structures
// ============================================================================

/// KanaText row + a marker whether the seq itself is root or a conjugation
/// of the suffix-class entry (Python sets `kf._conj_type`).
#[derive(Debug, Clone)]
pub struct SuffixKanaForm {
    pub row: KanaTextRow,
    pub is_conj: bool, // _conj_type == 'conj'
}

pub type SuffixValue = (String, Option<SuffixKanaForm>);

#[derive(Default)]
struct SuffixState {
    cache: HashMap<String, Vec<SuffixValue>>,
    ending_chars: HashSet<char>,
    class_by_seq: HashMap<i64, String>,
    class_by_text: HashMap<String, String>,
    initialized: bool,
}

static SUFFIX_STATE: Mutex<Option<SuffixState>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&SuffixState) -> R) -> Option<R> {
    let guard = SUFFIX_STATE.lock().unwrap();
    guard.as_ref().map(f)
}

fn with_state_mut<R>(f: impl FnOnce(&mut SuffixState) -> R) -> R {
    let mut guard = SUFFIX_STATE.lock().unwrap();
    f(guard.get_or_insert_with(SuffixState::default))
}

fn update_cache(text: &str, value: SuffixValue, join: bool) {
    with_state_mut(|s| match s.cache.get_mut(text) {
        None => {
            s.cache.insert(text.to_string(), vec![value]);
            if let Some(last) = text.chars().last() {
                s.ending_chars.insert(last);
            }
        }
        Some(v) => {
            if join {
                v.push(value);
            } else {
                *v = vec![value];
            }
        }
    });
}

// ============================================================================
// DB helpers: get_kana_forms / get_kana_form
// ============================================================================

fn get_kana_forms(conn: &Connection, seq: i64) -> Vec<SuffixKanaForm> {
    let mut out = Vec::new();
    let direct_sql = "SELECT id, seq, text, ord, common, best_kanji FROM kana_text WHERE seq = ?1";
    if let Ok(mut stmt) = conn.prepare_cached(direct_sql) {
        if let Ok(rows) = stmt.query_map([seq], |r| {
            Ok(KanaTextRow {
                id: r.get(0)?,
                seq: r.get(1)?,
                text: r.get(2)?,
                ord: r.get(3)?,
                common: r.get(4)?,
                best_kanji: r.get(5)?,
                ..Default::default()
            })
        }) {
            for r in rows.flatten() {
                out.push(SuffixKanaForm {
                    row: r,
                    is_conj: false,
                });
            }
        }
    }
    // Kana for conjugations of seq
    let conj_seqs: Vec<i64> = conn
        .prepare_cached("SELECT seq FROM conjugation WHERE \"from\" = ?1")
        .and_then(|mut s| {
            s.query_map([seq], |r| r.get::<_, i64>(0))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();
    if !conj_seqs.is_empty() {
        let ph = conj_seqs.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, seq, text, ord, common, best_kanji FROM kana_text WHERE seq IN ({})",
            ph
        );
        if let Ok(mut stmt) = conn.prepare_cached(&sql) {
            let params: Vec<rusqlite::types::Value> =
                conj_seqs.iter().map(|i| (*i).into()).collect();
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(params), |r| {
                Ok(KanaTextRow {
                    id: r.get(0)?,
                    seq: r.get(1)?,
                    text: r.get(2)?,
                    ord: r.get(3)?,
                    common: r.get(4)?,
                    best_kanji: r.get(5)?,
                    ..Default::default()
                })
            }) {
                for r in rows.flatten() {
                    out.push(SuffixKanaForm {
                        row: r,
                        is_conj: true,
                    });
                }
            }
        }
    }
    out
}

fn get_kana_form(conn: &Connection, seq: i64, text: &str, conj: bool) -> Option<SuffixKanaForm> {
    conn.prepare_cached(
        "SELECT id, seq, text, ord, common, best_kanji FROM kana_text WHERE seq = ?1 AND text = ?2 LIMIT 1",
    )
    .and_then(|mut s| {
        s.query_row(
            rusqlite::params![seq, text],
            |r| {
                Ok(KanaTextRow {
                    id: r.get(0)?,
                    seq: r.get(1)?,
                    text: r.get(2)?,
                    ord: r.get(3)?,
                    common: r.get(4)?,
                    best_kanji: r.get(5)?,
                    ..Default::default()
                })
            },
        )
    })
    .ok()
    .map(|row| SuffixKanaForm { row, is_conj: conj })
}

fn load_conjs(
    conn: &Connection,
    key: &str,
    seq: i64,
    suffix_class: Option<&str>,
    join: bool,
    include_kanji: bool,
) {
    let actual_class = suffix_class.unwrap_or(key).to_string();
    for kf in get_kana_forms(conn, seq) {
        update_cache(&kf.row.text, (key.to_string(), Some(kf.clone())), join);
        with_state_mut(|s| {
            s.class_by_seq.insert(kf.row.seq, actual_class.clone());
        });
        if include_kanji {
            let kanji_texts: Vec<String> = conn
                .prepare_cached("SELECT text FROM kanji_text WHERE seq = ?1")
                .and_then(|mut st| {
                    st.query_map([kf.row.seq], |r| r.get::<_, String>(0))
                        .map(|rows| rows.flatten().collect())
                })
                .unwrap_or_default();
            for kt in kanji_texts {
                if kt != kf.row.text {
                    update_cache(&kt, (key.to_string(), Some(kf.clone())), join);
                }
            }
        }
    }
}

fn load_kf(
    key: &str,
    kf: &SuffixKanaForm,
    suffix_class: Option<&str>,
    text: Option<&str>,
    join: bool,
) {
    let actual_text = text.unwrap_or(&kf.row.text).to_string();
    let actual_class = suffix_class.unwrap_or(key).to_string();
    update_cache(&actual_text, (key.to_string(), Some(kf.clone())), join);
    with_state_mut(|s| {
        s.class_by_seq.insert(kf.row.seq, actual_class);
    });
}

fn load_abbr(key: &str, text: &str, join: bool, suffix_class: Option<&str>) {
    update_cache(text, (key.to_string(), None), join);
    if let Some(cls) = suffix_class {
        with_state_mut(|s| {
            s.class_by_text.insert(text.to_string(), cls.to_string());
        });
    }
}

/// `get_suffix_description` — class lookup by seq, else by text.
pub fn get_suffix_description(seq: Option<i64>, text: Option<&str>) -> Option<String> {
    with_state(|s| {
        if let Some(seq) = seq {
            let class = s.class_by_seq.get(&seq).map(|c| c.as_str());
            // SUFFIX_DESCRIPTION keyed by class-name or seq
            if let Some(cls) = class {
                if let Some(d) = suffix_description(cls) {
                    return Some(d.to_string());
                }
            } else if let Some(d) = suffix_description_seq(seq) {
                return Some(d.to_string());
            }
        }
        if let Some(t) = text {
            if let Some(cls) = s.class_by_text.get(t) {
                if let Some(d) = suffix_description(cls) {
                    return Some(d.to_string());
                }
            }
        }
        None
    })
    .flatten()
}

/// Keyword → description (SUFFIX_DESCRIPTION string-keyed part).
fn suffix_description(key: &str) -> Option<&'static str> {
    Some(match key {
        "chau" => "indicates completion / to do something by accident or regret",
        "ha" => "topic marker particle",
        "tai" => "want to... / would like to...",
        "iru" => "indicates continuing action (to be ...ing)",
        "oru" => "indicates continuing action (to be ...ing) (humble)",
        "aru" => "indicates completion / finished action",
        "kuru" => "indicates action that had been continuing up till now / came to be",
        "oku" => "to do in advance / to leave in the current state expecting a later change",
        "kureru" => "(asking) to do something for one",
        "morau" => "(asking) to get somebody to do something",
        "itadaku" => "(asking) to get somebody to do something (polite)",
        "iku" => "is becoming / action starting now and continuing",
        "miru" => "to try doing ...",
        "ageru" => "to do something for someone",
        "hoshii" => "want someone to ...",
        "suru" => "makes a verb from a noun",
        "itasu" => "makes a verb from a noun (humble)",
        "sareru" => "makes a verb from a noun (honorific or passive)",
        "saseru" => "let/make someone/something do ...",
        "rou" => "probably / it seems that... / I guess ...",
        "ii" => "it's ok if ... / is it ok if ...?",
        "mo" => "even if ...",
        "sugiru" => "to be too (much) ...",
        "nikui" => "difficult to...",
        "sa" => "-ness (degree or condition of adjective)",
        "tsutsu" => "while ... / in the process of ...",
        "nagara" => "while doing ... / simultaneously ...",
        "tsutsuaru" => "to be doing ... / to be in the process of doing ...",
        "tsuzukeru" => "to continue ...",
        "uru" => "can ... / to be able to ...",
        "sou" => "looking like ... / seeming ...",
        "nai" => "negative suffix",
        "naide" => "without doing ... / don't",
        "ra" => "pluralizing suffix (not polite)",
        "kudasai" => "please do ...",
        "yagaru" => "indicates disdain or contempt",
        "naru" => "to become ...",
        "desu" => "formal copula",
        "desho" => "it seems/perhaps/don't you think?",
        "tosuru" => "to try to .../to be about to...",
        "garu" => "to feel .../have a ... impression of someone",
        "me" => "somewhat/-ish",
        "gai" => "worth it to ...",
        "tasou" => "seem to want to... (tai+sou)",
        "nade" => "na-adj conjunctive (and/being)",
        "nakereba" => "must do / have to (contraction)",
        "shimashou" => "let's ... (polite volitional contraction)",
        "kurai" => "about/approximately/to the extent of",
        "ppoi" => "-ish / tends to / apt to",
        "gatai" => "difficult to... / hard to... (literary)",
        "dasu" => "to burst out doing / to start suddenly",
        "kiru" => "to do completely / to finish doing",
        "kata" => "way of doing / how to ...",
        "mi" => "-ness / depth of feeling (nominalization)",
        "yasui" => "easy to ... / likely to ...",
        "makuru" => "to do relentlessly / to keep doing",
        "naosu" => "to redo / to do again",
        "sokonau" => "to fail to do / to miss doing",
        "wasureru" => "to forget to do",
        "oeru" => "to finish doing",
        "zurai" => "difficult to ... / hard to ...",
        "gimi" => "tending to / -ish / -like",
        "ppanashi" => "left doing / leaving as is",
        "tachi" => "plural (people/animals)",
        "au" => "to do mutually / to do together",
        "komu" => "to do into / to do thoroughly",
        "houdai" => "as much as one likes / unlimited",
        "owaru" => "to finish doing",
        "hajimeru" => "to start doing / to begin to",
        "tsukeru" => "to be accustomed to doing",
        "yaru" => "to do for (someone, casual)",
        "mairu" => "to go/come (humble, te-form auxiliary)",
        "kudasaru" => "to kindly do for (honorific)",
        "sashiageru" => "to do for (humble, respectful)",
        _ => return None,
    })
}

// ============================================================================
// init_suffixes
// ============================================================================

pub fn init_suffixes(conn: &Connection, reset: bool) {
    let ready = with_state(|s| s.initialized).unwrap_or(false);
    if ready && !reset {
        return;
    }
    {
        let mut guard = SUFFIX_STATE.lock().unwrap();
        *guard = Some(SuffixState::default());
    }

    // ちゃう / ちまう
    load_conjs(conn, "chau", SEQ_CHAU, None, false, false);
    load_conjs(conn, "chau", SEQ_CHIMAU, None, false, false);

    // は particle with ちゃ/じゃ readings
    if let Some(ha_kf) = get_kana_form(conn, SEQ_WA, "は", false) {
        load_kf("chau", &ha_kf, Some("ha"), Some("ちゃ"), false);
        load_kf("chau", &ha_kf, Some("ha"), Some("じゃ"), false);
    }

    // たい
    load_conjs(conn, "tai", SEQ_TAI, None, false, false);

    // たそう (synthetic 900000)
    if let Some(tasou) = get_kana_form(conn, 900000, "たそう", false) {
        load_kf("tai", &tasou, Some("tasou"), None, false);
    }

    // 難い
    load_conjs(conn, "ren-", SEQ_NIKUI, Some("nikui"), false, false);

    // おる/ある humble progressive / result state
    load_conjs(conn, "te", SEQ_ORU, Some("oru"), false, false);
    load_conjs(conn, "te", SEQ_ARU, Some("aru"), false, false);

    // いる progressive — two-level cache: full → 'teiru+', tkf[1:] → 'teiru'
    for kf in get_kana_forms(conn, SEQ_IRU) {
        let tkf = kf.row.text.clone();
        let tlen = tkf.chars().count();
        if tlen > 1 {
            update_cache(&tkf, ("teiru+".to_string(), Some(kf.clone())), false);
            let tail: String = tkf.chars().skip(1).collect();
            update_cache(&tail, ("teiru".to_string(), Some(kf.clone())), false);
        } else {
            update_cache(&tkf, ("teiru".to_string(), Some(kf.clone())), false);
        }
        with_state_mut(|s| {
            s.class_by_seq.insert(kf.row.seq, "iru".to_string());
        });
    }

    // くる coming to be
    load_conjs(conn, "te", SEQ_KURU, Some("kuru"), false, false);

    // おく in advance (+ とく contraction)
    load_conjs(conn, "te", SEQ_OKU, Some("oku"), false, false);
    load_conjs(conn, "to", SEQ_TOKU, Some("oku"), false, false);

    // しまう completion (via chau)
    load_conjs(conn, "te", SEQ_SHIMAU, Some("chau"), false, false);

    // くれる/もらう/いただく/みる/あげる/ほしい/やる/まいる/くださる/さしあげる
    for (seq, cls) in [
        (SEQ_KURERU, "kureru"),
        (SEQ_MORAU, "morau"),
        (SEQ_ITADAKU, "itadaku"),
        (SEQ_MIRU, "miru"),
        (SEQ_AGERU, "ageru"),
        (SEQ_HOSHII, "hoshii"),
        (SEQ_YARU, "yaru"),
        (SEQ_MAIRU, "mairu"),
        (SEQ_KUDASARU, "kudasaru"),
        (SEQ_SASHIAGERU, "sashiageru"),
    ] {
        load_conjs(conn, "te+space", seq, Some(cls), false, false);
    }

    // いく going/becoming — special: starts with い
    for kf in get_kana_forms(conn, SEQ_IKU) {
        let tkf = kf.row.text.clone();
        if tkf.starts_with('い') {
            update_cache(&tkf, ("te".to_string(), Some(kf.clone())), false);
            let tlen = tkf.chars().count();
            if tlen > 1 {
                let tail: String = tkf.chars().skip(1).collect();
                update_cache(&tail, ("te".to_string(), Some(kf.clone())), false);
            }
        }
        with_state_mut(|s| {
            s.class_by_seq.insert(kf.row.seq, "iku".to_string());
        });
    }

    // いい
    if let Some(ii_kf) = get_kana_form(conn, SEQ_II, "いい", false) {
        load_kf("teii", &ii_kf, Some("ii"), None, false);
    }

    // もいい
    if let Some(moii_kf) = get_kana_form(conn, SEQ_MOII, "もいい", false) {
        load_kf("teii", &moii_kf, Some("ii"), Some("もいい"), false);
    } else {
        load_abbr("teii", "もいい", false, Some("ii"));
    }

    // も
    if let Some(mo_kf) = get_kana_form(conn, SEQ_MO, "も", false) {
        load_kf("te", &mo_kf, Some("mo"), None, false);
    }

    // ください
    // Python: get_kana_form(..., conj='root') — 'root' tag ≠ 'conj' → no conj ids
    if let Some(k) = get_kana_form(conn, SEQ_KUDASAI, "ください", false) {
        load_kf("kudasai", &k, None, None, false);
    }

    // する/いたす/される/させる
    load_conjs(conn, "suru", SEQ_SURU, None, false, false);
    load_conjs(conn, "suru", SEQ_ITASU, Some("itasu"), false, false);
    load_conjs(conn, "suru", SEQ_SARERU, Some("sareru"), false, false);
    load_conjs(conn, "suru", SEQ_SASERU, Some("saseru"), false, false);

    // そう (only SEQ_SOU — NOT SEQ_SOU_NI_NAI)
    load_conjs(conn, "sou", SEQ_SOU, None, false, false);

    // すぎる
    load_conjs(conn, "sugiru", 1195970, None, false, false);

    // さ
    if let Some(sa) = get_kana_form(conn, 2029120, "さ", false) {
        load_kf("sa", &sa, None, None, false);
    }

    // つつ / つつある
    if let Some(t) = get_kana_form(conn, 1008120, "つつ", false) {
        load_kf("ren", &t, Some("tsutsu"), None, false);
    }
    load_conjs(conn, "ren", 2027910, Some("tsutsuaru"), false, false);

    // ながら
    if let Some(n) = get_kana_form(conn, SEQ_NAGARA, "ながら", false) {
        load_kf("ren", &n, Some("nagara"), None, false);
    }

    // 続ける (include kanji so 続けて matches)
    load_conjs(conn, "ren", 1405800, Some("tsuzukeru"), false, true);

    // っぽい
    if let Some(p) = get_kana_form(conn, SEQ_PPOI, "っぽい", false) {
        load_kf("ppoi", &p, None, None, false);
    }

    // がたい
    if let Some(g) = get_kana_form(conn, SEQ_GATAI, "がたい", false) {
        load_kf("ren", &g, Some("gatai"), None, false);
    }

    // 出す / 切る / 方
    if let Some(d) = get_kana_form(conn, SEQ_DASU, "だす", false) {
        load_kf("ren", &d, Some("dasu"), None, false);
        load_kf("ren", &d, Some("dasu"), Some("出す"), false);
    }
    if let Some(k) = get_kana_form(conn, SEQ_KIRU, "きる", false) {
        load_kf("ren", &k, Some("kiru"), None, false);
        load_kf("ren", &k, Some("kiru"), Some("切る"), false);
    }
    if let Some(k) = get_kana_form(conn, SEQ_KATA, "かた", false) {
        load_kf("ren", &k, Some("kata"), None, false);
        load_kf("ren", &k, Some("kata"), Some("方"), false);
    }

    // み
    if let Some(m) = get_kana_form(conn, SEQ_MI, "み", false) {
        load_kf("mi", &m, Some("mi"), None, false);
    }

    // やすい / まくる / なおす / そこなう / わすれる / おえる / づらい / ぎみ / っぱなし
    for (seq, text, cls) in [
        (SEQ_YASUI, "やすい", "yasui"),
        (SEQ_MAKURU, "まくる", "makuru"),
        (SEQ_NAOSU, "なおす", "naosu"),
        (SEQ_SOKONAU, "そこなう", "sokonau"),
        (SEQ_WASURERU, "わすれる", "wasureru"),
        (SEQ_OERU, "おえる", "oeru"),
        (SEQ_ZURAI, "づらい", "zurai"),
        (SEQ_GIMI, "ぎみ", "gimi"),
    ] {
        if let Some(kf) = get_kana_form(conn, seq, text, false) {
            load_kf("ren", &kf, Some(cls), None, false);
        }
    }
    if let Some(p) = get_kana_form(conn, SEQ_PPANASHI, "っぱなし", false) {
        load_kf("ren", &p, Some("ppanashi"), None, false);
    }

    // たち
    if let Some(t) = get_kana_form(conn, SEQ_TACHI, "たち", false) {
        load_kf("tachi", &t, None, None, false);
    }

    // 合う / 込む / 放題 / 終わる / 始める / つける
    if let Some(a) = get_kana_form(conn, SEQ_AU, "あう", false) {
        load_kf("ren", &a, Some("au"), None, false);
        load_kf("ren", &a, Some("au"), Some("合う"), false);
    }
    if let Some(k) = get_kana_form(conn, SEQ_KOMU, "こむ", false) {
        load_kf("ren", &k, Some("komu"), None, false);
        load_kf("ren", &k, Some("komu"), Some("込む"), false);
    }
    if let Some(h) = get_kana_form(conn, SEQ_HOUDAI, "ほうだい", false) {
        load_kf("ren", &h, Some("houdai"), None, false);
        load_kf("ren", &h, Some("houdai"), Some("放題"), false);
    }
    if let Some(o) = get_kana_form(conn, SEQ_OWARU, "おわる", false) {
        load_kf("ren+", &o, Some("owaru"), None, false);
        load_kf("ren+", &o, Some("owaru"), Some("終わる"), false);
    }
    if let Some(h) = get_kana_form(conn, SEQ_HAJIMERU, "はじめる", false) {
        load_kf("ren+", &h, Some("hajimeru"), None, false);
        load_kf("ren+", &h, Some("hajimeru"), Some("始める"), false);
    }
    if let Some(t) = get_kana_form(conn, SEQ_TSUKERU, "つける", false) {
        load_kf("ren", &t, Some("tsukeru"), None, false);
    }
    if let Some(u) = get_kana_form(conn, 1454500, "うる", false) {
        load_kf("ren", &u, Some("uru"), None, false);
    }

    // なく — kana_text.join(conjugation).from_seq == 1529520
    let naku_kf: Option<KanaTextRow> = conn
        .query_row(
            "SELECT kt.id, kt.seq, kt.text, kt.ord, kt.common, kt.best_kanji \
             FROM kana_text kt JOIN conjugation c ON kt.seq = c.seq \
             WHERE kt.text = 'なく' AND c.\"from\" = 1529520 LIMIT 1",
            [],
            |r| {
                Ok(KanaTextRow {
                    id: r.get(0)?,
                    seq: r.get(1)?,
                    text: r.get(2)?,
                    ord: r.get(3)?,
                    common: r.get(4)?,
                    best_kanji: r.get(5)?,
                    ..Default::default()
                })
            },
        )
        .ok();
    if let Some(nk) = naku_kf {
        // Python loads via _load_kf — _conj_type never set → suffix word gets no conj ids
        let nkf = SuffixKanaForm {
            row: nk,
            is_conj: false,
        };
        load_kf("neg", &nkf, Some("nai"), None, false);
    }

    // ないで
    if let Some(n) = get_kana_form(conn, 2258690, "ないで", false) {
        load_kf("neg", &n, Some("naide"), None, false);
    }

    // なる
    load_conjs(conn, "adv", 1375610, Some("naru"), false, false);

    // やがる
    load_conjs(conn, "teren", 1012740, Some("yagaru"), false, false);

    // ら
    if let Some(r) = get_kana_form(conn, 2067770, "ら", false) {
        load_kf("ra", &r, None, None, false);
    }

    // らしい
    load_conjs(conn, "rashii", 1013240, None, false, false);

    // です / でした
    if let Some(d) = get_kana_form(conn, 1628500, "です", false) {
        load_kf("desu", &d, None, None, false);
    }
    if let Some(d) = get_kana_form(conn, 10044689, "でした", false) {
        load_kf("desu", &d, None, Some("でした"), false);
    }

    // とする
    load_conjs(conn, "tosuru", 2136890, None, false, false);

    // くらい/ぐらい
    if let Some(k) = get_kana_form(conn, 1154340, "くらい", false) {
        load_kf("kurai", &k, None, None, false);
    }
    if let Some(g) = get_kana_form(conn, 1154340, "ぐらい", false) {
        load_kf("kurai", &g, None, None, false);
    }

    // がる / がち / げ / め / がい
    load_conjs(conn, "garu", 1631750, None, false, false);
    if let Some(g) = get_kana_form(conn, 2016470, "がち", false) {
        load_kf("ren", &g, Some("gachi"), None, false);
    }
    if let Some(g) = get_kana_form(conn, 2006580, "げ", false) {
        load_kf("iadj", &g, None, None, false);
    }
    if let Some(m) = get_kana_form(conn, 1604890, "め", false) {
        load_kf("iadj", &m, Some("me"), None, false);
    }
    if let Some(g) = get_kana_form(conn, 2606690, "がい", false) {
        load_kf("ren-", &g, Some("gai"), None, false);
    }

    // Abbreviations
    for abbr in ["ねえ", "ねぇ", "ねー"] {
        load_abbr("nai", abbr, false, None);
    }
    for abbr in ["ず", "ざる", "ぬ"] {
        load_abbr("nai-x", abbr, false, None);
    }
    load_abbr("nai-n", "ん", false, None);
    load_abbr("nakereba", "なきゃ", false, Some("nakereba"));
    load_abbr("nakereba", "なくちゃ", false, Some("nakereba"));
    load_abbr("teba", "ちゃ", true, None);
    load_abbr("reba", "りゃ", false, None);
    load_abbr("keba", "きゃ", false, None);
    load_abbr("geba", "ぎゃ", false, None);
    load_abbr("neba", "にゃ", false, None);
    load_abbr("beba", "びゃ", false, None);
    load_abbr("meba", "みゃ", false, None);
    load_abbr("seba", "しゃ", false, None);
    load_abbr("shimashou", "ましょ", false, Some("shimashou"));
    load_abbr("dewanai", "じゃない", false, None);
    load_abbr("ii", "ええ", false, None);
    load_abbr("nade", "で", false, Some("nade"));

    with_state_mut(|s| s.initialized = true);
}

pub fn is_suffix_cache_ready() -> bool {
    with_state(|s| s.initialized).unwrap_or(false)
}

/// `could_have_suffix` — last-char quick filter.
pub fn could_have_suffix(word: &str) -> bool {
    if !is_suffix_cache_ready() || word.chars().count() < 2 {
        return false;
    }
    let last = match word.chars().last() {
        Some(c) => c,
        None => return false,
    };
    with_state(|s| s.ending_chars.contains(&last)).unwrap_or(false)
}

// ============================================================================
// Suffix map / lookup
// ============================================================================

/// `get_suffix_map` — end position → (suffix_text, keyword, kf).
/// Positions are char offsets.
pub fn get_suffix_map(
    conn: &Connection,
    text: &str,
) -> HashMap<usize, Vec<(String, String, Option<SuffixKanaForm>)>> {
    init_suffixes(conn, false);
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut result: HashMap<usize, Vec<(String, String, Option<SuffixKanaForm>)>> = HashMap::new();
    with_state(|s| {
        for start in 0..n {
            for end in (start + 1)..=n {
                let substr: String = chars[start..end].iter().collect();
                if let Some(vals) = s.cache.get(substr.as_str()) {
                    for (keyword, kf) in vals {
                        result.entry(end).or_default().push((
                            substr.clone(),
                            keyword.clone(),
                            kf.clone(),
                        ));
                    }
                }
            }
        }
    });
    result
}

/// `get_suffixes` — suffix matches scanning backwards (char-indexed).
pub fn get_suffixes(
    conn: &Connection,
    word: &str,
) -> Vec<(String, String, Option<SuffixKanaForm>)> {
    init_suffixes(conn, false);
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    with_state(|s| {
        let mut results = Vec::new();
        for start in (1..n).rev() {
            let substr: String = chars[start..].iter().collect();
            if let Some(vals) = s.cache.get(substr.as_str()) {
                for (keyword, kf) in vals {
                    results.push((substr.clone(), keyword.clone(), kf.clone()));
                }
            }
        }
        results
    })
    .unwrap_or_default()
}

// ============================================================================
// Matching tables
// ============================================================================

pub static SUFFIX_UNIQUE_ONLY: &[&str] = &[
    "ra", "mo", "nikui", "gai", "nai-n", "dewanai", "eba", "teba", "reba", "keba", "geba", "neba",
    "beba", "meba", "seba", "ii",
];

lazy_set! {
    pub static BLOCKED_SUFFIX_WORDS: HashSet<&'static str> = {
        "ため", "だめ", "さげ", "もまず",
    }
}

pub static ABBREVIATION_SUFFIXES: &[&str] = &[
    "nai",
    "nai-x",
    "nai-n",
    "nakereba",
    "shimashou",
    "dewanai",
    "teba",
    "reba",
    "keba",
    "geba",
    "neba",
    "beba",
    "meba",
    "seba",
    "ii",
];

fn abbreviation_stem(keyword: &str) -> usize {
    match keyword {
        "nai" | "nai-x" | "nai-n" | "teba" | "reba" | "keba" | "geba" | "neba" | "beba"
        | "meba" | "seba" | "ii" => 2,
        "nakereba" | "shimashou" | "dewanai" => 4,
        _ => 0,
    }
}

fn suffix_score(keyword: &str) -> f64 {
    match keyword {
        "tai" => 5.0,
        "ren" => 5.0,
        "ren+" => 10.0,
        "ren-" => 0.0,
        "neg" => 5.0,
        "te" => 0.0,
        "teiru" => 3.0,
        "teiru+" => 6.0,
        "te+space" => 3.0,
        "teren" => 4.0,
        "teii" => 1.0,
        "chau" => 5.0,
        "to" => 0.0,
        "suru" => 5.0,
        "sou" => 60.0,
        "sou+" => 1.0,
        "adv" => 1.0,
        "sugiru" => 5.0,
        "sa" => 2.0,
        "iadj" => 1.0,
        "mi" => 1.0,
        "garu" => 0.0,
        "ra" => 1.0,
        "rashii" => 3.0,
        "ppoi" => 3.0,
        "tachi" => 3.0,
        "desu" => 200.0,
        "tosuru" => 3.0,
        "kurai" => 3.0,
        "nai" => 5.0,
        "kudasai" => 360.0,
        "nade" => 3.0,
        _ => 0.0,
    }
}

fn suffix_connector(keyword: &str) -> &'static str {
    match keyword {
        "suru" | "kudasai" | "te+space" | "teii" => " ",
        _ => "",
    }
}

/// `get_sou_score` — conditional そう score_mod.
pub fn get_sou_score(root: &str) -> f64 {
    if root == "から" {
        40.0
    } else if root == "い" {
        0.0
    } else if root == "出来" {
        100.0
    } else if root.chars().count() <= 2 && count_char_class(root, "kanji") == 0 {
        10.0
    } else {
        60.0
    }
}

// ============================================================================
// find_word_suffix
// ============================================================================

type SuffixMap = HashMap<usize, Vec<(String, String, Option<SuffixKanaForm>)>>;

/// `find_word_suffix` — main entry. `matches` = existing direct matches
/// (drives SUFFIX_UNIQUE_ONLY); `suffix_map`/`next_end` precomputed map mode.
pub fn find_word_suffix(
    conn: &Connection,
    word: &str,
    matches: &[WordMatch],
    suffix_map: Option<&SuffixMap>,
    next_end: Option<usize>,
    depth: usize,
) -> Vec<Word> {
    if BLOCKED_SUFFIX_WORDS.contains(word) {
        return Vec::new();
    }
    let current = SUFFIX_DEPTH.with(|d| d.get());
    let effective_depth = depth.max(current);
    if effective_depth >= MAX_SUFFIX_DEPTH {
        return Vec::new();
    }
    SUFFIX_DEPTH.with(|d| d.set(effective_depth + 1));
    let result = find_word_suffix_inner(conn, word, matches, suffix_map, next_end);
    SUFFIX_DEPTH.with(|d| d.set(effective_depth));
    result
}

fn find_word_suffix_inner(
    conn: &Connection,
    word: &str,
    matches: &[WordMatch],
    suffix_map: Option<&SuffixMap>,
    next_end: Option<usize>,
) -> Vec<Word> {
    init_suffixes(conn, false);
    let word_chars: Vec<char> = word.chars().collect();
    let wlen = word_chars.len();

    let suffixes: Vec<(String, String, Option<SuffixKanaForm>)> =
        if let (Some(map), Some(ne)) = (suffix_map, next_end) {
            map.get(&ne).cloned().unwrap_or_default()
        } else {
            get_suffixes(conn, word)
        };

    let mut results: Vec<Word> = Vec::new();

    for (suffix, keyword, kf) in suffixes {
        let suffix_class = kf
            .as_ref()
            .and_then(|f| with_state(|s| s.class_by_seq.get(&f.row.seq).cloned()).flatten())
            .unwrap_or_else(|| keyword.clone());
        if !matches.is_empty() && SUFFIX_UNIQUE_ONLY.contains(&suffix_class.as_str()) {
            continue;
        }
        let offset = wlen.saturating_sub(suffix.chars().count());
        if offset == 0 {
            continue;
        }
        let root: String = word_chars[..offset].iter().collect();

        let handler = crate::grammar::suffix_handlers::get_handler(&keyword);
        let primary_words: Vec<Word> = match handler {
            Some(h) => h(conn, &root, &suffix, kf.as_ref()),
            None => Vec::new(),
        };

        for pw in primary_words {
            // Conjugation ids for conjugated suffix forms
            let suffix_conj_ids: Option<Vec<i64>> = match &kf {
                Some(f) if f.is_conj => {
                    let ids: Vec<i64> = conn
                        .prepare_cached("SELECT id FROM conjugation WHERE seq = ?1")
                        .and_then(|mut st| {
                            st.query_map([f.row.seq], |r| r.get::<_, i64>(0))
                                .map(|rows| rows.flatten().collect())
                        })
                        .unwrap_or_default();
                    if ids.is_empty() {
                        None
                    } else {
                        Some(ids)
                    }
                }
                _ => None,
            };

            let suffix_word = match &kf {
                Some(f) => {
                    let mut sw = WordMatch::new(Reading::Kana(f.row.clone()));
                    if let Some(ids) = &suffix_conj_ids {
                        sw.conjugations = crate::types::Conj::Ids(ids.clone());
                    }
                    sw
                }
                None => WordMatch::new(Reading::Placeholder(crate::types::PlaceholderRow {
                    text: suffix.clone(),
                })),
            };

            let score_mod = if keyword == "sou" || keyword == "sou+" {
                get_sou_score(&root)
            } else {
                suffix_score(&keyword)
            };
            let connector = suffix_connector(&keyword);
            let is_abbrev = ABBREVIATION_SUFFIXES.contains(&keyword.as_str());

            // primary kana via get_word_kana equivalent
            let mut primary_kana = get_word_kana(conn, &pw);
            let mut suffix_kana = kf
                .as_ref()
                .map(|f| f.row.text.clone())
                .unwrap_or_else(|| suffix.clone());

            let abbr_stem = abbreviation_stem(&keyword);
            if abbr_stem > 0 && primary_kana.chars().count() > abbr_stem {
                let keep = primary_kana.chars().count() - abbr_stem;
                primary_kana = primary_kana.chars().take(keep).collect();
            }

            // chau/to contraction kana fixup
            if keyword == "chau" || keyword == "to" {
                if primary_kana.ends_with('て') || primary_kana.ends_with('で') {
                    primary_kana = primary_kana
                        .chars()
                        .take(primary_kana.chars().count() - 1)
                        .collect();
                }
                suffix_kana = suffix.clone();
            }
            // teiru contraction fixup
            if keyword == "teiru" {
                if let Some(f) = &kf {
                    if suffix.chars().count() < f.row.text.chars().count() {
                        suffix_kana = suffix.clone();
                    }
                }
            }

            let compound_kana = format!("{}{}{}", primary_kana, connector, suffix_kana);

            match pw {
                Word::Compound(mut cw) => {
                    adjoin_onto(
                        &mut cw,
                        suffix_word,
                        Some(word.to_string()),
                        Some(compound_kana),
                        score_mod,
                        is_abbrev,
                    );
                    results.push(Word::Compound(cw));
                }
                Word::Simple(wm) => {
                    let cw = adjoin_word(
                        wm,
                        suffix_word,
                        Some(word.to_string()),
                        Some(compound_kana),
                        score_mod,
                        None,
                        is_abbrev,
                    );
                    results.push(Word::Compound(Box::new(cw)));
                }
                Word::Counter(_) => {}
            }
        }
    }
    results
}

/// `get_word_kana` — kana for a primary word: compound kana, kanji→ord-matched
/// kana, fallback first kana for seq, else reading text.
fn get_word_kana(conn: &Connection, w: &Word) -> String {
    if let Word::Compound(cw) = w {
        return cw.kana.clone();
    }
    if let Some(reading) = w.reading() {
        let seq = reading.seq();
        if seq != 0 {
            if let Reading::Kanji(k) = reading {
                // match on ord
                if let Ok(kana) = conn
                    .prepare_cached(
                        "SELECT text FROM kana_text WHERE seq = ?1 AND ord = ?2 LIMIT 1",
                    )
                    .and_then(|mut s| {
                        s.query_row(rusqlite::params![seq, k.ord], |r| r.get::<_, String>(0))
                    })
                {
                    return kana;
                }
            }
            if let Ok(kana) = conn
                .prepare_cached("SELECT text FROM kana_text WHERE seq = ?1 LIMIT 1")
                .and_then(|mut s| s.query_row([seq], |r| r.get::<_, String>(0)))
            {
                return kana;
            }
        }
        return reading.text().to_string();
    }
    w.text().to_string()
}
