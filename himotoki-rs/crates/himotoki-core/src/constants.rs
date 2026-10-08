//! Consolidated constants — port of `himotoki/constants.py`.
//!
//! WARNING: many constants are JMdict sequence numbers. They are valid only
//! against the DB built by the same pipeline (`data/himotoki.db`).

// ============================================================================
// Conjugation Type Constants (constants.py — rule-side numbering)
// ============================================================================

pub const CONJ_NON_PAST: i64 = 1;
pub const CONJ_PAST: i64 = 2;
pub const CONJ_TE: i64 = 3;
pub const CONJ_PROVISIONAL: i64 = 4;
pub const CONJ_POTENTIAL: i64 = 5;
pub const CONJ_PASSIVE: i64 = 6;
pub const CONJ_CAUSATIVE: i64 = 7;
pub const CONJ_CAUSATIVE_PASSIVE: i64 = 8;
pub const CONJ_VOLITIONAL: i64 = 9;
pub const CONJ_IMPERATIVE: i64 = 10;
pub const CONJ_CONDITIONAL: i64 = 11;
pub const CONJ_ALTERNATIVE: i64 = 12;
pub const CONJ_CONTINUATIVE: i64 = 13;

// Custom conjugation types (>=50 to avoid clashes with standard types)
pub const CONJ_ADVERBIAL: i64 = 50;
pub const CONJ_ADJECTIVE_STEM: i64 = 51;
pub const CONJ_NEGATIVE_STEM: i64 = 52;
pub const CONJ_CAUSATIVE_SU: i64 = 53; // NB: also 14 in conjo.csv context
pub const CONJ_ADJECTIVE_LITERARY: i64 = 54;

/// Human-readable names for conjugation types.
pub fn conj_type_name(conj_type: i64) -> String {
    match conj_type {
        CONJ_NON_PAST => "Non-past",
        CONJ_PAST => "Past (~ta)",
        CONJ_TE => "Conjunctive (~te)",
        CONJ_PROVISIONAL => "Provisional (~eba)",
        CONJ_POTENTIAL => "Potential",
        CONJ_PASSIVE => "Passive",
        CONJ_CAUSATIVE => "Causative",
        CONJ_CAUSATIVE_PASSIVE => "Causative-Passive",
        CONJ_VOLITIONAL => "Volitional",
        CONJ_IMPERATIVE => "Imperative",
        CONJ_CONDITIONAL => "Conditional (~tara)",
        CONJ_ALTERNATIVE => "Alternative (~tari)",
        CONJ_CONTINUATIVE => "Continuative (~i)",
        CONJ_ADVERBIAL => "Adverbial",
        CONJ_ADJECTIVE_STEM => "Adjective Stem",
        CONJ_NEGATIVE_STEM => "Negative Stem",
        CONJ_CAUSATIVE_SU => "Causative (~su)",
        CONJ_ADJECTIVE_LITERARY => "Old/Literary",
        other => return format!("Type {}", other),
    }
    .to_string()
}

/// English glosses for each conjugation step (breakdown tree display).
pub fn conj_step_gloss(conj_type: i64) -> &'static str {
    match conj_type {
        CONJ_NON_PAST => "does/is",
        CONJ_PAST => "did/was",
        CONJ_TE => "",
        CONJ_PROVISIONAL => "if",
        CONJ_POTENTIAL => "can do",
        CONJ_PASSIVE => "is done (to)",
        CONJ_CAUSATIVE => "makes do",
        CONJ_CAUSATIVE_PASSIVE => "is made to do",
        CONJ_VOLITIONAL => "let's/will",
        CONJ_IMPERATIVE => "do!",
        CONJ_CONDITIONAL => "if/when",
        CONJ_ALTERNATIVE => "doing things like",
        CONJ_CONTINUATIVE => "and (stem)",
        CONJ_ADVERBIAL => "adverbially",
        CONJ_ADJECTIVE_STEM => "stem",
        CONJ_NEGATIVE_STEM => "not (stem)",
        CONJ_CAUSATIVE_SU => "makes do",
        CONJ_ADJECTIVE_LITERARY => "old/literary",
        _ => "",
    }
}

// ============================================================================
// Weak/skip conjugation forms
// ============================================================================
// (conj_type, neg, fml) — None == "any"

/// Conj-form pattern: (conj_type, neg, fml) len-3 or (pos, conj_type, neg, fml)
/// len-4; None == "any". Shared shape for WEAK_CONJ_FORMS and SKIP_CONJ_FORMS.
#[derive(Debug, Clone, PartialEq)]
pub enum SkipForm {
    Cnf(i64, Option<bool>, Option<bool>),
    PosCnf(&'static str, i64, Option<bool>, Option<bool>),
}

pub static WEAK_CONJ_FORMS: &[SkipForm] = &[
    SkipForm::Cnf(CONJ_ADJECTIVE_STEM, None, None),
    SkipForm::Cnf(CONJ_NEGATIVE_STEM, None, None),
    SkipForm::Cnf(CONJ_CAUSATIVE_SU, None, None),
    SkipForm::Cnf(CONJ_ADJECTIVE_LITERARY, None, None),
    SkipForm::Cnf(CONJ_VOLITIONAL, Some(true), None),
];

pub static SKIP_CONJ_FORMS: &[SkipForm] = &[
    SkipForm::Cnf(CONJ_IMPERATIVE, Some(true), None),
    SkipForm::Cnf(CONJ_TE, Some(true), Some(true)),
    SkipForm::PosCnf("vs-s", CONJ_POTENTIAL, None, None),
];

// ============================================================================
// JMdict Sequence Number Constants
// ============================================================================

pub const SEQ_WA: i64 = 2028920;
pub const SEQ_GA: i64 = 2028930;
pub const SEQ_NI: i64 = 2028990;
pub const SEQ_DE: i64 = 2028980;
pub const SEQ_HE: i64 = 2029000;
pub const SEQ_WO: i64 = 2029010;
pub const SEQ_NO: i64 = 1469800;
pub const SEQ_TO: i64 = 1008490;
pub const SEQ_MO: i64 = 2028940;
pub const SEQ_YA: i64 = 2028960;
pub const SEQ_KA: i64 = 2028970;
pub const SEQ_YO: i64 = 2029090;

pub const SEQ_NIHA: i64 = 2215430;
pub const SEQ_TOHA: i64 = 2028950;
pub const SEQ_TOKA: i64 = 1008530;
pub const SEQ_TOSHITE: i64 = 1008590;
pub const SEQ_DESAE: i64 = 2034520;

pub const SEQ_DAKE: i64 = 1007340;
pub const SEQ_GORO: i64 = 1579080;
pub const SEQ_MADE: i64 = 1525680;
pub const SEQ_NADO: i64 = 1582300;
pub const SEQ_NOMI: i64 = 1009990;
pub const SEQ_SAE: i64 = 1005120;
pub const SEQ_TTE: i64 = 2086960;
pub const SEQ_KARA: i64 = 1002980;
pub const SEQ_NITOTTE: i64 = 1009600;

pub const SEQ_KIMI: i64 = 1247250;
pub const SEQ_KUN: i64 = 1247260;

pub const SEQ_SURU: i64 = 1157170;
pub const SEQ_IRU: i64 = 1577980;
pub const SEQ_KURU: i64 = 1547720;
pub const SEQ_ARU: i64 = 1296400;
pub const SEQ_NARU: i64 = 1375610;
pub const SEQ_TOMU: i64 = 1496740;
pub const SEQ_ORU: i64 = 1577985;
pub const SEQ_OKU: i64 = 1421850;
pub const SEQ_IKU: i64 = 1578850;
pub const SEQ_SHIMAU: i64 = 1305380;
pub const SEQ_MORAU: i64 = 1535910;
pub const SEQ_ITADAKU: i64 = 1587290;
pub const SEQ_KURERU: i64 = 1269130;
pub const SEQ_MIRU: i64 = 1259290;
pub const SEQ_AGERU: i64 = 1352320;
pub const SEQ_HOSHII: i64 = 1547330;

pub const SEQ_ITASU: i64 = 1421900;
pub const SEQ_SARERU: i64 = 2269820;
pub const SEQ_SASERU: i64 = 1005160;
pub const SEQ_TOKU: i64 = 2108590;

pub const SEQ_CHAU: i64 = 2013800;
pub const SEQ_CHIMAU: i64 = 2210750;
pub const SEQ_TAI: i64 = 2017560;

pub const SEQ_NAI: i64 = 2029110;
pub const SEQ_II: i64 = 2820690;

pub const SEQ_NIKUI: i64 = 2772730;
pub const SEQ_YASUI_P: i64 = 2028620; // やすい (easy to) — Python had a duplicate binding; last wins
pub const SEQ_SUGIRU: i64 = 1398990;
pub const SEQ_TSUZUKERU: i64 = 1405800;
pub const SEQ_TSUTSU: i64 = 2027910;
pub const SEQ_NAGARA: i64 = 1459640;
pub const SEQ_URU: i64 = 1454500;
pub const SEQ_KUDASAI: i64 = 1184270;
pub const SEQ_SOU: i64 = 1006610;
pub const SEQ_SOU_NI_NAI: i64 = 2141080;
pub const SEQ_PPOI: i64 = 2083720;
pub const SEQ_GATAI: i64 = 2867504;
pub const SEQ_DASU: i64 = 1338180;
pub const SEQ_KIRU: i64 = 1384830;
pub const SEQ_KATA: i64 = 1516925;
pub const SEQ_MI: i64 = 2258670;
/// NB: Python defines SEQ_YASUI twice (2028620 then 1156990); the second wins.
pub const SEQ_YASUI: i64 = 1156990;
pub const SEQ_MAKURU: i64 = 1257800;
pub const SEQ_NAOSU: i64 = 1599390;
pub const SEQ_SOKONAU: i64 = 1596510;
pub const SEQ_WASURERU: i64 = 1519210;
pub const SEQ_OERU: i64 = 1332760;
pub const SEQ_ZURAI: i64 = 2096480;
pub const SEQ_GIMI: i64 = 1790980;
pub const SEQ_PPANASHI: i64 = 1008020;
pub const SEQ_TACHI: i64 = 1416220;
pub const SEQ_AU: i64 = 1284430;
pub const SEQ_KOMU: i64 = 1593410;
pub const SEQ_HOUDAI: i64 = 1516770;
pub const SEQ_OWARU: i64 = 1589600;
pub const SEQ_HAJIMERU: i64 = 1307550;
pub const SEQ_TSUKERU: i64 = 1331540;
pub const SEQ_YARU: i64 = 1012980;
pub const SEQ_MAIRU: i64 = 1302070;
pub const SEQ_KUDASARU: i64 = 1184280;
pub const SEQ_SASHIAGERU: i64 = 1291270;

pub const SEQ_TASOU: i64 = 900000;
pub const SEQ_MOII: i64 = 900001;

pub const SEQ_MAE_NOUN: i64 = 1392580;
pub const SEQ_HOU_NOUN: i64 = 1516930;
pub const SEQ_MEN_NOUN: i64 = 1584695;
pub const SEQ_HITO_NOUN: i64 = 1580640;
pub const SEQ_JIN_SUFFIX: i64 = 1366410;
pub const SEQ_NAKA_NOUN: i64 = 1423310;
pub const SEQ_SEN_NOUN: i64 = 2080730;
pub const SEQ_IKUSA_NOUN: i64 = 1587140;
pub const SEQ_TOMARU: i64 = 1310620;
pub const SEQ_TODOMARU: i64 = 2657130;
pub const SEQ_KARAI: i64 = 1365850;
pub const SEQ_TSURAI: i64 = 1365860;
pub const SEQ_KADOUKA: i64 = 2087300;
pub const SEQ_NITSURE: i64 = 2136050;
pub const SEQ_OSUSUME: i64 = 1002150;
pub const SEQ_HYAKUEN_SHOP: i64 = 2100330;
pub const SEQ_DENAITO: i64 = 2009070;
pub const SEQ_MURI_WO_SURU: i64 = 2838589;
pub const SEQ_UN_GA_II: i64 = 1172620;
pub const SEQ_KI_GA_SURU: i64 = 1221540;
pub const SEQ_HITO_GA_II: i64 = 2250200;

pub const SEQ_O_PREFIX: i64 = 2826528;

// --- Blocked seqs ---

lazy_set! { pub static BLOCKED_NAI_SEQS: HashSet<i64> = { SEQ_IRU, SEQ_KURU, SEQ_MIRU } }
lazy_set! { pub static BLOCKED_NAI_X_SEQS: HashSet<i64> = { SEQ_SURU, SEQ_TOMU } }
// (lazy_set! expands `HashSet` via fully-qualified path; no import needed)

// Particles that can follow nouns (noun+particle synergy).
lazy_set! { pub static NOUN_PARTICLES: HashSet<i64> = {
    SEQ_WA, SEQ_GA, SEQ_NI, SEQ_DE, SEQ_HE,
    SEQ_DAKE, SEQ_GORO, SEQ_MADE, SEQ_MO,
    SEQ_NADO, SEQ_NIHA, SEQ_NO, SEQ_NOMI,
    SEQ_WO, SEQ_SAE, SEQ_DESAE, SEQ_TO,
    SEQ_TOKA, SEQ_TOSHITE, SEQ_TOHA, SEQ_YA,
    SEQ_NITOTTE,
} }

// --- Suffix descriptions ---

pub fn suffix_description_seq(seq: i64) -> Option<&'static str> {
    Some(match seq {
        SEQ_O_PREFIX => "polite prefix",
        SEQ_DE => "at / in / by",
        SEQ_KA => "or / questioning particle",
        SEQ_NI => "to / at / in",
        SEQ_WO => "indicates direct object of action",
        SEQ_NO => "indicates possessive (...'s)",
        SEQ_TTE => "quoting particle",
        SEQ_KARA => "from / because",
        _ => return None,
    })
}

// ============================================================================
// POS tags (constants.py interned table — values are what matter)
// ============================================================================

pub static POS_TAGS: &[&str] = &[
    "n", "n-adv", "n-pref", "n-suf", "n-t", "v1", "v1-s", "v5aru", "v5b", "v5g", "v5k", "v5k-s",
    "v5m", "v5n", "v5r", "v5r-i", "v5s", "v5t", "v5u", "v5u-s", "v5uru", "vk", "vs", "vs-i",
    "vs-s", "vz", "vi", "vt", "vs-c", "adj-i", "adj-ix", "adj-na", "adj-no", "adj-pn", "adj-t",
    "adj-f", "adv", "adv-to", "aux", "aux-v", "aux-adj", "conj", "cop", "ctr", "exp", "int", "pn",
    "pref", "prt", "suf", "unc", "uk", "arch", "male", "fem", "vulg", "hon", "hum", "col", "fam",
];
