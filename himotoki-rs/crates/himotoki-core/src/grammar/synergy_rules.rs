//! Synergy/penalty/segfilter rule registrations — port of
//! `himotoki/grammar/synergy_rules.py`. Order of registration preserved.

use std::sync::Arc;

use crate::constants::*;
use crate::types::{Segment, SegmentList, Synergy};

use super::synergies::{
    def_generic_penalty, def_generic_synergy, def_segfilter_must_follow, ListTest, PenaltyFn,
    SegFilter, SegfilterFn, SynergyScore,
};
use super::synergy_filters::{
    filter_in_seq_set, filter_is_conjugation, filter_is_noun, filter_is_pos, filter_short_kana,
};
use std::rc::Rc;

type SynRes = Vec<(SegmentList, Synergy, SegmentList)>;

fn mk_list(segs: Vec<Rc<Segment>>, src: &SegmentList) -> SegmentList {
    SegmentList::new(segs, src.start, src.end, src.matches)
}

/// Shared constructor for the hand-written synergy fns:
/// adjacency check → filter both sides → one (right, syn, left) triple.
fn make_syn(
    left: &SegmentList,
    right: &SegmentList,
    lfilter: SegFilter,
    rfilter: SegFilter,
    description: &'static str,
    connector: &'static str,
    score: f64,
) -> SynRes {
    if left.end != right.start {
        return Vec::new();
    }
    let ls: Vec<Rc<Segment>> = left
        .segments
        .iter()
        .filter(|s| lfilter(s))
        .cloned()
        .collect();
    if ls.is_empty() {
        return Vec::new();
    }
    let rs: Vec<Rc<Segment>> = right
        .segments
        .iter()
        .filter(|s| rfilter(s))
        .cloned()
        .collect();
    if rs.is_empty() {
        return Vec::new();
    }
    let syn = Synergy {
        description: description.to_string(),
        connector: connector.to_string(),
        score,
        start: left.end,
        end: right.start,
    };
    vec![(mk_list(rs, right), syn, mk_list(ls, left))]
}

const VERB_POS: &[&str] = &[
    "v1", "v5r", "v5s", "v5k", "v5g", "v5b", "v5m", "v5n", "v5t", "v5u", "vk", "vs", "vs-i",
];

const VERB_POS_WIDE: &[&str] = &[
    "v1", "v5r", "v5s", "v5k", "v5g", "v5b", "v5m", "v5n", "v5t", "v5u", "v5u-s", "v5k-s", "v5r-i",
    "vk", "vs", "vs-i", "adj-i", "adj-ix",
];

/// neg-prop check used by shika+neg etc.
fn seg_is_neg(seg: &Segment) -> bool {
    seg.info
        .conj
        .iter()
        .any(|cd| cd.prop.as_ref().and_then(|p| p.neg).unwrap_or(false))
}

fn seg_has_conj_type(seg: &Segment, conj_type: i64) -> bool {
    seg.info.conj.iter().any(|cd| {
        cd.prop
            .as_ref()
            .map(|p| p.conj_type == conj_type)
            .unwrap_or(false)
    })
}

/// `is_conjugated` — any ConjData with a prop (Python checks
/// `conj_type is not None`; prop rows always carry a conj_type).
fn seg_is_conjugated(seg: &Segment) -> bool {
    seg.info.conj.iter().any(|cd| cd.prop.is_some())
}

fn has_pos(seg: &Segment, pos: &[&'static str]) -> bool {
    seg.info.posi.iter().any(|p| pos.contains(&p.as_str()))
}

/// `has_seq_simple` — list-level seq test (penalty helper).
fn has_seq_simple(seqs: &[i64]) -> ListTest {
    let set: std::collections::HashSet<i64> = seqs.iter().copied().collect();
    Arc::new(move |sl: &SegmentList| {
        sl.segments
            .iter()
            .any(|s| !s.info.seq_set.is_disjoint(&set))
    })
}

/// Generic "block seq-pair" segfilter body shared by ni_tsuke/mada_ni/to_mo/
/// neg_imperative_n pattern: if left-matching AND right-matching segs exist,
/// return (left_other × right) ∪ (left × right_other); else pass through.
fn block_pair(
    seg_left: Option<&SegmentList>,
    seg_right: &SegmentList,
    left_hit: impl Fn(&Segment) -> bool,
    right_hit: impl Fn(&Segment) -> bool,
) -> Vec<(Option<SegmentList>, SegmentList)> {
    let left = match seg_left {
        Some(l) if l.end == seg_right.start => l,
        _ => return vec![(seg_left.cloned(), seg_right.clone())],
    };
    let left_hit_segs: Vec<Rc<Segment>> = left
        .segments
        .iter()
        .filter(|s| left_hit(s))
        .cloned()
        .collect();
    let left_other: Vec<Rc<Segment>> = left
        .segments
        .iter()
        .filter(|s| !left_hit(s))
        .cloned()
        .collect();
    let right_hit_segs: Vec<Rc<Segment>> = seg_right
        .segments
        .iter()
        .filter(|s| right_hit(s))
        .cloned()
        .collect();
    let right_other: Vec<Rc<Segment>> = seg_right
        .segments
        .iter()
        .filter(|s| !right_hit(s))
        .cloned()
        .collect();

    if left_hit_segs.is_empty() || right_hit_segs.is_empty() {
        return vec![(Some(left.clone()), seg_right.clone())];
    }
    let mut results = Vec::new();
    if !left_other.is_empty() {
        results.push((Some(mk_list(left_other, left)), seg_right.clone()));
    }
    if !right_other.is_empty() {
        results.push((Some(left.clone()), mk_list(right_other, seg_right)));
    }
    results
}

pub(crate) fn init(r: &mut super::synergies::Registries) {
    init_synergies(r);
    init_penalties(r);
    init_segfilters(r);
}

// ============================================================================
// Synergies
// ============================================================================

fn init_synergies(r: &mut super::synergies::Registries) {
    // noun + particle (with と+は exclusion)
    {
        let f_noun = filter_is_noun();
        let f_part = filter_in_seq_set(&NOUN_PARTICLES.iter().copied().collect::<Vec<_>>());
        r.synergies
            .push(Arc::new(move |left: &SegmentList, right: &SegmentList| {
                if left.end != right.start {
                    return Vec::new();
                }
                let ls: Vec<Rc<Segment>> = left
                    .segments
                    .iter()
                    .filter(|s| f_noun(s))
                    .cloned()
                    .collect();
                if ls.is_empty() {
                    return Vec::new();
                }
                let rs: Vec<Rc<Segment>> = right
                    .segments
                    .iter()
                    .filter(|s| f_part(s))
                    .cloned()
                    .collect();
                if rs.is_empty() {
                    return Vec::new();
                }
                // block と + は
                let left_has_to = left
                    .segments
                    .iter()
                    .any(|s| s.info.seq_set.contains(&SEQ_TO));
                let right_has_wa = right
                    .segments
                    .iter()
                    .any(|s| s.info.seq_set.contains(&SEQ_WA));
                if left_has_to && right_has_wa {
                    return Vec::new();
                }
                let length = right.end - right.start;
                let syn = Synergy {
                    description: "noun+prt".to_string(),
                    connector: " ".to_string(),
                    score: 10.0 + 4.0 * length as f64,
                    start: left.end,
                    end: right.start,
                };
                // Python returns the ORIGINAL lists here (not narrowed).
                vec![(right.clone(), syn, left.clone())]
            }));
    }

    // noun + だ
    r.synergies.push(def_generic_synergy(
        filter_is_noun(),
        filter_in_seq_set(&[2089020]),
        "noun+da",
        SynergyScore::Fixed(10.0),
        " ",
    ));

    // の/ん + だ/だった/だろう
    r.synergies.push(def_generic_synergy(
        filter_in_seq_set(&[1469800, 2139720]),
        filter_in_seq_set(&[2089020, 1007370, 1928670]),
        "no da/desu",
        SynergyScore::Fixed(15.0),
        " ",
    ));

    // そう + なんだ
    r.synergies.push(def_generic_synergy(
        filter_in_seq_set(&[2137720]),
        filter_in_seq_set(&[2140410]),
        "sou na n da",
        SynergyScore::Fixed(50.0),
        " ",
    ));

    // adj-no + の
    r.synergies.push(def_generic_synergy(
        filter_is_pos(&["adj-no"]),
        filter_in_seq_set(&[1469800]),
        "no-adjective",
        SynergyScore::Fixed(15.0),
        " ",
    ));

    // adj-na + な/に
    r.synergies.push(def_generic_synergy(
        filter_is_pos(&["adj-na"]),
        filter_in_seq_set(&[2029110, 2028990]),
        "na-adjective",
        SynergyScore::Fixed(15.0),
        " ",
    ));

    // adv-to + と (dynamic score 10 + 10*left_len)
    r.synergies.push(def_generic_synergy(
        filter_is_pos(&["adv-to"]),
        filter_in_seq_set(&[1008490]),
        "to-adverb",
        SynergyScore::Dyn(Arc::new(|l: &SegmentList, _r: &SegmentList| {
            10.0 + 10.0 * (l.end - l.start) as f64
        })),
        " ",
    ));

    // noun + 中
    r.synergies.push(def_generic_synergy(
        filter_is_noun(),
        filter_in_seq_set(&[1620400, 2083570]),
        "suffix-chu",
        SynergyScore::Fixed(12.0),
        "-",
    ));

    // noun + たち (10 + 5*left_len)
    r.synergies.push(def_generic_synergy(
        filter_is_noun(),
        filter_in_seq_set(&[1416220]),
        "suffix-tachi",
        SynergyScore::Dyn(Arc::new(|l: &SegmentList, _r: &SegmentList| {
            10.0 + 5.0 * (l.end - l.start) as f64
        })),
        "-",
    ));

    // noun + ぶり
    r.synergies.push(def_generic_synergy(
        filter_is_noun(),
        filter_in_seq_set(&[1361140]),
        "suffix-buri",
        SynergyScore::Fixed(40.0),
        "",
    ));

    // noun + 性
    r.synergies.push(def_generic_synergy(
        filter_is_noun(),
        filter_in_seq_set(&[1375260]),
        "suffix-sei",
        SynergyScore::Fixed(12.0),
        "",
    ));

    // お + noun (excluding みの seqs)
    {
        let f_pos = filter_is_pos(&["n"]);
        let f_o = filter_in_seq_set(&[1270190]);
        let f_right: SegFilter = Arc::new(move |s: &Segment| {
            if !f_pos(s) {
                return false;
            }
            !s.info
                .seq_set
                .iter()
                .any(|sq| *sq == 1634010 || *sq == 2845080)
        });
        r.synergies.push(def_generic_synergy(
            f_o,
            f_right,
            "o+noun",
            SynergyScore::Fixed(10.0),
            "",
        ));
    }

    // 未/不 + noun
    r.synergies.push(def_generic_synergy(
        filter_in_seq_set(&[2242840, 1922780, 2423740]),
        filter_is_pos(&["n"]),
        "kanji prefix+noun",
        SynergyScore::Fixed(15.0),
        "",
    ));

    // しちゃ/しては + いけない (compound_end is dead in Python → left never
    // satisfies, so this synergy never fires; keep for parity)
    r.synergies.push(def_generic_synergy(
        super::synergy_filters::filter_is_compound_end(&[2028920]),
        filter_in_seq_set(&[1000730, 1612750, 1409110, 2829697, 1587610]),
        "shicha ikenai",
        SynergyScore::Fixed(50.0),
        " ",
    ));

    // しか + negative
    {
        let f_shika = filter_in_seq_set(&[1005460]);
        let f_neg: SegFilter = Arc::new(seg_is_neg);
        r.synergies
            .push(Arc::new(move |l: &SegmentList, rr: &SegmentList| {
                make_syn(
                    l,
                    rr,
                    f_shika.clone(),
                    f_neg.clone(),
                    "shika+neg",
                    " ",
                    50.0,
                )
            }));
    }

    // の + 通り
    r.synergies.push(def_generic_synergy(
        filter_in_seq_set(&[1469800]),
        filter_in_seq_set(&[1432920]),
        "no toori",
        SynergyScore::Fixed(50.0),
        " ",
    ));

    // counter + おき
    r.synergies.push(def_generic_synergy(
        filter_is_pos(&["ctr"]),
        filter_in_seq_set(&[2854117, 2084550]),
        "counter+oki",
        SynergyScore::Fixed(20.0),
        "",
    ));

    // かどうか + は
    r.synergies.push(def_generic_synergy(
        filter_in_seq_set(&[2087300]),
        filter_in_seq_set(&[2028920]),
        "kadouka+wa",
        SynergyScore::Fixed(30.0),
        " ",
    ));

    // particle + common adverb
    {
        const COMMON_ADVERB: &[i64] = &[
            1527110, 1010180, 1623080, 2084660, 1303920, 1008930, 1010210, 1008550, 1004920,
            1530760, 1340610, 1008680, 1391950, 1391700, 1583020, 1311820, 1208880, 1397270,
            1399930, 1252670, 2423580, 1541310, 1273140, 1216780, 1623560, 1320780, 1544660,
            1010990,
        ];
        const PARTICLES: &[i64] = &[
            2028920, 2028930, 2028940, 2028990, 2028980, 2215430, 2028950, 1007340, 1525680,
            1002980,
        ];
        r.synergies.push(def_generic_synergy(
            filter_in_seq_set(PARTICLES),
            filter_in_seq_set(COMMON_ADVERB),
            "particle+adverb",
            SynergyScore::Fixed(20.0),
            " ",
        ));
    }

    // volitional + とも
    {
        let f_vol: SegFilter = Arc::new(|s: &Segment| seg_has_conj_type(s, 9));
        let f_tomo = filter_in_seq_set(&[1632180]);
        r.synergies
            .push(Arc::new(move |l: &SegmentList, rr: &SegmentList| {
                make_syn(
                    l,
                    rr,
                    f_vol.clone(),
                    f_tomo.clone(),
                    "volitional+tomo",
                    " ",
                    50.0,
                )
            }));
    }

    // verb + なんて
    {
        let f_verb: SegFilter = Arc::new(|s: &Segment| {
            has_pos(s, VERB_POS) && !(seg_has_conj_type(s, 10) && seg_is_neg(s))
        });
        let f_nante = filter_in_seq_set(&[1188370]);
        r.synergies
            .push(Arc::new(move |l: &SegmentList, rr: &SegmentList| {
                make_syn(
                    l,
                    rr,
                    f_verb.clone(),
                    f_nante.clone(),
                    "verb+nante",
                    " ",
                    60.0,
                )
            }));
    }

    // し (particle) + ただ
    r.synergies
        .push(Arc::new(|l: &SegmentList, rr: &SegmentList| {
            make_syn(
                l,
                rr,
                filter_in_seq_set(&[2086640]),
                filter_in_seq_set(&[1538900]),
                "shi+tada",
                " ",
                10.0,
            )
        }));

    // verb + よ (unconjugated only)
    {
        let f_verb: SegFilter = Arc::new(|s: &Segment| has_pos(s, VERB_POS));
        let f_yo: SegFilter = {
            let f_seq = filter_in_seq_set(&[2029090]);
            Arc::new(move |s: &Segment| f_seq(s) && !seg_is_conjugated(s))
        };
        r.synergies
            .push(Arc::new(move |l: &SegmentList, rr: &SegmentList| {
                make_syn(l, rr, f_verb.clone(), f_yo.clone(), "verb+yo", " ", 30.0)
            }));
    }

    // copula + よ
    {
        let f_cop = filter_in_seq_set(&[1628500, 2089020]);
        let f_yo: SegFilter = {
            let f_seq = filter_in_seq_set(&[2029090]);
            Arc::new(move |s: &Segment| f_seq(s) && !seg_is_conjugated(s))
        };
        r.synergies
            .push(Arc::new(move |l: &SegmentList, rr: &SegmentList| {
                make_syn(l, rr, f_cop.clone(), f_yo.clone(), "copula+yo", " ", 40.0)
            }));
    }

    // 前(まえ) + に
    r.synergies
        .push(Arc::new(|l: &SegmentList, rr: &SegmentList| {
            make_syn(
                l,
                rr,
                filter_in_seq_set(&[SEQ_MAE_NOUN]),
                filter_in_seq_set(&[SEQ_NI]),
                "mae+ni",
                " ",
                25.0,
            )
        }));

    // の + 方(ほう)
    r.synergies
        .push(Arc::new(|l: &SegmentList, rr: &SegmentList| {
            make_syn(
                l,
                rr,
                filter_in_seq_set(&[SEQ_NO]),
                filter_in_seq_set(&[SEQ_HOU_NOUN]),
                "no+hou",
                " ",
                25.0,
            )
        }));

    // noun + 面(めん)
    r.synergies
        .push(Arc::new(|l: &SegmentList, rr: &SegmentList| {
            make_syn(
                l,
                rr,
                filter_is_noun(),
                filter_in_seq_set(&[SEQ_MEN_NOUN]),
                "noun+men",
                " ",
                25.0,
            )
        }));

    // verb/adj + 人(ひと)
    {
        let f_verb = Arc::new(|s: &Segment| has_pos(s, VERB_POS_WIDE)) as SegFilter;
        r.synergies
            .push(Arc::new(move |l: &SegmentList, rr: &SegmentList| {
                make_syn(
                    l,
                    rr,
                    f_verb.clone(),
                    filter_in_seq_set(&[SEQ_HITO_NOUN]),
                    "verb+hito",
                    " ",
                    25.0,
                )
            }));
    }

    // 人(ひと) + の
    r.synergies
        .push(Arc::new(|l: &SegmentList, rr: &SegmentList| {
            make_syn(
                l,
                rr,
                filter_in_seq_set(&[SEQ_HITO_NOUN]),
                filter_in_seq_set(&[SEQ_NO]),
                "hito+no",
                " ",
                30.0,
            )
        }));

    // verb + 中(なか)
    {
        const VERB_POS_NAKA: &[&str] = &[
            "v1", "v5r", "v5s", "v5k", "v5g", "v5b", "v5m", "v5n", "v5t", "v5u", "v5u-s", "v5k-s",
            "v5r-i", "vk", "vs", "vs-i",
        ];
        let f_verb = Arc::new(|s: &Segment| has_pos(s, VERB_POS_NAKA)) as SegFilter;
        r.synergies
            .push(Arc::new(move |l: &SegmentList, rr: &SegmentList| {
                make_syn(
                    l,
                    rr,
                    f_verb.clone(),
                    filter_in_seq_set(&[SEQ_NAKA_NOUN]),
                    "verb+naka",
                    " ",
                    25.0,
                )
            }));
    }

    // が + 止まる(とまる)
    r.synergies
        .push(Arc::new(|l: &SegmentList, rr: &SegmentList| {
            make_syn(
                l,
                rr,
                filter_in_seq_set(&[SEQ_GA]),
                filter_in_seq_set(&[SEQ_TOMARU]),
                "ga+tomaru",
                " ",
                25.0,
            )
        }));

    // は + 辛い(つらい)
    r.synergies
        .push(Arc::new(|l: &SegmentList, rr: &SegmentList| {
            make_syn(
                l,
                rr,
                filter_in_seq_set(&[SEQ_WA]),
                filter_in_seq_set(&[SEQ_TSURAI]),
                "wa+tsurai",
                " ",
                50.0,
            )
        }));
}

// ============================================================================
// Penalties
// ============================================================================

fn init_penalties(r: &mut super::synergies::Registries) {
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[1008490]),
        has_seq_simple(&[2028920]),
        "to+wa-penalty",
        -20.0,
        true,
        " ",
    ));
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[2028990]),
        has_seq_simple(&[10351890, 1434020, 10097136, 1559290, 1434120, 10351981]),
        "ni+tsure-penalty",
        -30.0,
        true,
        " ",
    ));
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[
            1343610, 1485770, 2089690, 2268350, 2603520, 2742870, 2826528,
        ]),
        has_seq_simple(&[1210900, 10074000, 1365980, 1365990]),
        "o+susume-penalty",
        -40.0,
        true,
        " ",
    ));
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[10043332, 2250200]),
        has_seq_simple(&[1408160, 1416790, 2029050, 11435516, 11679733]),
        "hitogai+tara-penalty",
        -75.0,
        true,
        " ",
    ));
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[1270190]),
        has_seq_simple(&[1634010, 2845080]),
        "go+mino-penalty",
        -15.0,
        true,
        " ",
    ));
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[10256789, 10256836]),
        has_seq_simple(&[10452328, 1529520]),
        "wakan+nai-penalty",
        -30.0,
        true,
        " ",
    ));
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[10350776]),
        has_seq_simple(&[1004200]),
        "shiran+kedo-penalty",
        -100.0,
        true,
        " ",
    ));
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[1490710, 2084350]),
        has_seq_simple(&[2089020]),
        "hama+da-penalty",
        -20.0,
        true,
        " ",
    ));

    // single-kanji + 人たち penalty
    r.penalties
        .push(Arc::new(|left: &SegmentList, right: &SegmentList| {
            if left.end != right.start {
                return None;
            }
            let has_hitotachi = right
                .segments
                .iter()
                .any(|s| s.info.seq_set.contains(&1368740));
            if !has_hitotachi {
                return None;
            }
            for seg in &left.segments {
                let text = seg.word.text();
                if text.chars().count() == 1 && crate::chars::is_kanji(text) {
                    return Some(Synergy {
                        description: "single-kanji+hitotachi-penalty".to_string(),
                        connector: " ".to_string(),
                        score: -15.0,
                        start: left.end,
                        end: right.start,
                    });
                }
            }
            None
        }) as PenaltyFn);

    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[2028990]),
        has_seq_simple(&[1495750, 10092135, 10092153, 1495740]),
        "ni+tsuke-penalty",
        -30.0,
        true,
        " ",
    ));
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[1527110]),
        has_seq_simple(&[2028990]),
        "mada+ni-penalty",
        -30.0,
        true,
        " ",
    ));
    r.penalties.push(def_generic_penalty(
        has_seq_simple(&[1008490]),
        has_seq_simple(&[2028940]),
        "to+mo-penalty",
        -30.0,
        true,
        " ",
    ));

    // verb-stem + たん penalty (-400)
    r.penalties
        .push(Arc::new(|left: &SegmentList, right: &SegmentList| {
            if left.end != right.start {
                return None;
            }
            if !right
                .segments
                .iter()
                .any(|s| s.info.seq_set.contains(&2646370))
            {
                return None;
            }
            const VERB_POS_TAN: &[&str] = &[
                "v1", "v5r", "v5s", "v5k", "v5g", "v5b", "v5m", "v5n", "v5t", "v5u", "vk", "vs",
                "vs-i", "n",
            ];
            for seg in &left.segments {
                if has_pos(seg, VERB_POS_TAN) {
                    return Some(Synergy {
                        description: "verb-stem+tan-penalty".to_string(),
                        connector: " ".to_string(),
                        score: -400.0,
                        start: left.end,
                        end: right.start,
                    });
                }
            }
            None
        }) as PenaltyFn);

    // short kana × short kana (non-serial)
    r.penalties.push(def_generic_penalty(
        filter_short_kana(1, &[]),
        filter_short_kana(1, &["と"]),
        "short",
        -9.0,
        false,
        " ",
    ));

    // semi-final particle not at end
    r.penalties
        .push(Arc::new(|left: &SegmentList, right: &SegmentList| {
            let f = filter_in_seq_set(
                &crate::score::SEMI_FINAL_PRT
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            );
            if !left.segments.iter().any(|s| f(s)) {
                return None;
            }
            Some(Synergy {
                description: "semi-final not final".to_string(),
                connector: " ".to_string(),
                score: -15.0,
                start: left.end,
                end: right.start,
            })
        }) as PenaltyFn);
}

// ============================================================================
// Segfilters
// ============================================================================

fn init_segfilters(r: &mut super::synergies::Registries) {
    // Auxiliary verbs must follow continuative
    r.segfilters.push(def_segfilter_must_follow(
        filter_is_conjugation(13),
        filter_in_seq_set(&[1342560]),
        false,
    ));

    // いる must not follow つ (2221640)
    {
        let f_2221640 = filter_in_seq_set(&[2221640]);
        r.segfilters.push(def_segfilter_must_follow(
            Arc::new(move |s: &Segment| !f_2221640(s)),
            filter_in_seq_set(&[1577980]),
            true,
        ));
    }

    // ん/んだ must not follow simple particles
    {
        let np: Vec<i64> = NOUN_PARTICLES.iter().copied().collect();
        let f_np = filter_in_seq_set(&np);
        r.segfilters.push(def_segfilter_must_follow(
            Arc::new(move |s: &Segment| !f_np(s)),
            filter_in_seq_set(&[2139720, 2849370, 2849387]),
            true,
        ));
    }

    // を + 枯らす
    r.segfilters.push(def_segfilter_must_follow(
        filter_in_seq_set(&[2029010]),
        filter_in_seq_set(&[2087020]),
        false,
    ));

    // Bad endings — filter_is_compound_end_text is dead in Python (never
    // matches), so right satisfies nothing → passthrough. Keep registration.
    r.segfilters.push(def_segfilter_must_follow(
        Arc::new(|_s: &Segment| false),
        super::synergy_filters::filter_is_compound_end_text(&[
            "ちゃい",
            "いか",
            "とか",
            "とき",
            "い",
        ]),
        false,
    ));

    // じゃない must not follow は-compound — dead for the same reason, but
    // registered identically.
    r.segfilters.push(def_segfilter_must_follow(
        Arc::new(|_s: &Segment| true), // not compound_end → always true
        filter_in_seq_set(&[1529520, 1296400, 2139720]),
        true,
    ));

    // だ + する (dashi) — custom
    r.segfilters.push(
        Arc::new(|seg_left: Option<&SegmentList>, seg_right: &SegmentList| {
            let f_right = filter_in_seq_set(&[1157170, 2424740, 1305070]);
            let satisfies_r: Vec<Rc<Segment>> = seg_right
                .segments
                .iter()
                .filter(|s| f_right(s))
                .cloned()
                .collect();
            let contradicts_r: Vec<Rc<Segment>> = seg_right
                .segments
                .iter()
                .filter(|s| !f_right(s))
                .cloned()
                .collect();
            if satisfies_r.is_empty() {
                return vec![(seg_left.cloned(), seg_right.clone())];
            }
            let left = match seg_left {
                Some(l) => l,
                None => return vec![(None, seg_right.clone())],
            };
            let left_ok = |s: &Segment| {
                let ss = &s.info.seq_set;
                !ss.contains(&2089020) || ss.contains(&2028980)
            };
            if left.segments.iter().any(|s| left_ok(s)) {
                return vec![(Some(left.clone()), seg_right.clone())];
            }
            if !contradicts_r.is_empty() {
                return vec![(Some(left.clone()), mk_list(contradicts_r, seg_right))];
            }
            Vec::new()
        }) as SegfilterFn,
    );

    // Honorifics must follow non-particles
    {
        let np: Vec<i64> = NOUN_PARTICLES.iter().copied().collect();
        let f_np = filter_in_seq_set(&np);
        r.segfilters.push(def_segfilter_must_follow(
            Arc::new(move |s: &Segment| !f_np(s)),
            filter_in_seq_set(&[1247260]),
            false,
        ));
    }

    // くん before a particle → pronoun reading, remove くん
    r.segfilters.push(
        Arc::new(|seg_left: Option<&SegmentList>, seg_right: &SegmentList| {
            let left = match seg_left {
                Some(l) if l.end == seg_right.start => l,
                _ => return vec![(seg_left.cloned(), seg_right.clone())],
            };
            let np: Vec<i64> = NOUN_PARTICLES.iter().copied().collect();
            let f_part = filter_in_seq_set(&np);
            if !seg_right.segments.iter().any(|s| f_part(s)) {
                return vec![(Some(left.clone()), seg_right.clone())];
            }
            let f_kun = filter_in_seq_set(&[SEQ_KUN]);
            let left_wo: Vec<Rc<Segment>> = left
                .segments
                .iter()
                .filter(|s| !f_kun(s))
                .cloned()
                .collect();
            if left_wo.is_empty() {
                return Vec::new();
            }
            vec![(Some(mk_list(left_wo, left)), seg_right.clone())]
        }) as SegfilterFn,
    );

    // に + つけ blocked (prefer につけ)
    {
        let f_l = filter_in_seq_set(&[2028990]);
        let f_r = filter_in_seq_set(&[1495750, 10092135, 10092153, 1495740]);
        r.segfilters
            .push(Arc::new(move |l: Option<&SegmentList>, rr: &SegmentList| {
                block_pair(l, rr, |s| f_l(s), |s| f_r(s))
            }) as SegfilterFn);
    }

    // 未だ + に blocked (prefer 未だに)
    {
        let f_l = filter_in_seq_set(&[1527110]);
        let f_r = filter_in_seq_set(&[2028990]);
        r.segfilters
            .push(Arc::new(move |l: Option<&SegmentList>, rr: &SegmentList| {
                block_pair(l, rr, |s| f_l(s), |s| f_r(s))
            }) as SegfilterFn);
    }

    // と + も blocked (prefer とも)
    {
        let f_l = filter_in_seq_set(&[1008490]);
        let f_r = filter_in_seq_set(&[2028940]);
        r.segfilters
            .push(Arc::new(move |l: Option<&SegmentList>, rr: &SegmentList| {
                block_pair(l, rr, |s| f_l(s), |s| f_r(s))
            }) as SegfilterFn);
    }

    // verb negative-imperative + ん blocked
    {
        let f_r = filter_in_seq_set(&[2139720]);
        r.segfilters
            .push(Arc::new(move |l: Option<&SegmentList>, rr: &SegmentList| {
                block_pair(
                    l,
                    rr,
                    |s| seg_has_conj_type(s, 10) && seg_is_neg(s),
                    |s| f_r(s),
                )
            }) as SegfilterFn);
    }

    // Remove いくさ reading of 戦 after nouns
    r.segfilters.push(
        Arc::new(|seg_left: Option<&SegmentList>, seg_right: &SegmentList| {
            let left = match seg_left {
                Some(l) if l.end == seg_right.start => l,
                _ => return vec![(seg_left.cloned(), seg_right.clone())],
            };
            let f_noun = filter_is_noun();
            if !left.segments.iter().any(|s| f_noun(s)) {
                return vec![(Some(left.clone()), seg_right.clone())];
            }
            let f_ikusa = filter_in_seq_set(&[SEQ_IKUSA_NOUN]);
            let right_wo: Vec<Rc<Segment>> = seg_right
                .segments
                .iter()
                .filter(|s| !f_ikusa(s))
                .cloned()
                .collect();
            if right_wo.is_empty() {
                return Vec::new();
            }
            if right_wo.len() == seg_right.segments.len() {
                return vec![(Some(left.clone()), seg_right.clone())];
            }
            vec![(Some(left.clone()), mk_list(right_wo, seg_right))]
        }) as SegfilterFn,
    );

    // Block ないよう matched from kana (内容/内用/内洋)
    r.segfilters.push(
        Arc::new(|seg_left: Option<&SegmentList>, seg_right: &SegmentList| {
            const NAIYOU: &[i64] = &[1459400, 1459440, 2862582];
            let filtered: Vec<Rc<Segment>> = seg_right
                .segments
                .iter()
                .filter(|s| {
                    let seq = s.word.seq().unwrap_or(0);
                    !(NAIYOU.contains(&seq) && s.word.word_type() == "kana")
                })
                .cloned()
                .collect();
            if filtered.len() == seg_right.segments.len() {
                return vec![(seg_left.cloned(), seg_right.clone())];
            }
            if filtered.is_empty() {
                return Vec::new();
            }
            vec![(seg_left.cloned(), mk_list(filtered, seg_right))]
        }) as SegfilterFn,
    );

    // Block ところが conjunction after rentaikei
    r.segfilters.push(
        Arc::new(|seg_left: Option<&SegmentList>, seg_right: &SegmentList| {
            let left = match seg_left {
                Some(l) if l.end == seg_right.start => l,
                _ => return vec![(seg_left.cloned(), seg_right.clone())],
            };
            let is_tokoroga = |s: &Segment| s.word.seq() == Some(1008570);
            let is_rentaikei = |s: &Segment| {
                if s.info.conj.is_empty() {
                    return has_pos(s, &["n", "adj-i", "adj-na"]);
                }
                s.info.conj.iter().any(|cd| {
                    cd.prop
                        .as_ref()
                        .map(|p| p.conj_type == 1 || p.conj_type == 2)
                        .unwrap_or(false)
                })
            };
            let tokoroga: Vec<Rc<Segment>> = seg_right
                .segments
                .iter()
                .filter(|s| is_tokoroga(s))
                .cloned()
                .collect();
            let other: Vec<Rc<Segment>> = seg_right
                .segments
                .iter()
                .filter(|s| !is_tokoroga(s))
                .cloned()
                .collect();
            if tokoroga.is_empty() {
                return vec![(Some(left.clone()), seg_right.clone())];
            }
            if left.segments.iter().any(|s| is_rentaikei(s)) {
                if !other.is_empty() {
                    return vec![(Some(left.clone()), mk_list(other, seg_right))];
                }
                return Vec::new();
            }
            vec![(Some(left.clone()), seg_right.clone())]
        }) as SegfilterFn,
    );

    // Block たん/たんだ after masu-stem
    r.segfilters.push(
        Arc::new(|seg_left: Option<&SegmentList>, seg_right: &SegmentList| {
            let left = match seg_left {
                Some(l) if l.end == seg_right.start => l,
                _ => return vec![(seg_left.cloned(), seg_right.clone())],
            };
            let is_tan = |s: &Segment| s.word.text() == "たん" || s.word.text() == "たんだ";
            let is_stem = |s: &Segment| {
                if s.info
                    .conj
                    .iter()
                    .any(|cd| cd.prop.as_ref().map(|p| p.conj_type == 13).unwrap_or(false))
                {
                    return true;
                }
                if s.info.posi.contains("n") {
                    const TAILS: &[char] = &[
                        'け', 'き', 'し', 'り', 'ち', 'み', 'び', 'ぎ', 'に', 'え', 'い', 'て',
                        'れ', 'ね', 'め', 'べ', 'げ',
                    ];
                    if let Some(last) = s.word.text().chars().last() {
                        if TAILS.contains(&last) {
                            return true;
                        }
                    }
                }
                false
            };
            let tan: Vec<Rc<Segment>> = seg_right
                .segments
                .iter()
                .filter(|s| is_tan(s))
                .cloned()
                .collect();
            let other: Vec<Rc<Segment>> = seg_right
                .segments
                .iter()
                .filter(|s| !is_tan(s))
                .cloned()
                .collect();
            if tan.is_empty() {
                return vec![(Some(left.clone()), seg_right.clone())];
            }
            if left.segments.iter().any(|s| is_stem(s)) {
                if !other.is_empty() {
                    return vec![(Some(left.clone()), mk_list(other, seg_right))];
                }
                return Vec::new();
            }
            vec![(Some(left.clone()), seg_right.clone())]
        }) as SegfilterFn,
    );
}
