//! Row structs mirroring `himotoki/db/models.py` tables.
//!
//! Unlike the Python side there is no ORM/raw split — one struct per table.
//! Fields not selected by a given query are `Option`/`Default`.

#[derive(Debug, Clone, Default)]
pub struct EntryRow {
    pub seq: i64,
    pub root_p: bool,
    pub n_kanji: i64,
    pub n_kana: i64,
    pub primary_nokanji: bool,
}

/// `kanji_text` row. `ord` = reading order; `common` None = not common.
/// `common_tags`, `conjugate_p`, `nokanji`, `best_kana` are only populated
/// when the query selected them (hot-path selects omit `conjugate_p`/`common_tags`).
#[derive(Debug, Clone, Default)]
pub struct KanjiTextRow {
    pub id: i64,
    pub seq: i64,
    pub text: String,
    pub ord: i64,
    pub common: Option<i64>,
    pub common_tags: Option<String>,
    pub conjugate_p: Option<bool>,
    pub nokanji: Option<bool>,
    pub best_kana: Option<String>,
}

/// `kana_text` row.
#[derive(Debug, Clone, Default)]
pub struct KanaTextRow {
    pub id: i64,
    pub seq: i64,
    pub text: String,
    pub ord: i64,
    pub common: Option<i64>,
    pub common_tags: Option<String>,
    pub conjugate_p: Option<bool>,
    pub nokanji: Option<bool>,
    pub best_kanji: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SenseRow {
    pub id: i64,
    pub seq: i64,
    pub ord: i64,
}

#[derive(Debug, Clone, Default)]
pub struct GlossRow {
    pub id: i64,
    pub sense_id: i64,
    pub text: String,
    pub ord: i64,
}

#[derive(Debug, Clone, Default)]
pub struct SensePropRow {
    pub id: i64,
    pub sense_id: i64,
    pub seq: i64,
    pub tag: String,
    pub text: String,
    pub ord: i64,
}

#[derive(Debug, Clone, Default)]
pub struct RestrictedReadingRow {
    pub id: i64,
    pub seq: i64,
    pub reading: String,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct ConjugationRow {
    pub id: i64,
    pub seq: i64,
    /// Column name is `from` in SQL.
    pub from_seq: i64,
    pub via: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct ConjPropRow {
    pub id: i64,
    pub conj_id: i64,
    pub conj_type: i64,
    pub pos: String,
    pub neg: Option<bool>,
    pub fml: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct ConjSourceReadingRow {
    pub id: i64,
    pub conj_id: i64,
    pub text: String,
    pub source_text: String,
}
