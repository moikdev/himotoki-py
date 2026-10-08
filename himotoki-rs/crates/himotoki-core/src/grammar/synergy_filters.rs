//! Filter helpers — port of `himotoki/grammar/synergy_filters.rs`.
//!
//! Filters are `Arc<dyn Fn(&Segment) -> bool>` with per-segment result caching
//! keyed by a unique filter id (Python `cached_filter` + `segment._filter_cache`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use crate::types::{Segment, SegmentList};

pub type Filter = Arc<dyn Fn(&Segment) -> bool + Send + Sync>;
/// List-level filter (filter_short_kana takes a SegmentList).
pub type ListFilter = Arc<dyn Fn(&SegmentList) -> bool + Send + Sync>;

static FILTER_ID: AtomicU32 = AtomicU32::new(1);

fn next_filter_id() -> u32 {
    FILTER_ID.fetch_add(1, Ordering::Relaxed)
}

/// `cached_filter` — segment-level result cache keyed by filter id.
pub fn cached_filter(f: impl Fn(&Segment) -> bool + Send + Sync + 'static) -> Filter {
    let id = next_filter_id();
    Arc::new(move |seg: &Segment| {
        if let Some(v) = seg.filter_cache.borrow().get(&id) {
            return *v;
        }
        let r = f(seg);
        seg.filter_cache.borrow_mut().insert(id, r);
        r
    })
}

/// `filter_is_noun` (cached).
pub fn filter_is_noun() -> Filter {
    static F: LazyLock<Filter> = LazyLock::new(|| {
        cached_filter(|seg| {
            let kpcl = seg.info.kpcl;
            let (k, p, c, l) = (kpcl[0], kpcl[1], kpcl[2], kpcl[3]);
            const NOUN_POS: &[&str] = &["n", "n-adv", "n-t", "adj-na", "n-suf", "pn"];
            if (l || k || (p && c)) && seg.info.posi.iter().any(|p| NOUN_POS.contains(&p.as_str()))
            {
                return true;
            }
            // CounterText → check seq_set
            if seg.word.is_counter() {
                return !seg.info.seq_set.is_empty();
            }
            false
        })
    });
    F.clone()
}

/// `filter_is_pos(*pos)` (cached by pos-set).
pub fn filter_is_pos(pos: &[&'static str]) -> Filter {
    static CACHE: LazyLock<Mutex<HashMap<Vec<&'static str>, Filter>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut key = pos.to_vec();
    key.sort_unstable();
    if let Some(f) = CACHE.lock().unwrap().get(&key) {
        return f.clone();
    }
    let set: std::collections::HashSet<&'static str> = pos.iter().copied().collect();
    let f = cached_filter(move |seg| seg.info.posi.iter().any(|p| set.contains(p.as_str())));
    CACHE.lock().unwrap().insert(key, f.clone());
    f
}

/// `filter_in_seq_set(*seqs)` — seq_set intersection (cached).
pub fn filter_in_seq_set(seqs: &[i64]) -> Filter {
    static CACHE: LazyLock<Mutex<HashMap<Vec<i64>, Filter>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut key = seqs.to_vec();
    key.sort_unstable();
    if let Some(f) = CACHE.lock().unwrap().get(&key) {
        return f.clone();
    }
    let set: std::collections::HashSet<i64> = seqs.iter().copied().collect();
    let f = cached_filter(move |seg| !seg.info.seq_set.is_disjoint(&set));
    CACHE.lock().unwrap().insert(key, f.clone());
    f
}

/// `filter_in_seq_set_simple` — identical to filter_in_seq_set in Rust: the
/// Python `isinstance(seq, list)` guard can never fire (no word type exposes
/// a list seq).
pub fn filter_in_seq_set_simple(seqs: &[i64]) -> Filter {
    filter_in_seq_set(seqs)
}

/// `filter_is_conjugation(conj_type)` (cached).
pub fn filter_is_conjugation(conj_type: i64) -> Filter {
    static CACHE: LazyLock<Mutex<HashMap<i64, Filter>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    if let Some(f) = CACHE.lock().unwrap().get(&conj_type) {
        return f.clone();
    }
    let f = cached_filter(move |seg| {
        seg.info.conj.iter().any(|cd| {
            cd.prop
                .as_ref()
                .map(|p| p.conj_type == conj_type)
                .unwrap_or(false)
        })
    });
    CACHE.lock().unwrap().insert(conj_type, f.clone());
    f
}

/// `filter_is_compound_end(*seqs)` — DEAD in Python (CompoundWord.seq is an
/// int, never a list). Kept for parity; always false.
pub fn filter_is_compound_end(_seqs: &[i64]) -> Filter {
    cached_filter(|_seg| false)
}

/// `filter_is_compound_end_text(*texts)` — DEAD for the same reason.
pub fn filter_is_compound_end_text(_texts: &[&'static str]) -> Filter {
    cached_filter(|_seg| false)
}

/// `filter_short_kana(length, except_list)` — operates on a SegmentList.
pub fn filter_short_kana(length: usize, except: &[&'static str]) -> ListFilter {
    let except: std::collections::HashSet<&'static str> = except.iter().copied().collect();
    Arc::new(move |sl: &SegmentList| {
        let seg = match sl.segments.first() {
            Some(s) => s,
            None => return false,
        };
        if sl.end - sl.start > length {
            return false;
        }
        if seg.info.kpcl[0] {
            return false;
        }
        let text = seg.text();
        if except.contains(text) {
            return false;
        }
        true
    })
}
