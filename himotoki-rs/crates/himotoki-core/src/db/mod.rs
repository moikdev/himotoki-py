//! Database connection management — port of `himotoki/db/connection.py`.
//!
//! Path resolution priority:
//!   1. `HIMOTOKI_DB_PATH` / `HIMOTOKI_DB` env vars
//!   2. `~/.himotoki/himotoki.db`
//!   3. `<crate-root>/../data/himotoki.db` (dev tree)

pub mod rows;

use std::path::PathBuf;

use rusqlite::Connection;

/// Resolve the default database path, mirroring `_get_default_db_path`.
pub fn default_db_path() -> PathBuf {
    if let Ok(p) = std::env::var("HIMOTOKI_DB_PATH").or_else(|_| std::env::var("HIMOTOKI_DB")) {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return pb;
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let user_db = home.join(".himotoki").join("himotoki.db");
        if user_db.exists() {
            return user_db;
        }
    }
    // Dev-tree data dir: <repo>/data/himotoki.db (manifest is crates/himotoki-core)
    let dev_db = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../data/himotoki.db");
    if dev_db.exists() {
        return dev_db;
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".himotoki")
        .join("himotoki.db")
}

/// Open a connection with the same pragmas as Python
/// (db/connection.py `set_sqlite_pragma`).
pub fn open(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    apply_pragmas(&conn)?;
    Ok(conn)
}

pub fn apply_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "cache_size", "-64000")?;
    conn.pragma_update(None, "mmap_size", "268435456")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}
