//! Data types for word lookup and segmentation — port of `himotoki/types.py`.
//!
//! Python duck-types `WordMatch | CompoundWord | CounterText` for `word`
//! and `KanjiText | KanaText | Raw*Reading` for `reading`. In Rust:
//! `Reading` and `Word` enums with accessor methods matching the Python
//! properties exactly.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use crate::db::rows::{ConjPropRow, KanaTextRow, KanjiTextRow};

/// Placeholder suffix reading (find_word_suffix abbreviation path —
/// Python builds a minimal object with text/seq=None/ord=0/common=None).
#[derive(Debug, Clone, Default)]
pub struct PlaceholderRow {
    pub text: String,
}

// ============================================================================
// Reading — kanji_text / kana_text row wrapper
// ============================================================================

#[derive(Debug, Clone)]
pub enum Reading {
    Kanji(KanjiTextRow),
    Kana(KanaTextRow),
    Placeholder(PlaceholderRow),
}

impl Reading {
    pub fn seq(&self) -> i64 {
        match self {
            Reading::Kanji(r) => r.seq,
            Reading::Kana(r) => r.seq,
            Reading::Placeholder(_) => 0, // Python None
        }
    }
    pub fn id(&self) -> i64 {
        match self {
            Reading::Kanji(r) => r.id,
            Reading::Kana(r) => r.id,
            Reading::Placeholder(_) => 0,
        }
    }
    pub fn text(&self) -> &str {
        match self {
            Reading::Kanji(r) => &r.text,
            Reading::Kana(r) => &r.text,
            Reading::Placeholder(r) => &r.text,
        }
    }
    pub fn ord(&self) -> i64 {
        match self {
            Reading::Kanji(r) => r.ord,
            Reading::Kana(r) => r.ord,
            Reading::Placeholder(_) => 0,
        }
    }
    pub fn common(&self) -> Option<i64> {
        match self {
            Reading::Kanji(r) => r.common,
            Reading::Kana(r) => r.common,
            Reading::Placeholder(_) => None,
        }
    }
    /// Whether this reading is a kanji (vs kana) row — Python `word_type`.
    pub fn is_kanji(&self) -> bool {
        matches!(self, Reading::Kanji(_))
    }
    /// `best_kana` on kanji rows / `best_kanji` on kana rows.
    pub fn best_kana(&self) -> Option<&str> {
        match self {
            Reading::Kanji(r) => r.best_kana.as_deref(),
            _ => None,
        }
    }
    pub fn best_kanji(&self) -> Option<&str> {
        match self {
            Reading::Kana(r) => r.best_kanji.as_deref(),
            _ => None,
        }
    }
    /// `nokanji` flag (only present on full selects; None on hot-path selects).
    pub fn nokanji(&self) -> Option<bool> {
        match self {
            Reading::Kanji(r) => r.nokanji,
            Reading::Kana(r) => r.nokanji,
            Reading::Placeholder(_) => None,
        }
    }
    /// The kana form of this reading (for compound kana derivation —
    /// mirrors adjoin_word's get_word_kana).
    pub fn kana_text(&self) -> &str {
        match self {
            Reading::Kanji(r) => r.best_kana.as_deref().unwrap_or(&r.text),
            Reading::Kana(r) => &r.text,
            Reading::Placeholder(r) => &r.text,
        }
    }
}

// ============================================================================
// Conjugations — tri-state: Unset / Root / Ids
// ============================================================================

/// Mirrors Python `Optional[List[int] | 'root']`.
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub enum Conj {
    #[default]
    Unset,
    Root,
    Ids(Vec<i64>),
}


impl Conj {
    pub fn is_root(&self) -> bool {
        matches!(self, Conj::Root)
    }
    pub fn ids(&self) -> Option<&[i64]> {
        match self {
            Conj::Ids(v) => Some(v),
            _ => None,
        }
    }
}

// ============================================================================
// ConjData — conjugation chain record
// ============================================================================

#[derive(Debug, Clone)]
pub struct ConjData {
    pub seq: i64,         // conjugated entry seq
    pub from_seq: i64,    // root entry seq
    pub via: Option<i64>, // intermediate seq for secondary conjugations
    pub prop: Option<ConjPropRow>,
    /// (conjugated_text, source_text) pairs
    pub src_map: Vec<(String, String)>,
}

// ============================================================================
// ScoreInfo — typed replacement for the Python `info` dict
// ============================================================================

/// Key census (Python `info` dict): posi, seq_set, conj, common, score_info,
/// kpcl, counter, conj_type, neg, fml, source_text.
#[derive(Debug, Clone, Default)]
pub struct ScoreInfo {
    /// POS tag set ('n', 'prt', ...) — set semantics, order irrelevant.
    pub posi: HashSet<String>,
    /// seq + conjugation-source seqs.
    pub seq_set: HashSet<i64>,
    /// Conjugation chain data.
    pub conj: Vec<ConjData>,
    /// Effective common value (None = not common).
    pub common: Option<i64>,
    /// [prop_score, use_length_bonus] bookkeeping + kanji_break + split info.
    pub prop_score: f64,
    pub kanji_break: Vec<usize>,
    pub use_length_bonus: f64,
    pub split_info: Option<SplitInfo>,
    /// [kanji_or_katakana, primary, common, long]
    pub kpcl: [bool; 4],
    /// Counter-expression flag.
    pub counter: bool,
    // --- written later by the output/conj layer ---
    pub conj_type: Option<String>,
    pub neg: bool,
    pub fml: bool,
    pub source_text: Option<String>,
}

/// Metadata for split-scoring adjustments (see calc_score split branch).
#[derive(Debug, Clone)]
pub enum SplitInfo {
    Score(f64),
    Pscore(f64),
    Split { bonus: f64, part_scores: Vec<f64> },
}

// ============================================================================
// WordMatch — a single dictionary hit
// ============================================================================

#[derive(Debug, Clone)]
pub struct WordMatch {
    pub reading: Reading,
    pub conjugations: Conj,
    pub hinted: bool,
}

impl WordMatch {
    pub fn new(reading: Reading) -> Self {
        WordMatch {
            reading,
            conjugations: Conj::Unset,
            hinted: false,
        }
    }
    pub fn seq(&self) -> i64 {
        self.reading.seq()
    }
    pub fn text(&self) -> &str {
        self.reading.text()
    }
    pub fn common(&self) -> Option<i64> {
        self.reading.common()
    }
    pub fn ord(&self) -> i64 {
        self.reading.ord()
    }
    /// Python: `'kanji' if isinstance(reading, KanjiText) else 'kana'` — where the
    /// check is actually `hasattr(reading, 'best_kanji')`, so PlaceholderReading
    /// (no best_kanji attr) reports 'kanji'.
    pub fn word_type(&self) -> &'static str {
        match self.reading {
            Reading::Kanji(_) | Reading::Placeholder(_) => "kanji",
            Reading::Kana(_) => "kana",
        }
    }
    /// Python `word_match.seq` — None for placeholder readings.
    pub fn seq_opt(&self) -> Option<i64> {
        match self.reading {
            Reading::Placeholder(_) => None,
            _ => Some(self.reading.seq()),
        }
    }
    /// `conjugations == 'root'`
    pub fn is_root(&self) -> bool {
        self.conjugations.is_root()
    }
    /// Kana form — kanji reading's best_kana else its text; kana's own text.
    pub fn kana(&self) -> &str {
        self.reading.kana_text()
    }
}

// ============================================================================
// CompoundWord — word1 + suffix word(s), e.g. 食べている
// ============================================================================

#[derive(Debug, Clone)]
pub struct CompoundWord {
    pub text: String,
    pub kana: String,
    pub primary: WordMatch,
    pub words: Vec<WordMatch>,
    /// score_mod accumulated; Python stores float or list — we always keep
    /// a Vec with the join's mods (newest first, mirroring list prepends).
    pub score_mod: Vec<f64>,
    pub score_base: Option<WordMatch>,
    pub is_abbrev: bool,
}

impl CompoundWord {
    pub fn seq(&self) -> i64 {
        self.primary.seq()
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn ord(&self) -> i64 {
        self.primary.ord()
    }
    pub fn common(&self) -> Option<i64> {
        self.primary.common()
    }
    pub fn word_type(&self) -> &'static str {
        self.primary.word_type()
    }
    pub fn components(&self) -> Vec<&str> {
        self.words.iter().map(|w| w.text()).collect()
    }
    /// Conjugations of the last word in the compound.
    pub fn conjugations(&self) -> Option<&Conj> {
        self.words.last().map(|w| &w.conjugations)
    }
    pub fn conjugations_mut(&mut self) -> Option<&mut Conj> {
        self.words.last_mut().map(|w| &mut w.conjugations)
    }
    pub fn get_score_base(&self) -> &WordMatch {
        self.score_base.as_ref().unwrap_or(&self.primary)
    }
}

// ============================================================================
// CounterText — number + counter compound (e.g. 三匹)
// ============================================================================

/// Per-digit phonetic options for counters: either a list of flags
/// ('g' geminate, 'r' rendaku, 'h' handakuten) or a whole-string kana
/// replacement (e.g. 4: 'よ').
#[derive(Debug, Clone)]
pub enum DigitOpt {
    Flags(Vec<String>),
    Kana(String),
}

pub type DigitOpts = HashMap<i64, DigitOpt>;

#[derive(Debug, Clone)]
pub struct CounterText {
    pub text: String,
    pub kana: String,
    pub number_text: String,
    pub number_value: i64,
    pub counter_text: String,
    pub counter_kana: String,
    pub source: Option<Reading>,
    pub ordinalp: bool,
    pub suffix: Option<String>,
    pub common_override: Option<i64>,
    pub digit_opts: Option<Arc<DigitOpts>>,
}

impl CounterText {
    pub fn seq(&self) -> Option<i64> {
        self.source.as_ref().map(|r| r.seq())
    }
    pub fn ord(&self) -> i64 {
        self.source.as_ref().map(|r| r.ord()).unwrap_or(0)
    }
    pub fn common(&self) -> Option<i64> {
        if let Some(c) = self.common_override {
            return Some(c);
        }
        match &self.source {
            Some(r) => r.common(),
            None => Some(0),
        }
    }
    /// `word_type` — 'kanji' if text has kanji else 'kana'.
    pub fn word_type(&self) -> &'static str {
        if crate::chars::count_char_class(&self.text, "kanji") > 0 {
            "kanji"
        } else {
            "kana"
        }
    }
}

// ============================================================================
// Word — the polymorphic word type
// ============================================================================

#[derive(Debug, Clone)]
pub enum Word {
    Simple(WordMatch),
    Compound(Box<CompoundWord>),
    Counter(Box<CounterText>),
}

impl Word {
    /// Python `word.seq` — can be None for CounterText without source.
    pub fn seq(&self) -> Option<i64> {
        match self {
            Word::Simple(w) => Some(w.seq()),
            Word::Compound(c) => Some(c.seq()),
            Word::Counter(c) => c.seq(),
        }
    }
    pub fn text(&self) -> &str {
        match self {
            Word::Simple(w) => w.text(),
            Word::Compound(c) => c.text(),
            Word::Counter(c) => &c.text,
        }
    }
    pub fn common(&self) -> Option<i64> {
        match self {
            Word::Simple(w) => w.common(),
            Word::Compound(c) => c.common(),
            Word::Counter(c) => c.common(),
        }
    }
    pub fn ord(&self) -> i64 {
        match self {
            Word::Simple(w) => w.ord(),
            Word::Compound(c) => c.ord(),
            Word::Counter(c) => c.ord(),
        }
    }
    /// 'kanji' | 'kana'
    pub fn word_type(&self) -> &'static str {
        match self {
            Word::Simple(w) => w.word_type(),
            Word::Compound(c) => c.word_type(),
            Word::Counter(c) => c.word_type(),
        }
    }
    /// `word.conjugations` — Unset/Root/Ids. Compound delegates to last word;
    /// Counter is Unset.
    pub fn conjugations(&self) -> &Conj {
        match self {
            Word::Simple(w) => &w.conjugations,
            Word::Compound(c) => c
                .words
                .last()
                .map(|w| &w.conjugations)
                .unwrap_or(&Conj::Unset),
            Word::Counter(_) => &Conj::Unset,
        }
    }
    /// `conjugations == 'root'` / Counter: always root.
    pub fn is_root(&self) -> bool {
        match self {
            Word::Simple(w) => w.is_root(),
            Word::Compound(_) => false,
            Word::Counter(_) => true,
        }
    }
    pub fn is_compound(&self) -> bool {
        matches!(self, Word::Compound(_))
    }
    pub fn is_counter(&self) -> bool {
        matches!(self, Word::Counter(_))
    }
    pub fn components(&self) -> Vec<String> {
        match self {
            Word::Simple(_) | Word::Counter(_) => Vec::new(),
            Word::Compound(c) => c.components().iter().map(|s| s.to_string()).collect(),
        }
    }
    /// `.reading` — for Compound this is primary.reading; Counter's Python
    /// `reading` returns self — callers that need a Reading use `source`.
    pub fn reading(&self) -> Option<&Reading> {
        match self {
            Word::Simple(w) => Some(&w.reading),
            Word::Compound(c) => Some(&c.primary.reading),
            Word::Counter(c) => c.source.as_ref(),
        }
    }
    /// `reading.nokanji` with the Python hasattr-guard semantics.
    pub fn reading_nokanji(&self) -> Option<bool> {
        self.reading().and_then(|r| r.nokanji())
    }
    pub fn as_simple(&self) -> Option<&WordMatch> {
        match self {
            Word::Simple(w) => Some(w),
            _ => None,
        }
    }
    pub fn as_compound(&self) -> Option<&CompoundWord> {
        match self {
            Word::Compound(c) => Some(c),
            _ => None,
        }
    }
    pub fn as_counter(&self) -> Option<&CounterText> {
        match self {
            Word::Counter(c) => Some(c),
            _ => None,
        }
    }
}

/// Port of `adjoin_word` (types.py:246). Mutates `word1` when it is already a
/// CompoundWord; otherwise creates a new compound.
pub fn adjoin_word(
    word1: WordMatch,
    word2: WordMatch,
    text: Option<String>,
    kana: Option<String>,
    score_mod: f64,
    score_base: Option<WordMatch>,
    is_abbrev: bool,
) -> CompoundWord {
    let text = text.unwrap_or_else(|| format!("{}{}", word1.text(), word2.text()));
    let kana = kana.unwrap_or_else(|| format!("{}{}", word1.kana(), word2.kana()));
    CompoundWord {
        text,
        kana,
        primary: word1.clone(),
        words: vec![word1, word2],
        score_mod: vec![score_mod],
        score_base,
        is_abbrev,
    }
}

/// Extend an existing compound (Python adjoin_word's compound branch):
/// set text/kana, append word, PREPEND score_mod, or-flag is_abbrev.
pub fn adjoin_onto(
    compound: &mut CompoundWord,
    word2: WordMatch,
    text: Option<String>,
    kana: Option<String>,
    score_mod: f64,
    is_abbrev: bool,
) {
    compound.text = text.unwrap_or_else(|| format!("{}{}", compound.text, word2.text()));
    compound.kana = kana.unwrap_or_else(|| format!("{}{}", compound.kana, word2.kana()));
    compound.words.push(word2);
    let mut mods = vec![score_mod];
    mods.extend_from_slice(&compound.score_mod);
    compound.score_mod = mods;
    if is_abbrev {
        compound.is_abbrev = true;
    }
}

// ============================================================================
// Segment / SegmentList / path nodes
// ============================================================================

#[derive(Debug, Clone)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub word: Word,
    pub score: f64,
    pub info: ScoreInfo,
    /// cached text (Python lazy `get_text`)
    pub text_cache: Option<String>,
    pub top: bool,
    /// filter_id -> cached result (Python `_filter_cache`)
    pub filter_cache: RefCell<HashMap<u32, bool>>,
}

impl Segment {
    pub fn new(start: usize, end: usize, word: Word) -> Self {
        Segment {
            start,
            end,
            word,
            score: 0.0,
            info: ScoreInfo::default(),
            text_cache: None,
            top: false,
            filter_cache: RefCell::new(HashMap::new()),
        }
    }
    pub fn get_text(&mut self) -> &str {
        if self.text_cache.is_none() {
            self.text_cache = Some(self.word.text().to_string());
        }
        self.text_cache.as_deref().unwrap()
    }
    /// Non-caching text accessor for shared refs.
    pub fn text(&self) -> &str {
        self.word.text()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SegmentList {
    /// Rc so that filtered/cloned lists share segments (Python's reference
    /// semantics make list copies shallow — mirrors that cost profile).
    pub segments: Vec<Rc<Segment>>,
    pub start: usize,
    pub end: usize,
    /// DP scratch — populated by find_best_path.
    pub top: Option<crate::segment::TopArray>,
    pub matches: usize,
}

impl SegmentList {
    pub fn new(segments: Vec<Rc<Segment>>, start: usize, end: usize, matches: usize) -> Self {
        SegmentList {
            segments,
            start,
            end,
            top: None,
            matches,
        }
    }

    /// Build from owned segments (join loop).
    pub fn from_owned(segments: Vec<Segment>, start: usize, end: usize, matches: usize) -> Self {
        SegmentList::new(
            segments.into_iter().map(Rc::new).collect(),
            start,
            end,
            matches,
        )
    }
}

// ============================================================================
// Synergy (grammar/synergies.py) — defined here so PathNode can use it
// ============================================================================

#[derive(Debug, Clone)]
pub struct Synergy {
    pub description: String,
    pub connector: String,
    pub score: f64,
    pub start: usize,
    pub end: usize,
}

/// Node in a best-path: SegmentList, lone Segment, or Synergy marker.
#[derive(Debug, Clone)]
pub enum PathNode {
    List(Rc<SegmentList>),
    Seg(Rc<Segment>),
    Syn(Rc<Synergy>),
}

impl PathNode {
    pub fn start(&self) -> usize {
        match self {
            PathNode::List(l) => l.start,
            PathNode::Seg(s) => s.start,
            PathNode::Syn(s) => s.start,
        }
    }
    pub fn end(&self) -> usize {
        match self {
            PathNode::List(l) => l.end,
            PathNode::Seg(s) => s.end,
            PathNode::Syn(s) => s.end,
        }
    }
    /// `get_segment_score` equivalent.
    pub fn score(&self) -> f64 {
        match self {
            PathNode::List(l) => l.segments.iter().map(|s| s.score).fold(0.0, f64::max),
            PathNode::Seg(s) => s.score,
            PathNode::Syn(s) => s.score,
        }
    }
}
