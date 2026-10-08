//! Synergies / penalties / segfilters runtime — port of
//! `himotoki/grammar/synergies.py`. Registries are built once by
//! `synergy_rules::ensure_initialized()`.

use std::sync::{Arc, LazyLock};

use crate::types::{SegmentList, Synergy};

// ============================================================================
// Types
// ============================================================================

/// (new_right, synergy, new_left) — Python tuple order preserved.
pub type SynergyResult = Vec<(SegmentList, Synergy, SegmentList)>;
pub type SynergyFn = Arc<dyn Fn(&SegmentList, &SegmentList) -> SynergyResult + Send + Sync>;
pub type PenaltyFn = Arc<dyn Fn(&SegmentList, &SegmentList) -> Option<Synergy> + Send + Sync>;
/// segfilter returns (seg_left, seg_right) pairs.
pub type SegfilterFn = Arc<
    dyn Fn(Option<&SegmentList>, &SegmentList) -> Vec<(Option<SegmentList>, SegmentList)>
        + Send
        + Sync,
>;
pub type SegFilter = Arc<dyn Fn(&crate::types::Segment) -> bool + Send + Sync>;
pub type ListTest = Arc<dyn Fn(&SegmentList) -> bool + Send + Sync>;

pub(crate) struct Registries {
    pub synergies: Vec<SynergyFn>,
    pub penalties: Vec<PenaltyFn>,
    pub segfilters: Vec<SegfilterFn>,
}

static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| {
    let mut r = Registries {
        synergies: Vec::new(),
        penalties: Vec::new(),
        segfilters: Vec::new(),
    };
    crate::grammar::synergy_rules::init(&mut r);
    r
});

fn regs() -> &'static Registries {
    &REGISTRIES
}

pub fn get_segment_score_synergy(syn: &Synergy) -> f64 {
    syn.score
}

// ============================================================================
// Generic synergy builder
// ============================================================================

pub enum SynergyScore {
    Fixed(f64),
    Dyn(Arc<dyn Fn(&SegmentList, &SegmentList) -> f64 + Send + Sync>),
}

/// `def_generic_synergy`.
pub fn def_generic_synergy(
    filter_left: SegFilter,
    filter_right: SegFilter,
    description: &'static str,
    score: SynergyScore,
    connector: &'static str,
) -> SynergyFn {
    Arc::new(move |left: &SegmentList, right: &SegmentList| {
        if left.end != right.start {
            return Vec::new();
        }
        let left_segments: Vec<_> = left
            .segments
            .iter()
            .filter(|s| filter_left(s))
            .cloned()
            .collect();
        let right_segments: Vec<_> = right
            .segments
            .iter()
            .filter(|s| filter_right(s))
            .cloned()
            .collect();
        if left_segments.is_empty() || right_segments.is_empty() {
            return Vec::new();
        }
        let actual_score = match &score {
            SynergyScore::Fixed(v) => *v,
            SynergyScore::Dyn(f) => f(left, right),
        };
        let syn = Synergy {
            description: description.to_string(),
            connector: connector.to_string(),
            score: actual_score,
            start: left.end,
            end: right.start,
        };
        let new_left = SegmentList::new(left_segments, left.start, left.end, left.matches);
        let new_right = SegmentList::new(right_segments, right.start, right.end, right.matches);
        vec![(new_right, syn, new_left)]
    })
}

/// `get_synergies` — concatenated results from all registered synergies.
pub fn get_synergies(left: &SegmentList, right: &SegmentList) -> SynergyResult {
    let mut out = Vec::new();
    for f in &regs().synergies {
        out.extend(f(left, right));
    }
    out
}

// ============================================================================
// Penalties
// ============================================================================

/// `def_generic_penalty`.
pub fn def_generic_penalty(
    test_left: ListTest,
    test_right: ListTest,
    description: &'static str,
    score: f64,
    serial: bool,
    connector: &'static str,
) -> PenaltyFn {
    Arc::new(move |left: &SegmentList, right: &SegmentList| {
        if serial && left.end != right.start {
            return None;
        }
        if !test_left(left) || !test_right(right) {
            return None;
        }
        Some(Synergy {
            description: description.to_string(),
            connector: connector.to_string(),
            score,
            start: left.end,
            end: right.start,
        })
    })
}

/// `get_penalties` — first matching penalty wins; returns the node list
/// `[right, penalty, left]` or `[right, left]` when no penalty applies
/// (Python returns a plain list of objects).
pub fn get_penalties_nodes(
    left: &SegmentList,
    right: &SegmentList,
) -> Vec<std::rc::Rc<crate::types::PathNode>> {
    use crate::types::PathNode;
    use std::rc::Rc;
    for f in &regs().penalties {
        if let Some(pen) = f(left, right) {
            return vec![
                Rc::new(PathNode::List(Rc::new(right.clone()))),
                Rc::new(PathNode::Syn(Rc::new(pen))),
                Rc::new(PathNode::List(Rc::new(left.clone()))),
            ];
        }
    }
    vec![
        Rc::new(PathNode::List(Rc::new(right.clone()))),
        Rc::new(PathNode::List(Rc::new(left.clone()))),
    ]
}

// ============================================================================
// Segfilters
// ============================================================================

/// `def_segfilter_must_follow` — right-filtered segments must follow
/// left-filtered segments.
pub fn def_segfilter_must_follow(
    filter_left: SegFilter,
    filter_right: SegFilter,
    allow_first: bool,
) -> SegfilterFn {
    Arc::new(
        move |seg_left: Option<&SegmentList>, seg_right: &SegmentList| {
            let (mut satisfies_r, mut contradicts_r) = (Vec::new(), Vec::new());
            for s in &seg_right.segments {
                if filter_right(s) {
                    satisfies_r.push(s.clone());
                } else {
                    contradicts_r.push(s.clone());
                }
            }
            if satisfies_r.is_empty() {
                return vec![(seg_left.cloned(), seg_right.clone())];
            }
            if allow_first && seg_left.is_none() {
                return vec![(None, seg_right.clone())];
            }
            let left = match seg_left {
                Some(l) if l.end == seg_right.start => l,
                _ => {
                    // Not adjacent (or no left): only allow contradicts
                    if !contradicts_r.is_empty() {
                        return vec![(
                            seg_left.cloned(),
                            SegmentList::new(
                                contradicts_r,
                                seg_right.start,
                                seg_right.end,
                                seg_right.matches,
                            ),
                        )];
                    }
                    return Vec::new();
                }
            };
            let (mut satisfies_l, mut contradicts_l) = (Vec::new(), Vec::new());
            for s in &left.segments {
                if filter_left(s) {
                    satisfies_l.push(s.clone());
                } else {
                    contradicts_l.push(s.clone());
                }
            }
            let mut results = Vec::new();
            if !contradicts_l.is_empty() && !contradicts_r.is_empty() {
                results.push((
                    Some(left.clone()),
                    SegmentList::new(
                        contradicts_r.clone(),
                        seg_right.start,
                        seg_right.end,
                        seg_right.matches,
                    ),
                ));
            }
            if !satisfies_l.is_empty() {
                results.push((
                    Some(SegmentList::new(
                        satisfies_l,
                        left.start,
                        left.end,
                        left.matches,
                    )),
                    SegmentList::new(
                        satisfies_r,
                        seg_right.start,
                        seg_right.end,
                        seg_right.matches,
                    ),
                ));
            }
            if results.is_empty() && !contradicts_r.is_empty() {
                results.push((
                    Some(left.clone()),
                    SegmentList::new(
                        contradicts_r,
                        seg_right.start,
                        seg_right.end,
                        seg_right.matches,
                    ),
                ));
            }
            if results.is_empty() {
                vec![(Some(left.clone()), seg_right.clone())]
            } else {
                results
            }
        },
    )
}

/// `apply_segfilters` — fold all segfilters over (left, right) pair sets.
pub fn apply_segfilters(
    seg_left: Option<&SegmentList>,
    seg_right: &SegmentList,
) -> Vec<(Option<SegmentList>, SegmentList)> {
    let mut splits: Vec<(Option<SegmentList>, SegmentList)> =
        vec![(seg_left.cloned(), seg_right.clone())];
    for f in &regs().segfilters {
        let mut next = Vec::new();
        for (l, r) in &splits {
            next.extend(f(l.as_ref(), r));
        }
        splits = next;
    }
    splits
}
