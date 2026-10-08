//! Conjugation hints — port of `himotoki/conjugation_hints.py`.
//! Static phrase tables + lookup for learner-friendly grammar hints.

/// Compound phrases indexed by first token → [(full_phrase, meaning)].
pub static COMPOUND_PHRASES: &[(&str, &[(&str, &str)])] = &[
    (
        "か",
        &[
            ("かどうか", "whether or not"),
            ("かのように", "as if; as though"),
            ("かもしれない", "might; may; possibly"),
            ("かもしれません", "might; may (polite)"),
        ],
    ),
    (
        "こと",
        &[
            ("ことにする", "decide to"),
            ("ことになる", "it's been decided; will end up"),
            ("ことができる", "can; be able to"),
            ("ことがある", "sometimes; have experienced"),
        ],
    ),
    (
        "な",
        &[
            ("なければならない", "must; have to"),
            ("なければいけない", "must; have to"),
            ("なければなりません", "must (polite)"),
            ("なくてはならない", "must; have to"),
            ("なくてはいけない", "must; have to"),
            ("ないといけない", "must; have to"),
            ("なきゃ", "must (casual)"),
            ("なくちゃ", "must (casual)"),
            ("ないわけにはいかない", "must; have no choice but to"),
        ],
    ),
    (
        "なきゃ",
        &[
            ("なきゃいけない", "must; have to (casual)"),
            ("なきゃだめ", "must; have to (casual)"),
            ("なきゃならない", "must; have to (casual)"),
        ],
    ),
    (
        "と",
        &[
            ("といけない", "must (if not...)"),
            ("というのは", "what ~ means is"),
            ("ということだ", "it means that; I heard that"),
            ("ところだ", "about to; just did"),
        ],
    ),
    (
        "て",
        &[
            ("てはいけない", "must not; may not"),
            ("てはいけません", "must not (polite)"),
            ("てはだめ", "must not (casual)"),
            ("てもいい", "may; it's okay to"),
            ("てもいいですか", "may I...?"),
            ("ても", "even if; even though"),
            ("てたまらない", "unbearably; extremely"),
            ("てならない", "can't help but feel"),
            ("てほしい", "want someone to do"),
            ("ている", "~ing; currently"),
            ("ていた", "was ~ing"),
            ("ていない", "not ~ing"),
            ("てしまう", "completely; accidentally"),
            ("てしまった", "ended up; regrettably did"),
        ],
    ),
    (
        "は",
        &[("はいけない", "must not"), ("はだめ", "must not (casual)")],
    ),
    ("ほど", &[("ほど", "the more... the more")]),
    (
        "わけ",
        &[
            ("わけだ", "no wonder; that's why"),
            ("わけがない", "no way that; impossible"),
            ("わけではない", "doesn't mean that"),
            ("わけにはいかない", "can't possibly; mustn't"),
        ],
    ),
    (
        "ば",
        &[
            ("ばよかった", "should have; wish I had"),
            ("ばいい", "should; ought to"),
            ("ばかり", "just; only; nothing but"),
        ],
    ),
    (
        "た",
        &[
            ("たばかり", "just did"),
            ("たことがある", "have done before"),
            ("たことがない", "have never done"),
            ("たほうがいい", "had better"),
            ("たい", "want to"),
            ("たかった", "wanted to"),
            ("たくない", "don't want to"),
        ],
    ),
    (
        "に",
        &[
            ("において", "in; at; regarding"),
            ("に対して", "towards; regarding"),
            ("について", "about; concerning"),
            ("によって", "by means of; depending on"),
            ("にとって", "for; to (someone)"),
            ("にしても", "even if; even though"),
            ("にちがいない", "must be; no doubt"),
            ("にすぎない", "merely; nothing but"),
        ],
    ),
    (
        "の",
        &[
            ("のに", "although; even though"),
            ("ので", "because; since"),
            ("のだ", "it is that; the fact is"),
            ("のです", "it is that (polite)"),
        ],
    ),
    (
        "そ",
        &[
            ("そうだ", "I heard that; seems like"),
            ("そうです", "I heard that (polite)"),
            ("そうにない", "unlikely to; doesn't seem"),
        ],
    ),
    (
        "よ",
        &[
            ("ようにする", "to make sure to"),
            ("ようになる", "to come to; to become able"),
            ("ようとする", "try to; be about to"),
            ("ようがない", "no way to; cannot"),
        ],
    ),
    ("ざ", &[("ざるをえない", "can't help but; have to")]),
    (
        "し",
        &[
            ("しかない", "have no choice but to"),
            ("しかたがない", "can't be helped"),
        ],
    ),
    (
        "せ",
        &[
            ("せいだ", "because of (negative cause)"),
            ("せいで", "because of (negative cause)"),
        ],
    ),
    (
        "お",
        &[
            ("おかげだ", "thanks to (positive cause)"),
            ("おかげで", "thanks to (positive cause)"),
        ],
    ),
    (
        "ど",
        &[
            ("どころか", "far from; let alone"),
            ("どころではない", "not in a position to"),
        ],
    ),
    ("だ", &[("だけでなく", "not only... but also")]),
    (
        "ちゃ",
        &[
            ("ちゃう", "end up doing (casual)"),
            ("ちゃった", "ended up doing"),
            ("ちゃいけない", "must not (casual)"),
            ("ちゃだめ", "must not (casual)"),
        ],
    ),
    (
        "じゃ",
        &[("じゃない", "isn't"), ("じゃないですか", "isn't it?")],
    ),
    (
        "らしい",
        &[
            ("らしい", "seems like; apparently"),
            ("らしいです", "seems like (polite)"),
        ],
    ),
    (
        "みたい",
        &[
            ("みたいだ", "seems like; looks like"),
            ("みたいです", "seems like (polite)"),
        ],
    ),
    ("ため", &[("ために", "in order to; for the sake of")]),
    (
        "よう",
        &[
            ("ような", "like; such as"),
            ("ように", "so that; in order to"),
            ("ようだ", "seems like; appears"),
        ],
    ),
    ("がち", &[("がちだ", "tend to; prone to")]),
    ("かけ", &[("かける", "start doing; partially do")]),
    (
        "きれ",
        &[
            ("きれる", "can do completely"),
            ("きれない", "cannot finish; unbearable"),
        ],
    ),
    ("づらい", &[("づらい", "hard to (physical/habitual)")]),
    ("にくい", &[("にくい", "hard to; difficult to")]),
    ("やすい", &[("やすい", "easy to")]),
    (
        "すぎ",
        &[
            ("すぎる", "too much; excessively"),
            ("すぎた", "was too much"),
        ],
    ),
    ("ながら", &[("ながら", "while doing; although")]),
    (
        "まま",
        &[
            ("まま", "as is; in the state of"),
            ("ままだ", "is still in the state of"),
        ],
    ),
];

/// Look up a conjugation hint for the given text.
/// Mirrors `get_conjugation_hint` — first-char bucket, then substring scan.
pub fn get_conjugation_hint(text: &str) -> Option<&'static str> {
    if text.is_empty() {
        return None;
    }
    let first = text.chars().next().unwrap().to_string();
    if let Some((_, patterns)) = COMPOUND_PHRASES.iter().find(|(k, _)| *k == first) {
        for (phrase, meaning) in *patterns {
            if text == *phrase || text.ends_with(phrase) {
                return Some(meaning);
            }
        }
    }
    for (_, patterns) in COMPOUND_PHRASES {
        for (phrase, meaning) in *patterns {
            if phrase.chars().count() >= 2 && text.contains(phrase) {
                return Some(meaning);
            }
        }
    }
    None
}
