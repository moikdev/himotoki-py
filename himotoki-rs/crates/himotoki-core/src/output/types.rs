//! Output types — port of `himotoki/output/types.py`.

use serde_json::{json, Value};

/// `WordType` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordType {
    Kanji,
    Kana,
    Gap,
}

impl WordType {
    pub fn value(&self) -> &'static str {
        match self {
            WordType::Kanji => "kanji",
            WordType::Kana => "kana",
            WordType::Gap => "gap",
        }
    }
}

/// `SPECIAL_CONJ_INFO` — standalone entries that are conjugated forms:
/// seq -> (from_seq, conj_type, pos, neg, fml).
pub fn special_conj_info(seq: i64) -> Option<(i64, i64, &'static str, bool, bool)> {
    match seq {
        1628500 => Some((2089020, 1, "cop", false, true)), // です = formal non-past of だ
        _ => None,
    }
}

/// `ConjStep` — one step in a conjugation breakdown chain.
#[derive(Debug, Clone)]
pub struct ConjStep {
    pub conj_type: String,
    pub suffix: String,
    pub gloss: String,
    pub neg: bool,
    pub fml: bool,
}

/// Particles suppressed from conjugation display.
pub const SUPPRESS_CONJ_FOR_PARTICLES: &[i64] = &[2028980]; // で
/// Nominalized forms with own meaning.
pub const SUPPRESS_CONJ_FOR_NOUNS: &[i64] = &[1382980]; // 積もり
/// Standalone verbs suppressed from conj display.
pub const SUPPRESS_CONJ_FOR_VERBS: &[i64] = &[1345930]; // 傷つける

/// `WordInfo` — output-ready word record.
#[derive(Debug, Clone)]
pub struct WordInfo {
    pub type_: WordType,
    pub text: String,
    /// String or list of readings.
    pub kana: Vec<String>,
    pub true_text: Option<String>,
    /// int or list of ints.
    pub seq: Option<Vec<i64>>,
    /// Conj ids or 'root' — serialized like Python.
    pub conjugations: Option<crate::types::Conj>,
    pub score: i64,
    /// Alternative/compound component WordInfos.
    pub components: Vec<WordInfo>,
    pub compound_texts: Vec<String>,
    pub alternative: bool,
    pub primary: bool,
    pub start: Option<usize>,
    pub end: Option<usize>,
    /// [value, ordinal]
    pub counter: Option<(i64, bool)>,
    pub skipped: usize,
    pub is_compound: bool,
    pub conj_type: Option<String>,
    pub conj_neg: bool,
    pub conj_fml: bool,
    pub source_text: Option<String>,
    pub meanings: Vec<String>,
    pub pos: Option<String>,
}

impl Default for WordInfo {
    fn default() -> Self {
        WordInfo {
            type_: WordType::Gap,
            text: String::new(),
            kana: Vec::new(),
            true_text: None,
            seq: None,
            conjugations: None,
            score: 0,
            components: Vec::new(),
            compound_texts: Vec::new(),
            alternative: false,
            primary: true,
            start: None,
            end: None,
            counter: None,
            skipped: 0,
            is_compound: false,
            conj_type: None,
            conj_neg: false,
            conj_fml: false,
            source_text: None,
            meanings: Vec::new(),
            pos: None,
        }
    }
}

impl WordInfo {
    /// `kana` accessor — Python Union[str, List[str]]; first element used by
    /// callers needing a single reading.
    pub fn kana_str(&self) -> &str {
        self.kana.first().map(|s| s.as_str()).unwrap_or("")
    }
    /// Python emits `kana` verbatim — str for single/empty, list for multi.
    pub fn kana_json(&self) -> Value {
        match self.kana.len() {
            0 => json!(""),
            1 => json!(self.kana[0]),
            _ => json!(self.kana),
        }
    }
    pub fn seq_json(&self) -> Value {
        match &self.seq {
            None => Value::Null,
            Some(v) if v.len() == 1 => json!(v[0]),
            Some(v) => json!(v),
        }
    }
    /// Python `wi.seq` truthiness for follow-up logic.
    pub fn has_seq(&self) -> bool {
        self.seq.as_ref().map(|v| !v.is_empty()).unwrap_or(false)
    }
    /// First seq (Python flattens list → use [0]).
    pub fn first_seq(&self) -> Option<i64> {
        self.seq.as_ref().and_then(|v| v.first().copied())
    }
    /// `conjugations` accessor used by format layer: Conj or None.
    pub fn conjugations_field(&self) -> Option<&crate::types::Conj> {
        self.conjugations.as_ref()
    }
}
