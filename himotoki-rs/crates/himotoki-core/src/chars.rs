//! Character utilities — port of `himotoki/characters.py`.
//!
//! Character class determination, kana conversion, voicing (rendaku),
//! normalization, basic splitting, mora counting, and romanization.
//!
//! All "length" semantics are in `char` counts (Python code points), not bytes.

use std::collections::HashMap;
use std::sync::OnceLock;

// ============================================================================
// Kana Character Mappings
// ============================================================================

pub const SOKUON_CHARS: &str = "っッ";
pub const ITERATION_CHARS: &str = "ゝヽ";
pub const ITERATION_VOICED_CHARS: &str = "ゞヾ";

/// Modifier characters (small kana, long vowel mark).
/// Maps class name -> [hiragana, katakana] (single-char entries have both
/// identical to themselves where Python only had one char).
pub static MODIFIER_CHARS: &[(&str, &str)] = &[
    ("+a", "ぁァ"),
    ("+i", "ぃィ"),
    ("+u", "ぅゥ"),
    ("+e", "ぇェ"),
    ("+o", "ぉォ"),
    ("+ya", "ゃャ"),
    ("+yu", "ゅュ"),
    ("+yo", "ょョ"),
    ("+wa", "ゎヮ"),
    ("long_vowel", "ー"),
];

/// Main kana character mappings: class name -> "hiragana+katakana" pair.
/// Index 0 = hiragana, last = katakana (mirrors Python string indexing).
pub static KANA_CHARS: &[(&str, &str)] = &[
    ("a", "あア"), ("i", "いイ"), ("u", "うウ"), ("e", "えエ"), ("o", "おオ"),
    ("ka", "かカ"), ("ki", "きキ"), ("ku", "くク"), ("ke", "けケ"), ("ko", "こコ"),
    ("sa", "さサ"), ("shi", "しシ"), ("su", "すス"), ("se", "せセ"), ("so", "そソ"),
    ("ta", "たタ"), ("chi", "ちチ"), ("tsu", "つツ"), ("te", "てテ"), ("to", "とト"),
    ("na", "なナ"), ("ni", "にニ"), ("nu", "ぬヌ"), ("ne", "ねネ"), ("no", "のノ"),
    ("ha", "はハ"), ("hi", "ひヒ"), ("fu", "ふフ"), ("he", "へヘ"), ("ho", "ほホ"),
    ("ma", "まマ"), ("mi", "みミ"), ("mu", "むム"), ("me", "めメ"), ("mo", "もモ"),
    ("ya", "やヤ"), ("yu", "ゆユ"), ("yo", "よヨ"),
    ("ra", "らラ"), ("ri", "りリ"), ("ru", "るル"), ("re", "れレ"), ("ro", "ろロ"),
    ("wa", "わワ"), ("wi", "ゐヰ"), ("we", "ゑヱ"), ("wo", "をヲ"),
    ("n", "んン"),
    ("ga", "がガ"), ("gi", "ぎギ"), ("gu", "ぐグ"), ("ge", "げゲ"), ("go", "ごゴ"),
    ("za", "ざザ"), ("ji", "じジ"), ("zu", "ずズ"), ("ze", "ぜゼ"), ("zo", "ぞゾ"),
    ("da", "だダ"), ("dji", "ぢヂ"), ("dzu", "づヅ"), ("de", "でデ"), ("do", "どド"),
    ("ba", "ばバ"), ("bi", "びビ"), ("bu", "ぶブ"), ("be", "べベ"), ("bo", "ぼボ"),
    ("pa", "ぱパ"), ("pi", "ぴピ"), ("pu", "ぷプ"), ("pe", "ぺペ"), ("po", "ぽポ"),
    ("vu", "ゔヴ"),
];

/// Voicing mappings (dakuten): unvoiced class -> voiced class.
pub static DAKUTEN_MAP: &[(&str, &str)] = &[
    ("ka", "ga"), ("ki", "gi"), ("ku", "gu"), ("ke", "ge"), ("ko", "go"),
    ("sa", "za"), ("shi", "ji"), ("su", "zu"), ("se", "ze"), ("so", "zo"),
    ("ta", "da"), ("chi", "dji"), ("tsu", "dzu"), ("te", "de"), ("to", "do"),
    ("ha", "ba"), ("hi", "bi"), ("fu", "bu"), ("he", "be"), ("ho", "bo"),
    ("u", "vu"),
];

/// Handakuten (semi-voicing): h-class -> p-class.
pub static HANDAKUTEN_MAP: &[(&str, &str)] = &[
    ("ha", "pa"), ("hi", "pi"), ("fu", "pu"), ("he", "pe"), ("ho", "po"),
];

/// char -> class name reverse lookup (KANA_CHARS + MODIFIER_CHARS + sokuon/iter).
static CHAR_CLASS_MAP: OnceLock<HashMap<char, &'static str>> = OnceLock::new();

fn char_class_map() -> &'static HashMap<char, &'static str> {
    CHAR_CLASS_MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for (name, chars) in KANA_CHARS {
            for c in chars.chars() {
                m.insert(c, *name);
            }
        }
        for (name, chars) in MODIFIER_CHARS {
            for c in chars.chars() {
                m.insert(c, *name);
            }
        }
        for c in SOKUON_CHARS.chars() {
            m.insert(c, "sokuon");
        }
        for c in ITERATION_CHARS.chars() {
            m.insert(c, "iter");
        }
        for c in ITERATION_VOICED_CHARS.chars() {
            m.insert(c, "iter_v");
        }
        m
    })
}

// ============================================================================
// Character Width Normalization
// ============================================================================

pub const HALF_WIDTH_KANA: &str =
    "･ｦｧｨｩｪｫｬｭｮｯｰｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝﾞﾟ";
pub const FULL_WIDTH_KANA: &str =
    "・ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン゛゜";

/// Abnormal (full/half-width) chars that normalize to NORMAL_CHARS.
/// Note: Python builds this as digits + lowercase + uppercase + symbols + HALF_WIDTH_KANA.
pub const ABNORMAL_CHARS: &str = concat!(
    "０１２３４５６７８９",
    "ａｂｃｄｅｆｇｈｉｊｋｌｍｎｏｐｑｒｓｔｕｖｗｘｙｚ",
    "ＡＢＣＤＥＦＧＨＩＪＫＬＭＮＯＰＱＲＳＴＵＶＷＸＹＺ",
    "＃＄％＆（）＊＋／〈＝〉？＠［］＾＿'｛｜｝～",
    "･ｦｧｨｩｪｫｬｭｮｯｰｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝﾞﾟ",
);

pub const NORMAL_CHARS: &str = concat!(
    "0123456789",
    "abcdefghijklmnopqrstuvwxyz",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    "#$%&()*+/<=>?@[]^_`{|}~",
    "・ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン゛゜",
);

/// Punctuation normalization — order matters ("・・・" before "・").
pub static PUNCTUATION_MAP: &[(&str, &str)] = &[
    ("【", " ["), ("】", "] "),
    ("、", ", "), ("，", ", "),
    ("。", ". "), ("・・・", "... "), ("・", " "), ("　", " "),
    ("「", " \""), ("」", "\" "), ("゛", "\""),
    ("『", " «"), ("』", "» "),
    ("〜", " - "), ("：", ": "), ("！", "! "), ("？", "? "), ("；", "; "),
];

// ============================================================================
// Character class predicates (replacing Python regexes)
// ============================================================================
//
// Python patterns, decoded to codepoint ranges:
//   KATAKANA_PATTERN  [ァ-ヺヽヾー]  = U+30A1..=U+30FA | U+30FC..=U+30FE
//                     (・U+30FB excluded deliberately)
//   KATAKANA_UNIQ     [ァ-ヺヽヾ]    = U+30A1..=U+30FA | U+30FD..=U+30FE
//   HIRAGANA_PATTERN  [ぁ-ゔゝゞー]  = U+3041..=U+3094 | U+309D | U+309E | U+30FC
//   KANJI_PATTERN     [々ヶ〆一-龯] = U+3005 | U+30F6 | U+3030 | U+4E00..=U+9FAF
//   KANJI_CHAR        [一-龯]       = U+4E00..=U+9FAF
//   WORD              kanji|katakana|hiragana|〇 (U+3007)
//   NUM_WORD          WORD | 0-9 | ０-９ (〇 already in WORD)
//   NUMERIC           0-9 ０-９ 〇 一二三四五六七八九零壱弐参拾十百千万億兆京
//   DIGIT             0-9 ０-９ 〇

pub fn is_katakana_char(c: char) -> bool {
    matches!(c, '\u{30A1}'..='\u{30FA}' | '\u{30FC}'..='\u{30FE}')
}

pub fn is_katakana_uniq_char(c: char) -> bool {
    matches!(c, '\u{30A1}'..='\u{30FA}' | '\u{30FD}'..='\u{30FE}')
}

pub fn is_hiragana_char(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3094}' | '\u{309D}' | '\u{309E}' | '\u{30FC}')
}

pub fn is_kanji_char(c: char) -> bool {
    matches!(c, '\u{3005}' | '\u{30F6}' | '\u{3030}' | '\u{4E00}'..='\u{9FAF}')
}

/// Kanji "char" pattern used by sequential_kanji_positions: [々一-龯]
fn is_kanji_seq_char(c: char) -> bool {
    matches!(c, '\u{3005}' | '\u{4E00}'..='\u{9FAF}')
}

pub fn is_kana_char(c: char) -> bool {
    is_katakana_char(c) || is_hiragana_char(c)
}

pub fn is_word_char(c: char) -> bool {
    is_kanji_char(c) || is_kana_char(c) || c == '\u{3007}'
}

pub fn is_num_word_char(c: char) -> bool {
    is_word_char(c) || c.is_ascii_digit() || ('\u{FF10}'..='\u{FF19}').contains(&c)
}

pub fn is_digit_char(c: char) -> bool {
    c.is_ascii_digit() || ('\u{FF10}'..='\u{FF19}').contains(&c) || c == '\u{3007}'
}

pub fn is_numeric_char(c: char) -> bool {
    is_digit_char(c)
        || matches!(
            c,
            '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '零' | '壱' | '弐'
                | '参' | '拾' | '十' | '百' | '千' | '万' | '億' | '兆' | '京'
        )
}

/// Get the kana class name for a character (e.g. 'ka', 'shi', 'n'),
/// or None if the char is not a classed kana.
pub fn get_char_class(c: char) -> Option<&'static str> {
    char_class_map().get(&c).copied()
}

/// word consists entirely of the named class:
/// 'katakana' | 'hiragana' | 'kanji' | 'kana' | 'nonword'
pub fn word_matches_class(word: &str, char_class: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let pred: fn(char) -> bool = match char_class {
        "katakana" => is_katakana_char,
        "hiragana" => is_hiragana_char,
        "kanji" => is_kanji_char,
        "kana" => is_kana_char,
        "nonword" => |c: char| !is_word_char(c),
        _ => return false,
    };
    word.chars().all(pred)
}

/// Count occurrences of a character class in word.
pub fn count_char_class(word: &str, char_class: &str) -> usize {
    let pred: fn(char) -> bool = match char_class {
        "katakana" => is_katakana_char,
        "hiragana" => is_hiragana_char,
        "kanji" => is_kanji_char,
        "kana" => is_kana_char,
        _ => return 0,
    };
    word.chars().filter(|c| pred(*c)).count()
}

pub fn is_katakana(word: &str) -> bool {
    word_matches_class(word, "katakana")
}
pub fn is_hiragana(word: &str) -> bool {
    word_matches_class(word, "hiragana")
}
pub fn is_kanji(word: &str) -> bool {
    word_matches_class(word, "kanji")
}
pub fn is_kana(word: &str) -> bool {
    word_matches_class(word, "kana")
}
pub fn has_kanji(word: &str) -> bool {
    word.chars().any(is_kanji_char)
}
pub fn has_kana(word: &str) -> bool {
    word.chars().any(is_kana_char)
}

// ============================================================================
// Kana Conversion
// ============================================================================

fn kana_pair(name: &str) -> Option<(char, char)> {
    KANA_CHARS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| {
            let mut it = s.chars();
            (it.next().unwrap(), it.next_back().unwrap())
        })
}

fn modifier_pair(name: &str) -> Option<(char, char)> {
    MODIFIER_CHARS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| {
            let mut it = s.chars();
            let first = it.next().unwrap();
            (first, it.next_back().unwrap_or(first))
        })
}

/// Convert katakana to hiragana.
pub fn as_hiragana(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for char in text.chars() {
        match get_char_class(char) {
            Some(cls) if kana_pair(cls).is_some() => {
                result.push(kana_pair(cls).unwrap().0);
            }
            Some(cls) if modifier_pair(cls).is_some() => {
                result.push(modifier_pair(cls).unwrap().0);
            }
            _ => match char {
                'ッ' => result.push('っ'),
                'ー' => result.push('ー'),
                _ if ITERATION_CHARS.contains(char) => result.push('ゝ'),
                _ if ITERATION_VOICED_CHARS.contains(char) => result.push('ゞ'),
                _ => result.push(char),
            },
        }
    }
    result
}

/// Convert hiragana to katakana.
pub fn as_katakana(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for char in text.chars() {
        match get_char_class(char) {
            Some(cls) if kana_pair(cls).is_some() => {
                result.push(kana_pair(cls).unwrap().1);
            }
            Some(cls) if modifier_pair(cls).is_some() => {
                result.push(modifier_pair(cls).unwrap().1);
            }
            _ => match char {
                'っ' => result.push('ッ'),
                'ー' => result.push('ー'),
                _ if ITERATION_CHARS.contains(char) => result.push('ヽ'),
                _ if ITERATION_VOICED_CHARS.contains(char) => result.push('ヾ'),
                _ => result.push(char),
            },
        }
    }
    result
}

// ============================================================================
// Voicing (rendaku / unrendaku / geminate)
// ============================================================================

fn map_get(map: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    map.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn kana_chars_of(name: &str) -> &'static str {
    KANA_CHARS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
        .unwrap_or("")
}

/// Apply rendaku (or handakuten) to the first character.
pub fn rendaku(text: &str, handakuten: bool) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    let mut chars = text.chars();
    let first = chars.next().unwrap();
    let Some(cls) = get_char_class(first) else {
        return text.to_string();
    };
    let voice_map = if handakuten { HANDAKUTEN_MAP } else { DAKUTEN_MAP };
    let Some(voiced_cls) = map_get(voice_map, cls) else {
        return text.to_string();
    };
    let orig_chars = kana_chars_of(cls);
    let Some(pos) = orig_chars.chars().position(|c| c == first) else {
        return text.to_string();
    };
    let voiced_chars = kana_chars_of(voiced_cls);
    match voiced_chars.chars().nth(pos) {
        Some(vc) => format!("{}{}", vc, chars.as_str()),
        None => text.to_string(),
    }
}

/// Remove rendaku from the first character.
pub fn unrendaku(text: &str) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    let mut chars = text.chars();
    let first = chars.next().unwrap();
    let Some(cls) = get_char_class(first) else {
        return text.to_string();
    };
    // UNDAKUTEN_MAP is the reverse of DAKUTEN_MAP
    let Some(unvoiced_cls) = DAKUTEN_MAP
        .iter()
        .find(|(_, v)| *v == cls)
        .map(|(k, _)| *k)
    else {
        return text.to_string();
    };
    let voiced_chars = kana_chars_of(cls);
    let Some(pos) = voiced_chars.chars().position(|c| c == first) else {
        return text.to_string();
    };
    let unvoiced_chars = kana_chars_of(unvoiced_cls);
    match unvoiced_chars.chars().nth(pos) {
        Some(uc) => format!("{}{}", uc, chars.as_str()),
        None => text.to_string(),
    }
}

/// Replace the last character with small tsu (gemination).
pub fn geminate(text: &str) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    *chars.last_mut().unwrap() = 'っ';
    chars.into_iter().collect()
}

// ============================================================================
// Normalization
// ============================================================================

/// Normalize a single character (context: None or "kana").
pub fn normalize_char(c: char, context: Option<&str>) -> char {
    let (abnormal, normal) = if context == Some("kana") {
        (HALF_WIDTH_KANA, FULL_WIDTH_KANA)
    } else {
        (ABNORMAL_CHARS, NORMAL_CHARS)
    };
    if let Some(pos) = abnormal.chars().position(|x| x == c) {
        if let Some(n) = normal.chars().nth(pos) {
            return n;
        }
    }
    c
}

/// Normalize text: width normalization + punctuation mapping.
pub fn normalize(text: &str, context: Option<&str>) -> String {
    let mut s: String = text.chars().map(|c| normalize_char(c, context)).collect();
    if context != Some("kana") {
        for (old, new) in PUNCTUATION_MAP {
            s = s.replace(old, new);
        }
    }
    s
}

// ============================================================================
// Basic splitting
// ============================================================================

/// Split text into ('word', s) / ('misc', s) segments.
/// Mirrors regex (WORD (NUM_WORD)* WORD? | WORD) — equivalent to a maximal
/// NUM_WORD run that starts with a WORD char.
pub fn basic_split(text: &str) -> Vec<(&'static str, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    let n = chars.len();
    while i < n {
        if is_word_char(chars[i]) {
            let start = i;
            i += 1;
            while i < n && is_num_word_char(chars[i]) {
                i += 1;
            }
            result.push(("word", chars[start..i].iter().collect()));
        } else {
            let start = i;
            while i < n && !is_word_char(chars[i]) {
                i += 1;
            }
            result.push(("misc", chars[start..i].iter().collect()));
        }
    }
    result
}

/// Mora length: count of non-modifier characters.
pub fn mora_length(text: &str) -> usize {
    const MODIFIERS: &str = "っッぁァぃィぅゥぇェぉォゃャゅュょョー";
    text.chars().filter(|c| !MODIFIERS.contains(*c)).count()
}

// ============================================================================
// Kanji utilities
// ============================================================================

/// Positions (char index, offset-added) where two kanji-seq chars are adjacent.
/// Mirrors regex (?=[々一-龯][々一-龯]) → position = start+1+offset.
pub fn sequential_kanji_positions(word: &str, offset: usize) -> Vec<usize> {
    let chars: Vec<char> = word.chars().collect();
    let mut positions = Vec::new();
    for i in 0..chars.len().saturating_sub(1) {
        if is_kanji_seq_char(chars[i]) && is_kanji_seq_char(chars[i + 1]) {
            positions.push(i + 1 + offset);
        }
    }
    positions
}

/// Kanji prefix: everything up to and including the last kanji char.
pub fn kanji_prefix(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    match chars.iter().rposition(|c| is_kanji_char(*c)) {
        Some(pos) => chars[..=pos].iter().collect(),
        None => String::new(),
    }
}

/// SQL LIKE mask: replace kanji runs with '%'.
pub fn kanji_mask(word: &str) -> String {
    let mut mask = String::new();
    let mut in_kanji = false;
    for c in word.chars() {
        if is_kanji_char(c) {
            if !in_kanji {
                mask.push('%');
                in_kanji = true;
            }
        } else {
            mask.push(c);
            in_kanji = false;
        }
    }
    mask
}

/// Check whether `reading` matches `word` where each '%' in the mask
/// (kanji run) stands for `.+` (one or more chars).
pub fn kanji_match(word: &str, reading: &str) -> bool {
    let mask = kanji_mask(word);
    let rchars: Vec<char> = reading.chars().collect();
    // Split mask on '%' into literal segments; '%' = .+ (>=1 chars)
    let segs: Vec<Vec<char>> = mask
        .split('%')
        .map(|s| s.chars().collect())
        .collect();
    let n_wild = segs.len() - 1;

    // match positions of literal segments within reading
    fn match_from(
        segs: &[Vec<char>],
        ri: usize,
        rchars: &[char],
        n_wild: usize,
        wi: usize,
    ) -> bool {
        if wi == segs.len() {
            return ri == rchars.len();
        }
        let seg = &segs[wi];
        if wi > 0 {
            // must consume at least 1 char for the wildcard before this seg
            if ri >= rchars.len() {
                return false;
            }
        }
        if wi == 0 {
            // anchor at start
            if seg.len() > rchars.len() {
                return false;
            }
            if rchars[..seg.len()] != seg[..] {
                return false;
            }
            return match_from(segs, seg.len(), rchars, n_wild, 1);
        }
        // wildcard >=1 char then find seg
        let mut ri = ri + 1;
        while ri + seg.len() <= rchars.len() {
            if rchars[ri..ri + seg.len()] == seg[..]
                && match_from(segs, ri + seg.len(), rchars, n_wild, wi + 1)
            {
                return true;
            }
            ri += 1;
        }
        false
    }
    // trailing: if mask ends with '%', last seg is empty -> leftover>=1 ok
    if mask.ends_with('%') {
        // drop trailing empty segment; require >=1 char consumed by wildcard
        let mut segs = segs;
        segs.pop();
        if segs.is_empty() {
            return !rchars.is_empty();
        }
        return match_from(&segs, 0, &rchars, n_wild, 0) || {
            // general: last seg may match earlier leaving >=1 trailing
            fn with_tail(
                segs: &[Vec<char>],
                ri: usize,
                rchars: &[char],
                wi: usize,
            ) -> bool {
                if wi == segs.len() {
                    return ri < rchars.len();
                }
                let seg = &segs[wi];
                let ri = ri + usize::from(wi > 0);
                if ri + seg.len() > rchars.len() {
                    return false;
                }
                let mut ri = ri;
                while ri + seg.len() <= rchars.len() {
                    if rchars[ri..ri + seg.len()] == seg[..]
                        && with_tail(segs, ri + seg.len(), rchars, wi + 1)
                    {
                        return true;
                    }
                    ri += 1;
                }
                false
            }
            with_tail(&segs, 0, &rchars, 0)
        };
    }
    match_from(&segs, 0, &rchars, n_wild, 0)
}

// ============================================================================
// Utility
// ============================================================================

/// Safe substring (char indices). Returns None on out-of-bounds.
pub fn safe_subseq(sequence: &str, start: i64, end: Option<i64>) -> Option<String> {
    let len = sequence.chars().count() as i64;
    if start < 0 || start > len {
        return None;
    }
    if let Some(e) = end {
        if e < start || e > len {
            return None;
        }
    }
    let e = end.unwrap_or(len) as usize;
    let s = start as usize;
    Some(sequence.chars().skip(s).take(e - s).collect())
}

/// Join items with a separator.
pub fn join(separator: &str, items: &[String]) -> String {
    items.join(separator)
}

// ============================================================================
// Romanization
// ============================================================================

fn romaji_of(class: &str) -> Option<&'static str> {
    Some(match class {
        "a" => "a", "i" => "i", "u" => "u", "e" => "e", "o" => "o",
        "ka" => "ka", "ki" => "ki", "ku" => "ku", "ke" => "ke", "ko" => "ko",
        "sa" => "sa", "shi" => "shi", "su" => "su", "se" => "se", "so" => "so",
        "ta" => "ta", "chi" => "chi", "tsu" => "tsu", "te" => "te", "to" => "to",
        "na" => "na", "ni" => "ni", "nu" => "nu", "ne" => "ne", "no" => "no",
        "ha" => "ha", "hi" => "hi", "fu" => "fu", "he" => "he", "ho" => "ho",
        "ma" => "ma", "mi" => "mi", "mu" => "mu", "me" => "me", "mo" => "mo",
        "ya" => "ya", "yu" => "yu", "yo" => "yo",
        "ra" => "ra", "ri" => "ri", "ru" => "ru", "re" => "re", "ro" => "ro",
        "wa" => "wa", "wi" => "wi", "we" => "we", "wo" => "wo",
        "n" => "n",
        "ga" => "ga", "gi" => "gi", "gu" => "gu", "ge" => "ge", "go" => "go",
        "za" => "za", "ji" => "ji", "zu" => "zu", "ze" => "ze", "zo" => "zo",
        "da" => "da", "dji" => "di", "dzu" => "du", "de" => "de", "do" => "do",
        "ba" => "ba", "bi" => "bi", "bu" => "bu", "be" => "be", "bo" => "bo",
        "pa" => "pa", "pi" => "pi", "pu" => "pu", "pe" => "pe", "po" => "po",
        "vu" => "vu",
        "+a" => "a", "+i" => "i", "+u" => "u", "+e" => "e", "+o" => "o",
        "+ya" => "ya", "+yu" => "yu", "+yo" => "yo", "+wa" => "wa",
        "sokuon" | "long_vowel" | "iter" | "iter_v" => "",
        _ => return None,
    })
}

/// Convert kana text to romaji (mirrors Python romanize_word).
pub fn romanize_word(text: &str) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut result: Vec<String> = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let char_class = get_char_class(c);

        let Some(cls) = char_class else {
            result.push(c.to_string());
            i += 1;
            continue;
        };

        // Sokuon — double next consonant
        if cls == "sokuon" {
            if i + 1 < chars.len() {
                if let Some(nc) = get_char_class(chars[i + 1]) {
                    if let Some(romaji) = romaji_of(nc) {
                        if let Some(first) = romaji.chars().next() {
                            if !"aeioun".contains(first) {
                                result.push(first.to_string());
                            }
                        }
                    }
                }
            }
            i += 1;
            continue;
        }

        // Long vowel mark
        if cls == "long_vowel" {
            if let Some(last) = result.last() {
                // Python: `last in 'aeiou'` — substring test, true only when
                // the whole last piece is a single vowel char.
                if last.len() == 1 && "aeiou".contains(last.as_str()) {
                    result.push(last.clone());
                } else {
                    result.push("ō".to_string());
                }
            } else {
                result.push("ō".to_string());
            }
            i += 1;
            continue;
        }

        // Small kana modifiers
        if cls.starts_with('+') {
            let base = romaji_of(cls).unwrap_or("");
            if let Some(last) = result.last_mut() {
                if last.ends_with('i') {
                    let mut l = last.clone();
                    l.pop();
                    l.push_str(base);
                    *last = l;
                } else {
                    result.push(base.to_string());
                }
            } else {
                result.push(base.to_string());
            }
            i += 1;
            continue;
        }

        // Regular kana
        let romaji = romaji_of(cls).unwrap_or("");
        let mut romaji_owned = romaji.to_string();

        // 'n' before vowel/y-initial sounds gets an apostrophe
        if cls == "n" && i + 1 < chars.len() {
            if let Some(nc) = get_char_class(chars[i + 1]) {
                if nc.starts_with('+')
                    || nc.starts_with('a')
                    || nc.starts_with('i')
                    || nc.starts_with('u')
                    || nc.starts_with('e')
                    || nc.starts_with('o')
                    || nc.starts_with('y')
                {
                    romaji_owned = "n'".to_string();
                }
            }
        }

        result.push(romaji_owned);
        i += 1;
    }

    result.concat()
}

// ============================================================================
// Chars helper: char-indexed view over a &str (the segmentation workhorse)
// ============================================================================

/// Char-indexed text buffer — all segment indices are char positions.
#[derive(Debug, Clone)]
pub struct Chars(pub Vec<char>);

impl Chars {
    pub fn new(s: &str) -> Self {
        Chars(s.chars().collect())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Substring by char range -> String
    pub fn slice(&self, start: usize, end: usize) -> String {
        self.0[start..end].iter().collect()
    }
    /// Char at index
    pub fn at(&self, i: usize) -> char {
        self.0[i]
    }
    pub fn as_string(&self) -> String {
        self.0.iter().collect()
    }
    /// mora_length over a char range
    pub fn mora_len(&self, start: usize, end: usize) -> usize {
        const MODIFIERS: &str = "っッぁァぃィぅゥぇェぉォゃャゅュょョー";
        self.0[start..end]
            .iter()
            .filter(|c| !MODIFIERS.contains(**c))
            .count()
    }
}

impl std::ops::Index<usize> for Chars {
    type Output = char;
    fn index(&self, i: usize) -> &char {
        &self.0[i]
    }
}
