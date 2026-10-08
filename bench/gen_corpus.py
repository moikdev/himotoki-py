#!/usr/bin/env python3
"""Generate the stress corpus for the Python/Rust benchmark.

Reads the golden corpus and synthesizes adversarial categories:
  gold   - the 633 real golden inputs (baseline realism)
  long   - golden sentences concatenated to ~95 chars (max-length stress)
  counter- number + counter expressions (rendaku/teouto branches)
  kana   - hiragana/katakana-only strings (ambiguity stress)
  kanji  - kanji-dense compounds
  mixed  - ASCII/emoji/punctuation mixed with Japanese
  patho  - short pathological strings (combinatorial stress)

Output: bench/corpus.jsonl  {"i", "cat", "text"}
Deterministic (seeded) so both implementations see identical input.
"""

import json
import random
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GOLD = ROOT / "himotoki-rs" / "tests" / "golden" / "inputs.jsonl"
OUT = ROOT / "bench" / "corpus.jsonl"

random.seed(20260724)


def load_gold():
    texts = []
    for line in GOLD.read_text(encoding="utf-8").splitlines():
        if line.strip():
            texts.append(json.loads(line)["text"])
    return texts


def truncate(text: str, n: int = 95) -> str:
    return text[:n]


def gen_long(gold):
    out = []
    for _ in range(200):
        a, b = random.choice(gold), random.choice(gold)
        t = truncate(a + "。" + b)
        if len(t) > 20:
            out.append(t)
    return out


COUNTERS = [
    "年", "月", "日", "時", "分", "秒", "人", "個", "冊", "枚", "台", "匹",
    "杯", "回", "階", "歳", "円", "メートル", "キロ", "ページ", "番", "度",
    "倍", "割", "本", "件", "点", "名", "頭", "羽", "足", "膳", "校",
]
NUMBERS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "20",
           "24", "100", "365", "1000", "2024", "一万", "三千", "五百",
           "３", "１０", "12345", "7.5", "0.5"]
TEMPLATES = [
    "{} {} が来た", "{} {} を買った", "彼女は {} {} 待った",
    "{} {} の本を読む", "そこから {} {} 歩いた", "{} {} かかった",
    "約 {} {} です", "{} {} に達した",
]


def gen_counter():
    out = []
    for _ in range(120):
        t = random.choice(TEMPLATES).format(
            random.choice(NUMBERS), random.choice(COUNTERS))
        out.append(t)
    return out


HIRA = "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわをんがぎぐげござじずぜぞだぢづでどばびぶべぼぱぴぷぺぽゃゅょっ"
KATA = "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲンガギグゲゴザジズゼゾダヂヅデドバビブベボパピプペポャュョッー"
KANJI = "日月火水木金土山川田中人子女子男大小上下左右中間学校先生日本語話読書見聞食飲行来帰出入仕事年月時分秒円元気力心手足目耳口車電話店駅前後北南西東"


def gen_kana():
    out = []
    for _ in range(80):
        n = random.randint(5, 60)
        pool = HIRA if random.random() < 0.6 else KATA
        out.append("".join(random.choice(pool) for _ in range(n)))
    return out


def gen_kanji():
    out = []
    for _ in range(80):
        n = random.randint(3, 40)
        t = "".join(random.choice(KANJI) for _ in range(n))
        if random.random() < 0.5:
            t += random.choice(["です", "した", "する", "だった"])
        out.append(t)
    return out


def gen_mixed():
    out = []
    fragments = [
        "hello", "OK", "email@example.com", "2024-01-15", "http://x.jp",
        "😀", "🎉", "ABC", "Wi-Fi", "iPhone", "50%", "#tag", "C++",
    ]
    for _ in range(80):
        parts = [random.choice(load_gold_cache())[: random.randint(5, 20)]
                 for _ in range(random.randint(1, 3))]
        parts.insert(random.randint(0, len(parts)), random.choice(fragments))
        out.append(truncate("".join(parts)))
    return out


def gen_patho():
    out = []
    # Long repetitive kana - worst-case substring enumeration
    for _ in range(15):
        out.append("あ" * random.randint(20, 30))
        out.append("の" * random.randint(10, 20))
        out.append(("です" * 8)[: random.randint(15, 30)])
        out.append(("する" * 8)[: random.randint(15, 30)])
    # Particle chains
    for _ in range(15):
        out.append(("のはがにをで" * 5)[: random.randint(15, 30)])
    # Single-char kanji runs
    for _ in range(10):
        n = random.randint(10, 25)
        out.append("".join(random.choice(KANJI) for _ in range(n)))
    return out


_gold_cache = None


def load_gold_cache():
    global _gold_cache
    if _gold_cache is None:
        _gold_cache = load_gold()
    return _gold_cache


def main():
    gold = load_gold()
    corpus = []
    cats = [
        ("gold", gold),
        ("long", gen_long(gold)),
        ("counter", gen_counter()),
        ("kana", gen_kana()),
        ("kanji", gen_kanji()),
        ("mixed", gen_mixed()),
        ("patho", gen_patho()),
    ]
    i = 0
    for cat, texts in cats:
        for t in texts:
            corpus.append({"i": i, "cat": cat, "text": t})
            i += 1
    OUT.parent.mkdir(exist_ok=True)
    with OUT.open("w", encoding="utf-8") as f:
        for r in corpus:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    from collections import Counter
    print(f"wrote {len(corpus)} inputs -> {OUT}")
    print(Counter(r["cat"] for r in corpus))


if __name__ == "__main__":
    main()
