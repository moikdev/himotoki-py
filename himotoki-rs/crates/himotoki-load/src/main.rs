//! himotoki-load — build a himotoki SQLite database from JMdict XML.
//!
//! Equivalent to `python -m himotoki.setup` / `load_jmdict(..., load_extras=False)`.

use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "himotoki-load", version, about = "himotoki database builder")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Load JMdict XML into a fresh database (schema + entries/readings/senses).
    Jmdict {
        /// Path to JMdict XML (e.g. JMdict_e.xml)
        xml: PathBuf,
        /// Database path to create/overwrite
        #[arg(short, long)]
        db: PathBuf,
        /// 'skip' or 'overwrite' for existing seqs
        #[arg(long, default_value = "skip")]
        if_exists: String,
        /// Keep existing tables (don't drop/recreate schema)
        #[arg(long)]
        no_init: bool,
    },
    /// Generate conjugations (primary + secondary) into an existing DB.
    Conj {
        /// Database path
        #[arg(short, long)]
        db: PathBuf,
        /// Directory with kwpos.csv / conj.csv / conjo.csv
        #[arg(long, default_value = "../data")]
        data_dir: PathBuf,
        /// Skip the secondary pass
        #[arg(long)]
        no_secondary: bool,
    },
    /// Apply errata corrections to an existing DB (post-conjugations).
    Errata {
        /// Database path
        #[arg(short, long)]
        db: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Jmdict {
            xml,
            db,
            if_exists,
            no_init,
        } => {
            let mut conn = rusqlite::Connection::open(&db)?;
            conn.execute_batch(
                "PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA journal_mode=WAL;",
            )?;
            if !no_init {
                himotoki_load::create_schema(&conn)?;
            }
            let t0 = Instant::now();
            let n = himotoki_load::load_jmdict(&mut conn, &xml, &if_exists, 1000, |count| {
                eprintln!("  {count} entries...")
            })?;
            eprintln!("loaded {n} entries in {:?}", t0.elapsed());
            conn.execute_batch("ANALYZE")?;
            eprintln!("ANALYZE done");
        }
        Cmd::Conj {
            db,
            data_dir,
            no_secondary,
        } => {
            let mut conn = rusqlite::Connection::open(&db)?;
            conn.execute_batch(
                "PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA journal_mode=WAL;",
            )?;
            let t0 = Instant::now();
            let prog = |done: usize, total: usize| {
                if done == total || done.is_multiple_of(20000) {
                    eprintln!("  {done}/{total}");
                }
            };
            let (recs, new, reused) =
                himotoki_load::conjugations::load_conjugations(&mut conn, &data_dir, prog)?;
            eprintln!("primary: {recs} conj records, {new} new entries, {reused} existing reused");
            if !no_secondary {
                let (r2, n2, u2) = himotoki_load::conjugations::load_secondary_conjugations(
                    &mut conn, &data_dir, prog,
                )?;
                eprintln!("secondary: {r2} conj records, {n2} new entries, {u2} existing reused");
            }
            conn.execute_batch("ANALYZE")?;
            eprintln!("ANALYZE done in {:?}", t0.elapsed());
        }
        Cmd::Errata { db } => {
            let conn = rusqlite::Connection::open(&db)?;
            conn.execute_batch(
                "PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY; PRAGMA journal_mode=WAL;",
            )?;
            let t0 = Instant::now();
            himotoki_load::errata::add_errata(&conn)?;
            conn.execute_batch("ANALYZE")?;
            eprintln!("errata applied in {:?}", t0.elapsed());
        }
    }
    Ok(())
}
