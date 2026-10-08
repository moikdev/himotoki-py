//! himotoki_rs — PyO3 bindings for the Rust himotoki port.
//!
//! Mirrors `himotoki/__init__.py`: `analyze(text, limit, max_length)` returns
//! `List[Tuple[List[dict], int]]` where each dict is `word_info_gloss_json`'s
//! output (same shape as `himotoki -j`).

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use himotoki_core::db;
use himotoki_core::index::WordIndex;
use himotoki_core::output::format::word_info_gloss_json;
use himotoki_core::output::golden::num_json;

/// Engine state: one SQLite connection + optional FST word index.
struct Engine {
    conn: rusqlite::Connection,
    index: Option<WordIndex>,
    path: PathBuf,
}

static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();

fn init_engine(db_path: Option<&str>) -> Engine {
    let path = db_path
        .map(PathBuf::from)
        .unwrap_or_else(db::default_db_path);
    let conn = db::open(&path).unwrap_or_else(|e| panic!("failed to open {}: {e}", path.display()));
    himotoki_core::grammar::suffixes::init_suffixes(&conn, false);
    let _ = himotoki_core::grammar::counters::init_counter_cache(&conn);
    let index = Some(WordIndex::load_or_build(&path, &conn));
    Engine { conn, index, path }
}

fn with_engine<T>(
    db_path: Option<&str>,
    f: impl FnOnce(&Engine) -> anyhow::Result<T>,
) -> PyResult<T> {
    let engine = ENGINE.get_or_init(|| Mutex::new(init_engine(db_path)));
    let eng = engine
        .lock()
        .map_err(|_| PyRuntimeError::new_err("himotoki engine lock poisoned"))?;
    if let Some(p) = db_path {
        if eng.path != *p {
            return Err(PyRuntimeError::new_err(format!(
                "engine already initialized with {}; cannot switch to {}",
                eng.path.display(),
                p
            )));
        }
    }
    f(&eng).map_err(|e| {
        let msg = e.to_string();
        match e.downcast_ref::<himotoki_core::TextTooLongError>() {
            Some(_) => PyValueError::new_err(msg),
            None => PyRuntimeError::new_err(msg),
        }
    })
}

/// `analyze` — NFC-normalize, segment, and fill word infos.
///
/// Returns `List[Tuple[List[dict], int]]` matching Python's
/// `himotoki.analyze` (dicts are `word_info_gloss_json` output).
#[pyfunction]
#[pyo3(signature = (text, limit=1, db_path=None, max_length=None))]
fn analyze(
    py: Python<'_>,
    text: &str,
    limit: usize,
    db_path: Option<&str>,
    max_length: Option<usize>,
) -> PyResult<PyObject> {
    if text.trim().is_empty() {
        return Err(PyValueError::new_err(
            "text must be non-empty and not whitespace-only",
        ));
    }
    if limit < 1 {
        return Err(PyValueError::new_err("limit must be >= 1"));
    }
    let text_owned = text.to_string();
    let results = py.allow_threads(move || {
        with_engine(db_path, |eng| {
            himotoki_core::analyze(
                &eng.conn,
                &text_owned,
                eng.index.as_ref(),
                limit,
                max_length,
            )
        })
    })?;

    // Now that we hold the GIL again, convert to Python objects.
    with_engine(db_path, |eng| {
        let out: Vec<(serde_json::Value, serde_json::Value)> = results
            .iter()
            .map(|(wis, score)| {
                (
                    serde_json::Value::Array(
                        wis.iter()
                            .map(|wi| word_info_gloss_json(&eng.conn, wi, false))
                            .collect(),
                    ),
                    num_json(*score),
                )
            })
            .collect();
        Ok(out)
    })
    .and_then(|v| {
        pythonize::pythonize(py, &v)
            .map(|o| o.unbind())
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    })
}

/// `warm_up` — eagerly initialize caches (suffix map, counter cache,
/// archaic-word set, FST index) so the first `analyze` isn't cold.
#[pyfunction]
#[pyo3(signature = (db_path=None))]
fn warm_up(db_path: Option<&str>) -> PyResult<()> {
    ENGINE.get_or_init(|| Mutex::new(init_engine(db_path)));
    Ok(())
}

/// Resolved database path (after env/default resolution).
#[pyfunction]
#[pyo3(signature = ())]
fn db_path() -> PyResult<String> {
    ENGINE
        .get()
        .map(|e| e.lock().unwrap().path.display().to_string())
        .ok_or_else(|| PyRuntimeError::new_err("engine not initialized"))
}

#[pymodule]
fn himotoki_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(analyze, m)?)?;
    m.add_function(wrap_pyfunction!(warm_up, m)?)?;
    m.add_function(wrap_pyfunction!(db_path, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
