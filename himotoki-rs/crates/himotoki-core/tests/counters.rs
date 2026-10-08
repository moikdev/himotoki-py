//! Counter lookup regression tests — `年` has two counter seqs (2084840 ねん,
//! 2220370 とせ); `find_counter_in_text` must yield both.

use himotoki_core::db;
use himotoki_core::grammar::counters::find_counter_in_text;

fn test_conn() -> rusqlite::Connection {
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../data/himotoki.db");
    db::open(&path).expect("open himotoki.db")
}

#[test]
fn nen_counter_yields_all_seqs() {
    let conn = test_conn();
    eprintln!(
        "init: {:?}",
        himotoki_core::grammar::counters::init_counter_cache(&conn)
    );
    let fc = himotoki_core::grammar::counters::find_counter(&conn, "24", "年");
    for c in &fc {
        eprintln!("  seq={:?} kana={}", c.seq(), c.kana);
    }
    let found = find_counter_in_text(&conn, "24年");
    eprintln!("find_counter_in_text -> {} results", found.len());
    let seqs: Vec<i64> = found
        .iter()
        .filter(|(s, e, _)| *s == 0 && *e == 3)
        .filter_map(|(_, _, c)| c.seq())
        .collect();
    assert!(
        seqs.contains(&2084840),
        "expected counter seq 2084840 in {seqs:?}"
    );
    assert!(
        seqs.contains(&2220370),
        "expected counter seq 2220370 in {seqs:?}"
    );
}
