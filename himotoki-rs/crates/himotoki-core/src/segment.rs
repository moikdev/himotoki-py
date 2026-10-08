//! Segmentation DP — port of `himotoki/segment.py`.
//!
//! join_substring_words → scored SegmentLists; find_best_path → top-N paths
//! of PathNode (List | Synergy) via per-segment TopArrays.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use rusqlite::Connection;

use crate::chars::{get_char_class, is_kana, sequential_kanji_positions, KANA_CHARS};
use crate::grammar::counters::find_counter_in_text;
use crate::grammar::suffixes::{could_have_suffix, find_word_suffix, is_suffix_cache_ready};
use crate::grammar::synergies::{apply_segfilters, get_penalties_nodes, get_synergies};
use crate::index::WordIndex;
use crate::score::{cull_segments, gap_penalty, gen_score, MAX_WORD_LENGTH, SCORE_CUTOFF};
use crate::types::{PathNode, Segment, SegmentList, Word};

// ============================================================================
// Constants
// ============================================================================

const MODIFIER_CLASSES: &[&str] = &[
    "+a",
    "+i",
    "+u",
    "+e",
    "+o",
    "+ya",
    "+yu",
    "+yo",
    "+wa",
    "long_vowel",
];
const ITERATION_CLASSES: &[&str] = &["iter", "iter_v"];

fn is_modifier_class(c: Option<&str>) -> bool {
    c.map(|c| MODIFIER_CLASSES.contains(&c)).unwrap_or(false)
}

fn is_iteration_class(c: Option<&str>) -> bool {
    c.map(|c| ITERATION_CLASSES.contains(&c)).unwrap_or(false)
}

fn is_kana_class(c: Option<&str>) -> bool {
    c.map(|c| KANA_CHARS.iter().any(|(n, _)| *n == c))
        .unwrap_or(false)
}

/// last char of the class' kana pair — Python `KANA_CHARS[char_class][-1]`.
fn kana_class_last(class: &str) -> Option<char> {
    KANA_CHARS
        .iter()
        .find(|(n, _)| *n == class)
        .and_then(|(_, s)| s.chars().last())
}

// ============================================================================
// TopArray — priority array for best paths
// ============================================================================

/// One entry in a TopArray: best path ending at a position.
#[derive(Debug, Clone)]
pub struct TopArrayItem {
    pub score: f64,
    /// The winning path (list of PathNode) for this position.
    pub payload: Vec<Rc<PathNode>>,
}

/// Fixed-size array keeping the top-N intermediate paths by score.
/// Mirrors `TopArray`: `array` slot list, `count` filled slots.
#[derive(Debug, Clone)]
pub struct TopArray {
    pub array: Vec<Option<TopArrayItem>>,
    pub count: usize,
    limit: usize,
}

impl TopArray {
    pub fn new(limit: usize) -> Self {
        TopArray {
            array: vec![None; limit],
            count: 0,
            limit,
        }
    }

    /// Faithful port of `TopArray.register` (segment.py:73): walk right-to-left
    /// from `min(count, limit)`, shifting lower-scored items right, write the
    /// new item when the left neighbor is empty or already >= score.
    /// Equal scores insert after existing equals (stable FIFO ties).
    pub fn register(&mut self, score: f64, payload: Vec<Rc<PathNode>>) {
        let item = Some(TopArrayItem { score, payload });
        let mut idx = self.count.min(self.limit) as i64;
        while idx >= 0 {
            let i = idx as usize;
            let prev_item = if i > 0 {
                self.array[i - 1].clone()
            } else {
                None
            };
            let done = prev_item.as_ref().map(|p| p.score >= score).unwrap_or(true);
            if i < self.limit {
                self.array[i] = if done { item.clone() } else { prev_item };
            }
            if done {
                break;
            }
            idx -= 1;
        }
        self.count += 1;
    }

    pub fn get_items(&self) -> impl Iterator<Item = &TopArrayItem> {
        self.array.iter().flatten()
    }
}

// ============================================================================
// Sticky positions
// ============================================================================

/// `is_long_vowel_modifier` — prev char's class ends in a vowel.
fn is_long_vowel_modifier(prev_char: char) -> bool {
    match get_char_class(prev_char) {
        Some(prev_class) => ["a", "i", "u", "e", "o"]
            .iter()
            .any(|v| prev_class.ends_with(v)),
        None => false,
    }
}

/// `find_sticky_positions` — positions where words can't start/end.
pub fn find_sticky_positions(text: &str) -> Vec<usize> {
    let chars: Vec<char> = text.chars().collect();
    let text_len = chars.len();
    let mut sticky = Vec::new();

    for pos in 0..text_len {
        let char = chars[pos];
        let char_class = get_char_class(char);

        if char_class == Some("sokuon") {
            if pos < text_len - 1 {
                let next_class = get_char_class(chars[pos + 1]);
                if is_kana_class(next_class) {
                    sticky.push(pos + 1);
                }
            }
        } else if char_class == Some("long_vowel") {
            if pos > 0 {
                sticky.push(pos);
            }
            let at_end = pos == text_len - 1;
            if !at_end {
                let prev_char = if pos > 0 { chars[pos - 1] } else { '\0' };
                if !(pos > 0 && is_long_vowel_modifier(prev_char)) {
                    sticky.push(pos);
                }
            }
        } else if is_modifier_class(char_class) || is_iteration_class(char_class) {
            let at_end = pos == text_len - 1;
            if !at_end {
                if pos > 0 && char_class == Some("long_vowel")
                    && is_long_vowel_modifier(chars[pos - 1]) {
                        continue;
                    }
                sticky.push(pos);
            }
        }
    }
    sticky
}

// ============================================================================
// Consecutive char groups
// ============================================================================

/// `consecutive_char_groups` — (start, end) char-offset ranges of
/// 'katakana' or 'number' runs.
pub fn consecutive_char_groups(char_type: &str, text: &str) -> Vec<(usize, usize)> {
    let mut groups = Vec::new();
    let mut current_start: Option<usize> = None;

    for (i, c) in text.chars().enumerate() {
        let is_type = match char_type {
            "katakana" => {
                // NB: 'ァ-ヺヽヾー' in Python is a substring membership —
                // the literal set {ァ, -, ヺ, ヽ, ヾ, ー} (ASCII '-' included).
                matches!(c, 'ァ' | '-' | 'ヺ' | 'ヽ' | 'ヾ' | 'ー') || {
                    match get_char_class(c) {
                        Some(cls) if is_kana_class(Some(cls)) => kana_class_last(cls) == Some(c),
                        _ => false,
                    }
                }
            }
            "number" => "0123456789０１２３４５６７８９〇一二三四五六七八九零壱弐参".contains(c),
            _ => false,
        };
        if is_type {
            if current_start.is_none() {
                current_start = Some(i);
            }
        } else if let Some(s) = current_start.take() {
            groups.push((s, i));
        }
    }
    if let Some(s) = current_start {
        groups.push((s, text.chars().count()));
    }
    groups
}

// ============================================================================
// Substring word matching
// ============================================================================

/// `find_substring_words` — batched IN-query over all in-trie substrings
/// plus suffix-compound matches for every substring.
pub fn find_substring_words(
    conn: &Connection,
    text: &str,
    sticky: &[usize],
    index: Option<&WordIndex>,
) -> HashMap<String, Vec<Word>> {
    let sticky_set: HashSet<usize> = sticky.iter().copied().collect();
    let mut substring_map: HashMap<String, Vec<Word>> = HashMap::new();
    let mut kana_keys: Vec<String> = Vec::new();
    let mut kanji_keys: Vec<String> = Vec::new();
    let mut all_substrings: Vec<String> = Vec::new();

    let prof = std::env::var("HIMOTOKI_PROFILE").is_ok();
    let t0 = std::time::Instant::now();
    let chars: Vec<char> = text.chars().collect();
    let text_len = chars.len();
    for start in 0..text_len {
        if sticky_set.contains(&start) {
            continue;
        }
        let max_end = text_len.min(start + MAX_WORD_LENGTH);
        for end in (start + 1)..=max_end {
            if sticky_set.contains(&end) {
                continue;
            }
            let part: String = chars[start..end].iter().collect();
            if substring_map.contains_key(&part) {
                continue;
            }
            all_substrings.push(part.clone());
            if index.map(|ix| ix.contains(&part)).unwrap_or(true) {
                substring_map.insert(part.clone(), Vec::new());
                if is_kana(&part) {
                    kana_keys.push(part);
                } else {
                    kanji_keys.push(part);
                }
            }
        }
    }
    let t_substr = t0.elapsed();

    // Batch queries
    let t1 = std::time::Instant::now();
    if !kana_keys.is_empty() {
        let unique: Vec<String> = kana_keys
            .iter()
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let ph = unique.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, seq, text, ord, common, best_kanji FROM kana_text WHERE text IN ({})",
            ph
        );
        if let Ok(mut stmt) = conn.prepare_cached(&sql) {
            let params: Vec<rusqlite::types::Value> =
                unique.iter().map(|s| s.clone().into()).collect();
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(params), |r| {
                Ok(crate::db::rows::KanaTextRow {
                    id: r.get(0)?,
                    seq: r.get(1)?,
                    text: r.get(2)?,
                    ord: r.get(3)?,
                    common: r.get(4)?,
                    best_kanji: r.get(5)?,
                    ..Default::default()
                })
            }) {
                for row in rows.flatten() {
                    substring_map
                        .entry(row.text.clone())
                        .or_default()
                        .push(Word::Simple(crate::types::WordMatch::new(
                            crate::types::Reading::Kana(row),
                        )));
                }
            }
        }
    }
    if !kanji_keys.is_empty() {
        let unique: Vec<String> = kanji_keys
            .iter()
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let ph = unique.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, seq, text, ord, common, best_kana FROM kanji_text WHERE text IN ({})",
            ph
        );
        if let Ok(mut stmt) = conn.prepare_cached(&sql) {
            let params: Vec<rusqlite::types::Value> =
                unique.iter().map(|s| s.clone().into()).collect();
            if let Ok(rows) = stmt.query_map(rusqlite::params_from_iter(params), |r| {
                Ok(crate::db::rows::KanjiTextRow {
                    id: r.get(0)?,
                    seq: r.get(1)?,
                    text: r.get(2)?,
                    ord: r.get(3)?,
                    common: r.get(4)?,
                    best_kana: r.get(5)?,
                    ..Default::default()
                })
            }) {
                for row in rows.flatten() {
                    substring_map
                        .entry(row.text.clone())
                        .or_default()
                        .push(Word::Simple(crate::types::WordMatch::new(
                            crate::types::Reading::Kanji(row),
                        )));
                }
            }
        }
    }

    let t_batch = t1.elapsed();

    // Suffix compounds for every substring (not in trie).
    let t2 = std::time::Instant::now();
    let mut n_suffix_checked = 0usize;
    if is_suffix_cache_ready() {
        for substring in &all_substrings {
            if !could_have_suffix(substring) {
                continue;
            }
            n_suffix_checked += 1;
            let suffix_results = find_word_suffix(conn, substring, &[], None, None, 0);
            if !suffix_results.is_empty() {
                substring_map
                    .entry(substring.clone())
                    .or_default()
                    .extend(suffix_results);
            }
        }
    }
    if prof {
        eprintln!(
            "  fsw: substr={:?}({}) batch={:?} suffix={:?}({} checked)",
            t_substr,
            all_substrings.len(),
            t_batch,
            t2.elapsed(),
            n_suffix_checked
        );
    }

    substring_map
}

/// `join_substring_words_impl` — positioned segments + kanji_break positions.
fn join_substring_words_impl(
    conn: &Connection,
    text: &str,
    index: Option<&WordIndex>,
) -> (Vec<(usize, usize, Vec<Segment>)>, Vec<usize>) {
    let sticky = find_sticky_positions(text);
    let substring_map = find_substring_words(conn, text, &sticky, index);
    let number_groups = consecutive_char_groups("number", text);
    // katakana_groups computed in Python but only feeds dead code; skipped.

    let counter_matches = find_counter_in_text(conn, text);
    let mut counter_map: HashMap<(usize, usize), Vec<crate::types::CounterText>> = HashMap::new();
    for (start, end, counter) in counter_matches {
        counter_map.entry((start, end)).or_default().push(counter);
    }

    let mut kanji_break: Vec<usize> = Vec::new();
    let mut ends: HashSet<usize> = HashSet::new();
    let mut results = Vec::new();

    let sticky_set: HashSet<usize> = sticky.iter().copied().collect();
    let chars: Vec<char> = text.chars().collect();
    let text_len = chars.len();

    for start in 0..text_len {
        if sticky_set.contains(&start) {
            continue;
        }
        let max_end = text_len.min(start + MAX_WORD_LENGTH);
        for end in (start + 1)..=max_end {
            if sticky_set.contains(&end) {
                continue;
            }
            let part: String = chars[start..end].iter().collect();
            let simple_words = substring_map.get(&part).cloned().unwrap_or_default();
            let all_words = simple_words;

            // number_groups/counter handling: counters keyed by (start,end).
            let counters = counter_map.get(&(start, end)).cloned().unwrap_or_default();

            let mut segments = Vec::new();
            for word in all_words {
                segments.push(Segment::new(start, end, word));
            }
            for counter in counters {
                let mut seg = Segment::new(start, end, Word::Counter(Box::new(counter)));
                seg.info.posi.insert("ctr".to_string());
                seg.info.counter = true;
                if let Some(seq) = seg.word.seq() {
                    seg.info.seq_set.insert(seq);
                }
                segments.push(seg);
            }

            if !segments.is_empty() {
                if start == 0 || ends.contains(&start) {
                    kanji_break.extend(sequential_kanji_positions(&part, start));
                }
                ends.insert(end);
                results.push((start, end, segments));
            }
        }
    }
    let _ = number_groups;
    let uniq_kb: Vec<usize> = {
        let s: HashSet<usize> = kanji_break.into_iter().collect();
        s.into_iter().collect()
    };
    (results, uniq_kb)
}

/// `join_substring_words` — scored, culled SegmentLists sorted by position.
pub fn join_substring_words(
    conn: &Connection,
    text: &str,
    index: Option<&WordIndex>,
) -> Vec<SegmentList> {
    let prof = std::env::var("HIMOTOKI_PROFILE").is_ok();
    let t0 = std::time::Instant::now();
    let (results, kanji_break) = join_substring_words_impl(conn, text, index);
    let t_impl = t0.elapsed();
    let mut segment_lists = Vec::new();

    let ends_with_long_vowel = text.ends_with('ー');
    let text_len = text.chars().count();

    let mut all_seqs: HashSet<i64> = HashSet::new();
    for (_, _, segments) in &results {
        for seg in segments {
            if let Some(seq) = seg.word.seq() {
                if seq != 0 {
                    all_seqs.insert(seq);
                }
            }
        }
    }
    if !all_seqs.is_empty() {
        crate::cache::preload_scoring_caches(conn, &all_seqs);
    }

    let t1 = std::time::Instant::now();
    let kb_set: HashSet<usize> = kanji_break.iter().copied().collect();
    for (start, end, segments) in results {
        let kb: Vec<usize> = [start, end]
            .iter()
            .filter(|n| kb_set.contains(n))
            .map(|n| n - start)
            .collect();

        let matches_n = segments.len();
        let mut scored = Vec::new();
        for mut segment in segments {
            let is_final = end == text_len || (ends_with_long_vowel && end == text_len - 1);
            gen_score(
                conn,
                &mut segment,
                is_final,
                if kb.is_empty() { None } else { Some(&kb) },
            );
            if segment.score >= SCORE_CUTOFF {
                scored.push(segment);
            }
        }
        if !scored.is_empty() {
            let culled = cull_segments(scored);
            // Reorder: suffix compounds first (they carry grammar chains),
            // then dict entries — iff both kinds are present.
            let (compound_segs, dict_segs): (Vec<_>, Vec<_>) =
                culled.into_iter().partition(|s| s.word.is_compound());
            let mut ordered = compound_segs;
            ordered.extend(dict_segs);
            segment_lists.push(SegmentList::from_owned(ordered, start, end, matches_n));
        }
    }
    if prof {
        eprintln!(
            "  join_breakdown: impl={:?} score_loop={:?}",
            t_impl,
            t1.elapsed()
        );
    }
    segment_lists
}

// Best-path DP
// ============================================================================

/// `get_segment_score`.
pub fn get_segment_score(node: &PathNode) -> f64 {
    node.score()
}

fn get_initial_segments(seg_list: &SegmentList) -> Vec<SegmentList> {
    apply_segfilters(None, seg_list)
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| !r.segments.is_empty())
        .collect()
}

/// `get_segment_splits` — synergy/penalty/segfilter-aware paths between
/// two adjacent positions. Returns Vec of node-paths ([right, syn?, left]).
fn get_segment_splits(seg_left: &SegmentList, seg_right: &SegmentList) -> Vec<Vec<Rc<PathNode>>> {
    let mut results: Vec<Vec<Rc<PathNode>>> = Vec::new();

    let filtered_pairs = apply_segfilters(Some(seg_left), seg_right);
    if filtered_pairs.is_empty() {
        return results;
    }

    for (new_left, new_right) in filtered_pairs {
        if new_right.segments.is_empty() {
            continue;
        }
        if let Some(nl) = &new_left {
            for (syn_right, synergy, syn_left) in get_synergies(nl, &new_right) {
                results.push(vec![
                    Rc::new(PathNode::List(Rc::new(syn_right))),
                    Rc::new(PathNode::Syn(Rc::new(synergy))),
                    Rc::new(PathNode::List(Rc::new(syn_left))),
                ]);
            }
            // Penalty path — Python always appends (dedup is identity-dead).
            results.push(get_penalties_nodes(nl, &new_right));
        } else {
            results.push(vec![Rc::new(PathNode::List(Rc::new(new_right)))]);
        }
    }
    results
}

/// `find_best_path` — DP over segment lists. Returns (path, score) pairs
/// with path reversed to left-to-right order.
pub fn find_best_path(
    segment_lists: &mut [SegmentList],
    text_length: usize,
    limit: usize,
) -> Vec<(Vec<Rc<PathNode>>, f64)> {
    let mut top = TopArray::new(limit);
    top.register(gap_penalty(0, text_length), Vec::new());

    for seg_list in segment_lists.iter_mut() {
        seg_list.top = Some(TopArray::new(limit));
    }

    let n = segment_lists.len();
    for i in 0..n {
        let (before, after) = segment_lists.split_at_mut(i + 1);
        let seg1 = &mut before[i];

        let gap_left = gap_penalty(0, seg1.start);
        let gap_right = gap_penalty(seg1.end, text_length);

        for seg in get_initial_segments(seg1) {
            let node = Rc::new(PathNode::List(Rc::new(seg)));
            let score1 = get_segment_score(&node);
            if let Some(t) = seg1.top.as_mut() {
                t.register(gap_left + score1, vec![node.clone()]);
            }
            top.register(gap_left + score1 + gap_right, vec![node]);
        }

        for seg2 in after.iter_mut() {
            let score2 = get_segment_score(&PathNode::List(Rc::new(seg2.clone())));
            if seg2.start < seg1.end {
                continue;
            }
            let gap_mid = gap_penalty(seg1.end, seg2.start);
            let gap_end = gap_penalty(seg2.end, text_length);

            let items: Vec<TopArrayItem> = match seg1.top.as_ref() {
                Some(t) => t.get_items().cloned().collect(),
                None => Vec::new(),
            };
            for tai in items {
                if tai.payload.is_empty() {
                    continue;
                }
                let seg_left_node = tai.payload[0].clone();
                let tail: Vec<Rc<PathNode>> = tai.payload.iter().skip(1).cloned().collect();
                let score3 = get_segment_score(&seg_left_node);
                let score_tail = tai.score - score3;

                let seg_left_list = match &*seg_left_node {
                    PathNode::List(l) => (**l).clone(),
                    _ => continue,
                };
                let splits = get_segment_splits(&seg_left_list, seg2);
                for split in splits {
                    let split_score: f64 = split.iter().map(|s| get_segment_score(s)).sum();
                    let accum =
                        gap_mid + split_score.max(score3 + 1.0).max(score2 + 1.0) + score_tail;
                    let mut new_path = split;
                    new_path.extend(tail.iter().cloned());

                    if let Some(t) = seg2.top.as_mut() {
                        t.register(accum, new_path.clone());
                    }
                    top.register(accum + gap_end, new_path);
                }
            }
        }
    }

    for seg_list in segment_lists.iter_mut() {
        seg_list.top = None;
    }

    top.get_items()
        .map(|tai| {
            let mut path = tai.payload.clone();
            path.reverse();
            (path, tai.score)
        })
        .collect()
}

/// `segment_text` — full pipeline entry.
pub fn segment_text(
    conn: &Connection,
    text: &str,
    index: Option<&WordIndex>,
    limit: usize,
) -> Vec<(Vec<Rc<PathNode>>, f64)> {
    if text.is_empty() {
        return Vec::new();
    }
    let prof = std::env::var("HIMOTOKI_PROFILE").is_ok();
    let t0 = std::time::Instant::now();
    let mut segment_lists = join_substring_words(conn, text, index);
    let t_js = t0.elapsed();
    if segment_lists.is_empty() {
        return Vec::new();
    }
    let t1 = std::time::Instant::now();
    let out = find_best_path(&mut segment_lists, text.chars().count(), limit);
    if prof {
        eprintln!(
            "PROF {:?}: join={:?} bestpath={:?}",
            text,
            t_js,
            t1.elapsed()
        );
    }
    out
}

/// `simple_segment` — best path's segments (flattened; Syn nodes skipped).
pub fn simple_segment(
    conn: &Connection,
    text: &str,
    index: Option<&WordIndex>,
) -> Vec<Rc<PathNode>> {
    segment_text(conn, text, index, 1)
        .into_iter()
        .next()
        .map(|(p, _)| p)
        .unwrap_or_default()
}
