//! Counter word recognition — port of `himotoki/grammar/counters.py`.
//!
//! Pure numeric/kana logic lives here; the DB-backed counter cache and
//! `find_counter` live in `counter_cache` (wired in Phase 4).

use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};

use rusqlite::Connection;

use crate::chars::mora_length;
use crate::db::rows::KanjiTextRow;
use crate::types::{CounterText, DigitOpt, DigitOpts, Reading};

// ============================================================================
// Tables
// ============================================================================

macro_rules! kmap {
    ( $($k:expr => $v:expr),* $(,)? ) => {{
        let mut m = HashMap::new();
        $( m.insert($k, $v); )*
        m
    }};
}

static KANJI_NUMBERS: LazyLock<HashMap<char, i64>> = LazyLock::new(|| {
    kmap![
        '零' => 0, '〇' => 0,
        '一' => 1, '壱' => 1,
        '二' => 2, '弐' => 2,
        '三' => 3, '参' => 3,
        '四' => 4, '五' => 5, '六' => 6, '七' => 7, '八' => 8, '九' => 9,
        '十' => 10, '百' => 100, '千' => 1000, '万' => 10000, '億' => 100_000_000,
    ]
});

static DIGIT_VALUES: LazyLock<HashMap<char, i64>> = LazyLock::new(|| {
    kmap![
        '0' => 0, '０' => 0,
        '1' => 1, '１' => 1,
        '2' => 2, '２' => 2,
        '3' => 3, '３' => 3,
        '4' => 4, '４' => 4,
        '5' => 5, '５' => 5,
        '6' => 6, '６' => 6,
        '7' => 7, '７' => 7,
        '8' => 8, '８' => 8,
        '9' => 9, '９' => 9,
    ]
});

static DIGIT_TO_KANA: LazyLock<HashMap<i64, &'static str>> = LazyLock::new(|| {
    kmap![
        1 => "いち", 2 => "に", 3 => "さん", 4 => "よん", 5 => "ご",
        6 => "ろく", 7 => "なな", 8 => "はち", 9 => "きゅう",
        10 => "じゅう", 100 => "ひゃく", 1000 => "せん", 10000 => "まん",
    ]
});

static POWER_TO_KANA: LazyLock<HashMap<i64, &'static str>> = LazyLock::new(|| {
    kmap![1 => "じゅう", 2 => "ひゃく", 3 => "せん", 4 => "まん"]
});

/// SPECIAL_COUNTER_OPTS: seq → {digit → opts}.
/// opts: Flags(['g','r','h']) or Kana(string replacement).
pub static SPECIAL_COUNTER_OPTS: LazyLock<HashMap<i64, DigitOpts>> = LazyLock::new(|| {
    fn flags(v: &[&str]) -> DigitOpt {
        DigitOpt::Flags(v.iter().map(|s| s.to_string()).collect())
    }
    kmap![
        1583370 => kmap![3 => flags(&["r"])],          // 匹/疋
        1522150 => kmap![3 => flags(&["r"])],          // 本
        2019640 => kmap![3 => flags(&["r"])],          // 杯/盃
        1203020 => kmap![3 => flags(&["r"])],          // 階
        2078590 => kmap![3 => flags(&["r"])],          // 軒
        2208060 => kmap![3 => flags(&["r"])],          // 遍
        1511870 => kmap![3 => flags(&["r"])],          // 編/篇
        2412230 => kmap![3 => flags(&["r"])],          // 足
        2020680 => kmap![4 => DigitOpt::Kana("よ".into()), 7 => DigitOpt::Kana("しち".into()), 9 => DigitOpt::Kana("く".into())], // 時
        1315920 => kmap![4 => DigitOpt::Kana("よ".into()), 9 => DigitOpt::Kana("く".into())], // 時間
        2084840 => kmap![4 => DigitOpt::Kana("よ".into()), 7 => DigitOpt::Kana("しち".into()), 9 => DigitOpt::Kana("く".into())], // 年
        1175570 => kmap![4 => DigitOpt::Kana("よ".into())], // 円
        1502840 => kmap![4 => flags(&["h"])],          // 分
        1514050 => kmap![4 => flags(&["h"])],          // 舗
        1901390 => kmap![4 => flags(&["h"])],          // 敗
        1919550 => kmap![4 => flags(&["h"])],          // 泊
        1487770 => kmap![4 => flags(&["h"])],          // 筆
        2149890 => kmap![4 => DigitOpt::Kana("よ".into()), 7 => DigitOpt::Kana("しち".into())], // 人
        1255430 => kmap![4 => DigitOpt::Kana("し".into()), 7 => DigitOpt::Kana("しち".into()), 9 => DigitOpt::Kana("く".into())], // 月
    ]
});

static DAYS_KUN_READINGS: LazyLock<HashMap<i64, &'static str>> = LazyLock::new(|| {
    kmap![
        1 => "ついたち", 2 => "ふつか", 3 => "みっか", 4 => "よっか",
        5 => "いつか", 6 => "むいか", 7 => "なのか", 8 => "ようか",
        9 => "ここのか", 10 => "とうか", 14 => "じゅうよっか",
        20 => "はつか", 24 => "にじゅうよっか", 30 => "みそか",
    ]
});

static PEOPLE_KUN_READINGS: LazyLock<HashMap<i64, &'static str>> = LazyLock::new(|| {
    kmap![1 => "ひとり", 2 => "ふたり"]
});

pub const EXTRA_COUNTER_IDS: &[i64] = &[1255430, 1606800]; // 月, 割
pub const SKIP_COUNTER_IDS: &[i64] = &[2426510, 2220370, 2248360, 2423450, 2671670, 2735690, 2838543];

/// seq → accepted COUNTER_SUFFIXES keys.
pub static COUNTER_ACCEPTS: LazyLock<HashMap<i64, Vec<&'static str>>> = LazyLock::new(|| {
    kmap![
        1194480 => vec!["kan"],          // 年
        1490430 => vec!["kan"],          // 日
        1333450 => vec!["kan", "kango"], // 週
    ]
});

pub const COUNTER_FOREIGN: &[i64] = &[1120410];

/// key → (text, kana, gloss)
pub static COUNTER_SUFFIXES: &[(&str, &str, &str, &str)] = &[
    ("kan", "間", "かん", "[duration]"),
    ("kango", "間後", "かんご", "[after ...]"),
    ("chuu", "中", "ちゅう", "[among/out of ...]"),
];

static SPECIAL_COUNTER_KANA_OVERRIDE: LazyLock<HashMap<i64, &'static str>> =
    LazyLock::new(|| kmap![1255430 => "がつ"]);

// ============================================================================
// Number parsing — parse_number / parse_kanji_number / number_to_kana
// ============================================================================

pub fn parse_number(text: &str) -> Option<i64> {
    if text.is_empty() {
        return None;
    }
    let mut arabic_str = String::new();
    for ch in text.chars() {
        if let Some(v) = DIGIT_VALUES.get(&ch) {
            arabic_str.push_str(&v.to_string());
        } else if ch.is_ascii_digit() {
            arabic_str.push(ch);
        } else {
            break;
        }
    }
    if !arabic_str.is_empty() && arabic_str.chars().count() == text.chars().count() {
        return arabic_str.parse().ok();
    }
    if !arabic_str.is_empty() {
        if let Ok(arabic_value) = arabic_str.parse::<i64>() {
            let suffix: String = text.chars().skip(arabic_str.chars().count()).collect();
            let mut schars = suffix.chars();
            if suffix.chars().count() == 1 {
                let s = schars.next().unwrap();
                if let Some(&multiplier) = KANJI_NUMBERS.get(&s) {
                    if multiplier >= 10 {
                        return Some(arabic_value * multiplier);
                    }
                }
            }
        }
    }
    parse_kanji_number(text)
}

pub fn parse_kanji_number(text: &str) -> Option<i64> {
    if text.is_empty() {
        return None;
    }
    let mut result: i64 = 0;
    let mut current: i64 = 0;
    for ch in text.chars() {
        let &value = KANJI_NUMBERS.get(&ch)?;
        if value >= 10 {
            if current == 0 {
                current = 1;
            }
            if value >= 10000 {
                result += current * value;
                current = 0;
            } else {
                current *= value;
                result += current;
                current = 0;
            }
        } else {
            if current > 0 {
                result += current;
            }
            current = value;
        }
    }
    result += current;
    if result > 0 || text == "零" || text == "〇" {
        Some(result)
    } else {
        None
    }
}

pub fn number_to_kana(n: i64, separator: &str) -> String {
    if n == 0 {
        return "ゼロ".to_string();
    }
    let mut n = n;
    let mut parts: Vec<String> = Vec::new();
    if n >= 100_000_000 {
        let oku = n / 100_000_000;
        if oku > 1 {
            parts.push(number_to_kana(oku, separator));
        } else {
            parts.push("いち".into());
        }
        parts.push("おく".into());
        n %= 100_000_000;
    }
    if n >= 10000 {
        let man = n / 10000;
        if man > 1 {
            parts.push(number_to_kana(man, separator));
        }
        parts.push("まん".into());
        n %= 10000;
    }
    if n >= 1000 {
        let sen = n / 1000;
        if sen == 3 {
            parts.push("さん".into());
        } else if sen > 1 {
            parts.push(DIGIT_TO_KANA.get(&sen).copied().unwrap_or("").into());
        }
        parts.push("せん".into());
        n %= 1000;
    }
    if n >= 100 {
        let hyaku = n / 100;
        if hyaku == 3 {
            parts.push("さん".into());
            parts.push("びゃく".into());
        } else if hyaku == 6 {
            parts.push("ろっ".into());
            parts.push("ぴゃく".into());
        } else if hyaku == 8 {
            parts.push("はっ".into());
            parts.push("ぴゃく".into());
        } else {
            if hyaku > 1 {
                parts.push(DIGIT_TO_KANA.get(&hyaku).copied().unwrap_or("").into());
            }
            parts.push("ひゃく".into());
        }
        n %= 100;
    }
    if n >= 10 {
        let juu = n / 10;
        if juu > 1 {
            parts.push(DIGIT_TO_KANA.get(&juu).copied().unwrap_or("").into());
        }
        parts.push("じゅう".into());
        n %= 10;
    }
    if n > 0 {
        parts.push(DIGIT_TO_KANA.get(&n).copied().unwrap_or("").into());
    }
    parts.join(separator)
}

// ============================================================================
// Phonetic rules — geminate / rendaku (string-level, counter-local variants)
// ============================================================================

/// Apply sokuon: replace last char with っ.
pub fn counter_geminate(kana: &str) -> String {
    if kana.is_empty() {
        return kana.to_string();
    }
    let mut out: String = kana.chars().take(kana.chars().count() - 1).collect();
    out.push('っ');
    out
}

/// Apply rendaku (voicing) or handakuten to first char.
pub fn counter_rendaku(kana: &str, handakuten: bool) -> String {
    if kana.is_empty() {
        return kana.to_string();
    }
    let mut chars = kana.chars();
    let first = chars.next().unwrap();
    let rest: String = chars.collect();
    let h_to_p = |c: char| -> Option<char> {
        Some(match c {
            'は' => 'ぱ', 'ひ' => 'ぴ', 'ふ' => 'ぷ', 'へ' => 'ぺ', 'ほ' => 'ぽ',
            'ハ' => 'パ', 'ヒ' => 'ピ', 'フ' => 'プ', 'ヘ' => 'ペ', 'ホ' => 'ポ',
            _ => return None,
        })
    };
    let voicing = |c: char| -> Option<char> {
        Some(match c {
            'か' => 'が', 'き' => 'ぎ', 'く' => 'ぐ', 'け' => 'げ', 'こ' => 'ご',
            'さ' => 'ざ', 'し' => 'じ', 'す' => 'ず', 'せ' => 'ぜ', 'そ' => 'ぞ',
            'た' => 'だ', 'ち' => 'ぢ', 'つ' => 'づ', 'て' => 'で', 'と' => 'ど',
            'は' => 'ば', 'ひ' => 'び', 'ふ' => 'ぶ', 'へ' => 'べ', 'ほ' => 'ぼ',
            'カ' => 'ガ', 'キ' => 'ギ', 'ク' => 'グ', 'ケ' => 'ゲ', 'コ' => 'ゴ',
            'サ' => 'ザ', 'シ' => 'ジ', 'ス' => 'ズ', 'セ' => 'ゼ', 'ソ' => 'ゾ',
            'タ' => 'ダ', 'チ' => 'ヂ', 'ツ' => 'ヅ', 'テ' => 'デ', 'ト' => 'ド',
            'ハ' => 'バ', 'ヒ' => 'ビ', 'フ' => 'ブ', 'ヘ' => 'ベ', 'ホ' => 'ボ',
            _ => return None,
        })
    };
    let mapped = if handakuten {
        h_to_p(first)
    } else {
        voicing(first)
    };
    match mapped {
        Some(c) => format!("{}{}", c, rest),
        None => kana.to_string(),
    }
}

/// Phonetic class of the first kana char.
fn kana_head_class(kana: &str) -> Option<&'static str> {
    let first = kana.chars().next()?;
    let groups: &[(&str, &str)] = &[
        ("ka", "かきくけこカキクケコ"),
        ("sa", "さしすせそサシスセソ"),
        ("ta", "たちつてとタチツテト"),
        ("na", "なにぬねのナニヌネノ"),
        ("ha", "はひふへほハヒフヘホ"),
        ("ma", "まみむめもマミムメモ"),
        ("ya", "やゆよヤユヨ"),
        ("ra", "らりるれろラリルレロ"),
        ("wa", "わをんワヲン"),
        ("pa", "ぱぴぷぺぽパピプペポ"),
    ];
    for (g, chars) in groups {
        if chars.contains(first) {
            return Some(g);
        }
    }
    None
}

/// `counter_join` — join number kana + counter kana with phonetic rules.
pub fn counter_join(
    number_value: i64,
    number_kana: &str,
    counter_kana: &str,
    digit_opts: Option<&DigitOpts>,
    foreign: bool,
) -> String {
    let mut digit = number_value % 10;
    if digit == 0 {
        for power in [10i64, 100, 1000, 10000] {
            if number_value % power == 0 && number_value % (power * 10) != 0 {
                digit = power;
                break;
            }
        }
    }
    let head = kana_head_class(counter_kana);

    if let Some(opts) = digit_opts.and_then(|d| d.get(&digit)) {
        match opts {
            DigitOpt::Kana(repl) => {
                let stem_kana = if digit < 10 {
                    DIGIT_TO_KANA.get(&digit).copied().unwrap_or("")
                } else {
                    let power = (digit as f64).log10().round() as i64; // 10→1, 100→2...
                    POWER_TO_KANA.get(&power).copied().unwrap_or("")
                };
                let stem_len = stem_kana.chars().count();
                let nk_len = number_kana.chars().count();
                if stem_len > 0 && nk_len >= stem_len {
                    let head_part: String = number_kana.chars().take(nk_len - stem_len).collect();
                    return format!("{}{}{}", head_part, repl, counter_kana);
                }
                return format!("{}{}", repl, counter_kana);
            }
            DigitOpt::Flags(flags) => {
                let mut result_number = number_kana.to_string();
                let mut result_counter = counter_kana.to_string();
                for opt in flags {
                    match opt.as_str() {
                        "g" => result_number = counter_geminate(&result_number),
                        "r" => result_counter = counter_rendaku(&result_counter, false),
                        "h" => result_counter = counter_rendaku(&result_counter, true),
                        _ => {}
                    }
                }
                return format!("{}{}", result_number, result_counter);
            }
        }
    }

    let mut result_number = number_kana.to_string();
    let mut result_counter = counter_kana.to_string();
    let in_heads = |heads: &[&str]| head.map(|h| heads.contains(&h)).unwrap_or(false);

    if foreign {
        if matches!(digit, 6 | 8 | 10 | 100) && in_heads(&["ka", "sa", "ta", "pa"]) {
            result_number = counter_geminate(&result_number);
        }
    } else {
        match digit {
            1 => {
                if in_heads(&["ka", "sa", "ta"]) {
                    result_number = counter_geminate(&result_number);
                } else if head == Some("ha") {
                    result_number = counter_geminate(&result_number);
                    result_counter = counter_rendaku(&result_counter, true);
                }
            }
            3 => {
                if head == Some("ha") {
                    result_counter = counter_rendaku(&result_counter, true);
                }
            }
            6 => {
                if in_heads(&["ka", "pa"]) {
                    result_number = counter_geminate(&result_number);
                } else if head == Some("ha") {
                    result_number = counter_geminate(&result_number);
                    result_counter = counter_rendaku(&result_counter, true);
                }
            }
            8 => {
                if in_heads(&["ka", "sa", "ta", "pa"]) {
                    result_number = counter_geminate(&result_number);
                } else if head == Some("ha") {
                    result_number = counter_geminate(&result_number);
                    result_counter = counter_rendaku(&result_counter, true);
                }
            }
            10 => {
                if in_heads(&["ka", "sa", "ta", "pa"]) {
                    result_number = counter_geminate(&result_number);
                } else if head == Some("ha") {
                    result_number = counter_geminate(&result_number);
                    result_counter = counter_rendaku(&result_counter, true);
                }
            }
            100 => {
                if head == Some("ka") {
                    result_number = counter_geminate(&result_number);
                } else if head == Some("ha") {
                    result_number = counter_geminate(&result_number);
                    result_counter = counter_rendaku(&result_counter, true);
                }
            }
            1000 | 10000 => {
                if head == Some("ha") {
                    result_counter = counter_rendaku(&result_counter, true);
                }
            }
            _ => {}
        }
    }
    format!("{}{}", result_number, result_counter)
}

// ============================================================================
// Counter cache — DB-backed (Phase 4 wiring)
// ============================================================================

/// One cached counter entry: (counter_text, counter_kana, source,
/// ordinalp, common, digit_opts).
#[derive(Debug, Clone)]
pub struct CounterEntry {
    pub counter_text: String,
    pub counter_kana: String,
    pub source: Option<Reading>,
    pub ordinalp: bool,
    pub common: Option<i64>,
    pub digit_opts: Option<std::sync::Arc<DigitOpts>>,
}

/// counter_text → candidate entries.
static COUNTER_CACHE: OnceLock<HashMap<String, Vec<CounterEntry>>> = OnceLock::new();

/// Init the counter cache from the DB (get_counter_ids + get_counter_readings +
/// init_counter_cache merged).
pub fn init_counter_cache(conn: &Connection) -> rusqlite::Result<()> {
    if COUNTER_CACHE.get().is_some() {
        return Ok(());
    }
    // counter seqs = pos:'ctr' + EXTRA - SKIP
    let mut stmt = conn.prepare(
        "SELECT DISTINCT seq FROM sense_prop WHERE tag='pos' AND text='ctr'",
    )?;
    let mut ids: std::collections::HashSet<i64> = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<_, _>>()?;
    // Python quirk: `set(result) | set(EXTRA) - set(SKIP)` parses as
    // `result | (EXTRA - SKIP)` — `-` binds tighter than `|` — so
    // SKIP_COUNTER_IDS never actually removes anything. Reproduce verbatim.
    for extra in EXTRA_COUNTER_IDS {
        if !SKIP_COUNTER_IDS.contains(extra) {
            ids.insert(*extra);
        }
    }

    // Fetch kanji + kana readings
    let mut cache: HashMap<String, Vec<CounterEntry>> = HashMap::new();
    if !ids.is_empty() {
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, seq, text, ord, common, best_kana FROM kanji_text WHERE seq IN ({}) ORDER BY ord",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<rusqlite::types::Value> =
            ids.iter().map(|i| (*i).into()).collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
            Ok(KanjiTextRow {
                id: r.get(0)?,
                seq: r.get(1)?,
                text: r.get(2)?,
                ord: r.get(3)?,
                common: r.get(4)?,
                best_kana: r.get(5)?,
                ..Default::default()
            })
        })?;
        // primary kana per seq = first kana_text row by ord (or override)
        let mut primary_kana: HashMap<i64, String> = HashMap::new();
        let ksql = format!(
            "SELECT seq, text FROM (SELECT seq, text, ROW_NUMBER() OVER (PARTITION BY seq ORDER BY ord) rn FROM kana_text WHERE seq IN ({})) WHERE rn=1",
            placeholders
        );
        let mut kstmt = conn.prepare(&ksql)?;
        let params2: Vec<rusqlite::types::Value> =
            ids.iter().map(|i| (*i).into()).collect();
        let krows = kstmt.query_map(rusqlite::params_from_iter(params2), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        for r in krows.flatten() {
            primary_kana.insert(r.0, r.1);
        }

        for kt in rows.flatten() {
            let counter_text = kt.text.clone();
            let is_ordinal = counter_text.chars().count() > 1 && counter_text.ends_with('目');
            let pk = SPECIAL_COUNTER_KANA_OVERRIDE
                .get(&kt.seq)
                .map(|s| s.to_string())
                .or_else(|| primary_kana.get(&kt.seq).cloned())
                .unwrap_or_default();
            cache.entry(counter_text.clone()).or_default().push(CounterEntry {
                counter_text,
                counter_kana: pk,
                source: Some(Reading::Kanji(kt.clone())),
                ordinalp: is_ordinal,
                common: kt.common,
                digit_opts: SPECIAL_COUNTER_OPTS
                    .get(&kt.seq)
                    .map(|d| std::sync::Arc::new(d.clone())),
            });
        }
    }
    let _ = COUNTER_CACHE.set(cache);
    Ok(())
}

/// `find_counter` — number_text + counter_text → CounterText list.
pub fn find_counter(
    conn: &Connection,
    number_text: &str,
    counter_text: &str,
) -> Vec<CounterText> {
    if COUNTER_CACHE.get().is_none() {
        let _ = init_counter_cache(conn);
    }
    let cache = match COUNTER_CACHE.get() {
        Some(c) => c,
        None => return Vec::new(),
    };
    let number_value = match parse_number(number_text) {
        Some(v) => v,
        None => return Vec::new(),
    };
    let entries = match cache.get(counter_text) {
        Some(e) => e,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    for args in entries {
        let seq = args.source.as_ref().map(|r| r.seq());
        let mut special_kana: Option<String> = None;
        if seq == Some(2083110) {
            // 日 ka: only kun-reading numbers
            match DAYS_KUN_READINGS.get(&number_value) {
                Some(k) => special_kana = Some(k.to_string()),
                None => continue,
            }
        }
        if seq == Some(2083100) {
            // 日 nichi: skip numbers with kun readings (except 1)
            if DAYS_KUN_READINGS.contains_key(&number_value) && number_value != 1 {
                continue;
            }
        }
        if seq == Some(2149890) {
            if let Some(k) = PEOPLE_KUN_READINGS.get(&number_value) {
                special_kana = Some(k.to_string());
            }
        }
        let full_kana = if let Some(sk) = special_kana {
            sk
        } else {
            let nk = number_to_kana(number_value, "");
            let foreign = seq.map(|s| COUNTER_FOREIGN.contains(&s)).unwrap_or(false);
            counter_join(number_value, &nk, &args.counter_kana, args.digit_opts.as_deref(), foreign)
        };
        results.push(CounterText {
            text: format!("{}{}", number_text, counter_text),
            kana: full_kana,
            number_text: number_text.to_string(),
            number_value,
            counter_text: counter_text.to_string(),
            counter_kana: args.counter_kana.clone(),
            source: args.source.clone(),
            ordinalp: args.ordinalp,
            suffix: None,
            common_override: args.common,
            digit_opts: args.digit_opts.clone(),
        });
    }
    results
}

/// `find_counter_in_text` — scan text for (start, end, CounterText).
pub fn find_counter_in_text(conn: &Connection, text: &str) -> Vec<(usize, usize, CounterText)> {
    let chars: Vec<char> = text.chars().collect();
    let text_len = chars.len();
    let mut results = Vec::new();
    for start in 0..text_len {
        let mut number_end = start;
        while number_end < text_len {
            let ch = chars[number_end];
            if KANJI_NUMBERS.contains_key(&ch) || DIGIT_VALUES.contains_key(&ch) || ch.is_ascii_digit() {
                number_end += 1;
            } else {
                break;
            }
        }
        if number_end == start {
            continue;
        }
        let number_text: String = chars[start..number_end].iter().collect();
        if parse_number(&number_text).is_none() {
            continue;
        }
        let max_end = (number_end + 5).min(text_len + 1);
        for end in (number_end + 1)..max_end {
            let counter_text: String = chars[number_end..end].iter().collect();
            for c in find_counter(conn, &number_text, &counter_text) {
                results.push((start, end, c));
            }
        }
    }
    results
}

/// `calc_counter_score`.
pub fn calc_counter_score(counter: &CounterText) -> i64 {
    let mut score: i64 = 5;
    match counter.common() {
        Some(0) => score += 10,
        Some(c) => score += (15 - c).max(5),
        None => {}
    }
    let word_len = mora_length(&counter.text);
    let length_coeffs = [0i64, 1, 8, 24, 40, 60];
    let coeff = if (word_len as usize) < length_coeffs.len() {
        length_coeffs[word_len as usize]
    } else {
        word_len as i64 * (length_coeffs[5] / 4)
    };
    score * coeff
}
