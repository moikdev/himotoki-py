//! himotoki CLI — port of `himotoki/cli.py` + dev tooling.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use himotoki_core::chars::romanize_word;
use himotoki_core::db;
use himotoki_core::grammar::suffixes::init_suffixes;
use himotoki_core::index::WordIndex;
use himotoki_core::output::format::{segment_to_json, simple_segment};
use himotoki_core::output::golden;
use himotoki_core::segment::segment_text;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "himotoki",
    version = VERSION,
    about = "Command line interface for Himotoki (Japanese Morphological Analyzer)"
)]
struct Cli {
    /// Path to himotoki.db (else HIMOTOKI_DB_PATH / dev-tree default)
    #[arg(short = 'd', long = "database", global = true)]
    db: Option<PathBuf>,

    /// Skip the FST word index (query every substring)
    #[arg(long, global = true)]
    no_index: bool,

    /// Simple romanization output only
    #[arg(short = 'r', long, conflicts_with_all = ["full", "kana", "json"], group = "fmt")]
    romanize: bool,
    /// Full output with romanization and dictionary info
    #[arg(short = 'f', long, group = "fmt")]
    full: bool,
    /// Kana reading with spaces between words
    #[arg(short = 'k', long, group = "fmt")]
    kana: bool,
    /// Full split info as JSON
    #[arg(short = 'j', long, group = "fmt")]
    json: bool,
    /// Limit segmentations to N results (with -j)
    #[arg(short = 'l', long, default_value_t = 1)]
    limit: i32,

    /// Japanese text to analyze
    text: Vec<String>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Segment a single text and print the top paths (debug).
    Analyze {
        text: String,
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Time segment_text over an inputs.jsonl corpus (throughput bench).
    Bench {
        /// Path to inputs.jsonl ({"i","text"} lines)
        #[arg(long)]
        inputs: PathBuf,
        /// Max inputs to process
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Reproduce scripts/dump_gold.py output from golden inputs.jsonl.
    Dump {
        /// Path to inputs.jsonl ({"i","text"} lines)
        #[arg(long)]
        inputs: PathBuf,
        /// Output directory for candidates.jsonl / paths.jsonl
        #[arg(long)]
        out: PathBuf,
        /// Only one of: candidates | paths | output
        #[arg(long)]
        only: Option<String>,
        /// Process first N inputs only
        #[arg(long)]
        limit: Option<usize>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db.unwrap_or_else(db::default_db_path);
    let conn = db::open(&db_path)?;
    init_suffixes(&conn, false);
    let _ = himotoki_core::grammar::counters::init_counter_cache(&conn);
    let index = if cli.no_index {
        None
    } else {
        Some(WordIndex::load_or_build(&db_path, &conn))
    };

    match cli.cmd {
        Some(Cmd::Analyze { text, limit }) => {
            let paths = segment_text(&conn, &text, index.as_ref(), limit);
            for (path, score) in &paths {
                println!("score={}", score);
                for node in path {
                    println!(
                        "  {}",
                        serde_json::to_string(&golden::path_node_json(node))?
                    );
                }
            }
        }
        Some(Cmd::Bench { inputs, limit }) => {
            let f = BufReader::new(std::fs::File::open(inputs)?);
            let mut texts = Vec::new();
            for line in f.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(&line)?;
                texts.push(v["text"].as_str().unwrap_or("").to_string());
                if let Some(m) = limit {
                    if texts.len() >= m {
                        break;
                    }
                }
            }
            let t0 = std::time::Instant::now();
            let mut nsegs = 0usize;
            for t in &texts {
                let paths = segment_text(&conn, t, index.as_ref(), 5);
                nsegs += paths.len();
            }
            let el = t0.elapsed();
            eprintln!(
                "bench: {} inputs, {} paths, {:.3}s total, {:.1} ms/input",
                texts.len(),
                nsegs,
                el.as_secs_f64(),
                el.as_secs_f64() * 1000.0 / texts.len() as f64
            );
        }
        Some(Cmd::Dump {
            inputs,
            out,
            only,
            limit,
        }) => {
            dump(&conn, index.as_ref(), &inputs, &out, only.as_deref(), limit)?;
        }
        None => {
            let text = cli.text.join(" ");
            if text.trim().is_empty() {
                eprintln!("usage: himotoki [FLAGS] <text>");
                std::process::exit(1);
            }
            if cli.json {
                let limit = if cli.limit > 0 { cli.limit as usize } else { 5 };
                let output = segment_to_json(&conn, &text, limit);
                println!("{}", serde_json::to_string(&output)?);
            } else if cli.romanize {
                let word_infos = simple_segment(&conn, &text, 5);
                if word_infos.is_empty() {
                    println!("{}", text);
                } else {
                    let parts: Vec<String> = word_infos
                        .iter()
                        .map(|wi| romanize_word(wi.kana_str_or_text()))
                        .collect();
                    println!("{}", parts.join(" "));
                }
            } else if cli.kana {
                let word_infos = simple_segment(&conn, &text, 5);
                if word_infos.is_empty() {
                    println!("{}", text);
                } else {
                    let parts: Vec<&str> =
                        word_infos.iter().map(|wi| wi.kana_str_or_text()).collect();
                    println!("{}", parts.join(" "));
                }
            } else if cli.full {
                // Python `output_full`: dict_segment + format_word_info_text(romanization)
                let word_infos = simple_segment(&conn, &text, 5);
                if word_infos.is_empty() {
                    println!("{}", text);
                } else {
                    print!("{}", format_word_info_text(&conn, &word_infos, true));
                    println!();
                }
            } else {
                // default: format_word_info_text(include_romanization=False)
                let word_infos = simple_segment(&conn, &text, 5);
                if word_infos.is_empty() {
                    println!("{}", text);
                } else {
                    print!("{}", format_word_info_text(&conn, &word_infos, false));
                    println!();
                }
            }
        }
    }
    Ok(())
}

/// `format_word_info_text` — like segment_to_text body on given word_infos.
fn format_word_info_text(
    conn: &rusqlite::Connection,
    word_infos: &[himotoki_core::output::types::WordInfo],
    include_romanization: bool,
) -> String {
    use himotoki_core::output::meanings::{get_senses_str, word_info_reading_str};
    use himotoki_core::output::types::WordType;

    let mut lines: Vec<String> = Vec::new();
    if include_romanization {
        lines.push(
            word_infos
                .iter()
                .map(|wi| romanize_word(wi.kana_str_or_text()))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    for wi in word_infos {
        if wi.type_ == WordType::Gap {
            continue;
        }
        lines.push(String::new());
        if include_romanization {
            lines.push(format!(
                "* {}  {}",
                romanize_word(wi.kana_str_or_text()),
                word_info_reading_str(wi)
            ));
        } else {
            lines.push(format!("* {}", word_info_reading_str(wi)));
        }
        if wi.first_seq().is_some() {
            let mut senses = get_senses_str(conn, wi.first_seq().unwrap());
            if senses.is_empty() {
                // Conjugated entry with no own senses — try root entry
                if let Some(root_seq) =
                    himotoki_core::output::meanings::get_root_seq(conn, wi.first_seq().unwrap())
                {
                    senses = get_senses_str(conn, root_seq);
                }
            }
            if !senses.is_empty() {
                lines.push(senses);
            }
        }
        for cs in himotoki_core::output::conjugation_display::get_conjugation_display(conn, wi) {
            lines.push(cs);
        }
    }
    lines.join("\n")
}

fn dump(
    conn: &rusqlite::Connection,
    index: Option<&WordIndex>,
    inputs: &PathBuf,
    out: &PathBuf,
    only: Option<&str>,
    limit: Option<usize>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    let f = BufReader::new(std::fs::File::open(inputs)?);

    let mut cand_file = if only.is_none() || only == Some("candidates") {
        Some(std::io::BufWriter::new(std::fs::File::create(
            out.join("candidates.jsonl"),
        )?))
    } else {
        None
    };
    let mut path_file = if only.is_none() || only == Some("paths") {
        Some(std::io::BufWriter::new(std::fs::File::create(
            out.join("paths.jsonl"),
        )?))
    } else {
        None
    };
    let mut out_file = if only.is_none() || only == Some("output") {
        Some(std::io::BufWriter::new(std::fs::File::create(
            out.join("output.jsonl"),
        )?))
    } else {
        None
    };

    let mut n = 0usize;
    for line in f.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line)?;
        let i = v["i"].as_u64().unwrap_or(n as u64);
        let text = v["text"].as_str().unwrap_or("");

        if let Some(f) = cand_file.as_mut() {
            writeln!(f, "{}", golden::candidates_line(conn, index, i, text))?;
        }
        if let Some(f) = path_file.as_mut() {
            writeln!(f, "{}", golden::paths_line(conn, index, i, text))?;
        }
        if let Some(f) = out_file.as_mut() {
            writeln!(f, "{}", golden::output_line(conn, i, text))?;
        }

        n += 1;
        if n.is_multiple_of(100) {
            eprintln!("dump: {n}");
        }
        if let Some(m) = limit {
            if n >= m {
                break;
            }
        }
    }
    eprintln!("dump: done {n} inputs");
    Ok(())
}
