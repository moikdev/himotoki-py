//! himotoki-core: Rust port of the himotoki Japanese morphological analyzer.
//!
//! Mirrors the Python package layout for cross-referencing during the port.

/// `lazy_set!` — defines a `pub static LazyLock<HashSet<T>>` for constant sets.
#[macro_export]
macro_rules! lazy_set {
    ($(pub static $name:ident : HashSet<$t:ty> = { $($v:expr),* $(,)? } )*) => {
        $(
            pub static $name: std::sync::LazyLock<std::collections::HashSet<$t>> =
                std::sync::LazyLock::new(|| {
                    let mut s = std::collections::HashSet::new();
                    $( s.insert($v); )*
                    s
                });
        )*
    };
}

pub mod cache;
pub mod chars;
pub mod conj;
pub mod constants;
pub mod db;
pub mod grammar;
pub mod hints;
pub mod index;
pub mod lookup;
pub mod output;
pub mod score;
pub mod segment;
pub mod types;

/// `himotoki/__init__.py` — MAX_TEXT_LENGTH, env `HIMOTOKI_MAX_TEXT_LENGTH`.
pub fn max_text_length() -> usize {
    std::env::var("HIMOTOKI_MAX_TEXT_LENGTH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
}

/// `himotoki/__init__.py` — TextTooLongError.
#[derive(Debug)]
pub struct TextTooLongError {
    pub len: usize,
    pub max: usize,
}

impl std::fmt::Display for TextTooLongError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "text length ({}) exceeds maximum allowed ({}). Split your text into smaller chunks.",
            self.len, self.max
        )
    }
}
impl std::error::Error for TextTooLongError {}

/// `himotoki/__init__.py:analyze` — NFC-normalize, length-check, segment, fill word infos.
///
/// Returns `(word_infos, score)` tuples sorted by score descending.
pub fn analyze(
    conn: &rusqlite::Connection,
    text: &str,
    index: Option<&index::WordIndex>,
    limit: usize,
    max_length: Option<usize>,
) -> anyhow::Result<Vec<(Vec<output::types::WordInfo>, f64)>> {
    use unicode_normalization::UnicodeNormalization;
    anyhow::ensure!(
        !text.trim().is_empty(),
        "text must be non-empty and not whitespace-only"
    );
    anyhow::ensure!(limit >= 1, "limit must be >= 1");
    let text: String = text.nfc().collect();
    let effective_max = max_length.unwrap_or_else(max_text_length);
    let len = text.chars().count();
    if len > effective_max {
        return Err(TextTooLongError {
            len,
            max: effective_max,
        }
        .into());
    }
    let results = segment::segment_text(conn, &text, index, limit);
    Ok(results
        .into_iter()
        .map(|(path, score)| {
            (
                output::word_info::fill_segment_path(conn, &text, &path, true),
                score,
            )
        })
        .collect())
}
