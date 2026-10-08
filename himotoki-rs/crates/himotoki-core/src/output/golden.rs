//! Golden-corpus serialization — mirrors `scripts/dump_gold.py` field-for-field
//! so Rust output can be diffed against the Python fixtures.

use serde_json::{json, Value};

use crate::types::{Conj, PathNode, Segment, SegmentList, Word};

pub fn num_json(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        json!(v as i64)
    } else {
        json!(v)
    }
}

fn conj_json(c: &Conj) -> Value {
    match c {
        Conj::Unset => Value::Null,
        Conj::Root => Value::from("root"),
        Conj::Ids(ids) => json!(ids),
    }
}

/// `serialize_word`.
pub fn word_json(word: &Word) -> Value {
    match word {
        Word::Counter(c) => json!({
            "kind": "counter",
            "text": c.text,
            "kana": c.kana,
            "seq": c.seq(),
            "ord": c.ord(),
            "common": c.common(),
            "number_value": c.number_value,
            "number_text": c.number_text,
            "counter_text": c.counter_text,
            "counter_kana": c.counter_kana,
            "ordinalp": c.ordinalp,
            "suffix": c.suffix,
        }),
        Word::Compound(cw) => {
            // Python stores score_mod as scalar for first adjoin, list for nested.
            let score_mod = if cw.score_mod.len() == 1 {
                json!(cw.score_mod[0])
            } else {
                json!(cw.score_mod)
            };
            json!({
                "kind": "compound",
                "text": cw.text,
                "kana": cw.kana,
                "seq": cw.seq(),
                "ord": cw.ord(),
                "common": cw.common(),
                "word_type": cw.word_type(),
                "is_abbrev": cw.is_abbrev,
                "components": cw.components(),
                "words": cw.words.iter().map(|w| word_json(&Word::Simple(w.clone()))).collect::<Vec<_>>(),
                "score_mod": score_mod,
                "conjugations": conj_json(cw.conjugations().unwrap_or(&Conj::Unset)),
            })
        }
        Word::Simple(w) => {
            // PlaceholderReading: Python has seq=None and word_type falls to
            // 'kanji' via the hasattr(best_kanji) fallback.
            let placeholder = matches!(w.reading, crate::types::Reading::Placeholder(_));
            json!({
                "kind": "word",
                "seq": if placeholder { Value::Null } else { json!(w.seq()) },
                "text": w.text(),
                "word_type": if placeholder { "kanji" } else { w.word_type() },
                "ord": w.ord(),
                "common": w.common(),
                "is_root": w.is_root(),
                "conjugations": conj_json(&w.conjugations),
            })
        }
    }
}

/// `serialize_segment` — word dict + score + posi.
pub fn segment_json(seg: &Segment) -> Value {
    let mut d = word_json(&seg.word);
    let obj = d.as_object_mut().unwrap();
    obj.insert("score".to_string(), num_json(seg.score));
    let mut posi: Vec<&String> = seg.info.posi.iter().collect();
    posi.sort();
    obj.insert("posi".to_string(), json!(posi));
    d
}

/// `serialize_path_node`.
pub fn path_node_json(node: &PathNode) -> Value {
    match node {
        PathNode::Seg(s) => json!({
            "kind": "seg",
            "start": s.start,
            "end": s.end,
            "score": num_json(s.score),
            "word": word_json(&s.word),
        }),
        PathNode::List(l) => json!({
            "kind": "seglist",
            "start": l.start,
            "end": l.end,
            "segs": l.segments.iter().map(segment_json).collect::<Vec<_>>(),
        }),
        PathNode::Syn(s) => json!({
            "kind": "syn",
            "start": s.start,
            "end": s.end,
            "score": num_json(s.score),
            "desc": s.description,
        }),
    }
}

/// candidates.jsonl record for one input.
pub fn candidates_record(i: usize, text: &str, lists: &[SegmentList]) -> Value {
    json!({
        "i": i,
        "text": text,
        "lists": lists.iter().map(|sl| json!({
            "start": sl.start,
            "end": sl.end,
            "matches": sl.matches,
            "segs": sl.segments.iter().map(segment_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// paths.jsonl record for one input.
pub fn paths_record(i: usize, text: &str, paths: &[(Vec<std::rc::Rc<PathNode>>, f64)]) -> Value {
    json!({
        "i": i,
        "text": text,
        "paths": paths.iter().map(|(p, sc)| json!({
            "score": num_json(*sc),
            "nodes": p.iter().map(|n| path_node_json(n)).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

// ============================================================================
// jsonl line helpers — one record per input, serialized compactly
// ============================================================================

/// candidates.jsonl line for input `i`.
pub fn candidates_line(
    conn: &rusqlite::Connection,
    index: Option<&crate::index::WordIndex>,
    i: u64,
    text: &str,
) -> String {
    let lists = crate::segment::join_substring_words(conn, text, index);
    serde_json::to_string(&candidates_record(i as usize, text, &lists))
        .unwrap_or_else(|e| format!("{{\"i\":{},\"error\":\"{}\"}}", i, e))
}

/// paths.jsonl line for input `i`.
pub fn paths_line(
    conn: &rusqlite::Connection,
    index: Option<&crate::index::WordIndex>,
    i: u64,
    text: &str,
) -> String {
    let paths = crate::segment::segment_text(conn, text, index, 5);
    serde_json::to_string(&paths_record(i as usize, text, &paths))
        .unwrap_or_else(|e| format!("{{\"i\":{},\"error\":\"{}\"}}", i, e))
}

/// output.jsonl line for input `i` (segment_to_json result).
pub fn output_line(conn: &rusqlite::Connection, i: u64, text: &str) -> String {
    let js = crate::output::format::segment_to_json(conn, text, 5);
    serde_json::to_string(&json!({"i": i, "text": text, "json": js}))
        .unwrap_or_else(|e| format!("{{\"i\":{},\"error\":\"{}\"}}", i, e))
}
