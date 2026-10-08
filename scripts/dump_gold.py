#!/usr/bin/env python3
"""
Dump golden segmentation data for the Rust port.

Generates JSONL fixtures under himotoki-rs/tests/golden/:

- inputs.jsonl    : {"i", "text"} corpus inputs
- candidates.jsonl: per-input candidate segments from join_substring_words
                    (pre-DP state: gates scoring/grammar phases)
- paths.jsonl     : per-input winning paths from segment_text (gates DP)
- output.jsonl    : per-input segment_to_json result (gates output layer)

Usage:
    uv run python scripts/dump_gold.py [--limit N] [--only inputs|candidates|paths|output]
"""

import json
import os
import sys
import unicodedata
from pathlib import Path

ROOT = Path(__file__).parent.parent
GOLD_DIR = ROOT / "himotoki-rs" / "tests" / "golden"

os.environ.setdefault("HIMOTOKI_DB_PATH", str(ROOT / "data" / "himotoki.db"))


def collect_inputs() -> list[str]:
    inputs: list[str] = []

    # 500 test sentences
    sys.path.insert(0, str(ROOT / "scripts"))
    try:
        from test_sentences import TEST_SENTENCES_500
        inputs.extend(TEST_SENTENCES_500)
    except Exception as e:
        print(f"warn: test_sentences import failed: {e}", file=sys.stderr)

    # 510 llm eval sentences
    res_path = ROOT / "output" / "llm_results.json"
    if res_path.exists():
        for entry in json.loads(res_path.read_text()):
            s = entry.get("sentence")
            if s:
                inputs.append(s)

    # Edge cases exercising sticky positions, counters, katakana runs, etc.
    inputs.extend([
        "学校で勉強しています", "食べさせられた", "走りたくなかった",
        "日本語を勉強する", "これは本です", "静かな部屋", "ゆっくりと歩く",
        "子供たち", "あいつ何考えてんだろうね", "来てんの",
        "てか最近どうしてるの", "考えてんだろうね", "食べてん", "行ってん",
        "見てん", "あ", "コーヒー", "かーい", "えーと", "三匹", "五冊",
        "二十歳", "一人", "二人", "三人", "123円", "４月５日",
        "食べちゃった", "飲んじゃう", "行っとく", "やっとく", "しとき",
        "わかんない", "じゃない", "すごくない？", "よくね？",
        "アメリカで買った", "昨日の夜", "３人で行った", "十五分",
        "駅まで歩いた", "私は学生です", "猫が好き", "犬を見た",
        "おいしいですね", "早く行かなきゃ", "食べなきゃ", "もう帰る",
        "何してるの？", "暇だから遊ぼう", "それは無理だよ",
        "ん？何？", "うーん、どうしよう", "あーあ、疲れた",
        "とにかくやってみる", "やっぱりダメだった", "さすがに無い",
        "食べようと思う", "行こうか迷う", "見られる", "読ませる",
        "書かせられた", "待たされちゃった", "飲まされちゃったよ",
        "走りすぎた", "食べ過ぎ", "静かすぎる", "大きすぎない？",
        "子供っぽい", "男らしい", "春らしい天気", "彼らしくない",
        "私たちの学校", "彼女たち", "これらの本", "あれら",
        "ご飯を食べなさい", "早くしろ", "来い！", "見ろ！",
        "静かにしろ", "ちゃんとやれ", "死ねばいい", "生きろ！",
        "そんなこと言わないで", "教えてください", "見せてください",
        "もう少し待ってくださいませんか", "お分かりになりましたか",
        "社長がいらっしゃいます", "先生がおっしゃいました",
        "明日雨が降るでしょう", "多分大丈夫でしょう",
        "彼は来ないだろう", "そう思わない？", "いいと思うよ",
        "お金がないんだ", "知らないんです", "会いたかったんだ",
        "本当なの？", "そうなんだね", "何なんだよ",
        "パソコンを使う", "インターネットで調べる", "スーパーで買い物",
        "コーヒーを飲みながら", "テレビを見る", "ゲームをする",
        "アプリをインストールした", "メールを送った",
        "ニュースで見た", "スマホが壊れた", "カメラで撮った",
    ])

    # Dedupe preserving order
    seen = set()
    out = []
    for s in inputs:
        s = unicodedata.normalize("NFC", s.strip())
        if s and s not in seen:
            seen.add(s)
            out.append(s)
    return out


def serialize_word(word) -> dict:
    name = type(word).__name__
    if name == "CounterText":
        return {
            "kind": "counter", "text": word.text, "kana": word.kana,
            "seq": word.seq, "ord": word.ord, "common": word.common,
            "number_value": word.number_value, "number_text": word.number_text,
            "counter_text": word.counter_text, "counter_kana": word.counter_kana,
            "ordinalp": word.ordinalp, "suffix": word.suffix,
        }
    if name == "CompoundWord":
        return {
            "kind": "compound", "text": word.text, "kana": word.kana,
            "seq": word.seq, "ord": word.ord, "common": word.common,
            "word_type": word.word_type, "is_abbrev": word.is_abbrev,
            "components": word.components,
            "words": [serialize_word(w) for w in word.words],
            "score_mod": word.score_mod if isinstance(word.score_mod, (int, float))
                        else list(word.score_mod),
            "conjugations": word.conjugations
            if word.conjugations == "root" else word.conjugations,
        }
    # WordMatch
    return {
        "kind": "word", "seq": word.seq, "text": word.text,
        "word_type": word.word_type, "ord": word.ord, "common": word.common,
        "is_root": word.is_root,
        "conjugations": word.conjugations
        if word.conjugations == "root" or word.conjugations is None
        else list(word.conjugations),
    }


def serialize_segment(seg) -> dict:
    d = serialize_word(seg.word)
    d["score"] = seg.score
    d["posi"] = sorted(seg.info.get("posi") or []) if seg.info else []
    return d


def serialize_path_node(node) -> dict:
    from himotoki.types import Segment, SegmentList
    from himotoki.synergies import Synergy

    if isinstance(node, Segment):
        return {
            "kind": "seg", "start": node.start, "end": node.end,
            "score": node.score, "word": serialize_word(node.word),
        }
    if isinstance(node, SegmentList):
        return {
            "kind": "seglist", "start": node.start, "end": node.end,
            "segs": [serialize_segment(s) for s in node.segments],
        }
    if isinstance(node, Synergy):
        return {
            "kind": "syn", "start": node.start, "end": node.end,
            "score": node.score, "desc": node.description,
        }
    return {"kind": type(node).__name__}


def main():
    only = None
    limit = None
    args = sys.argv[1:]
    if "--limit" in args:
        i = args.index("--limit")
        limit = int(args[i + 1])
        del args[i:i + 2]
    if "--only" in args:
        i = args.index("--only")
        only = args[i + 1]
        del args[i:i + 2]

    GOLD_DIR.mkdir(parents=True, exist_ok=True)
    inputs = collect_inputs()
    if limit:
        inputs = inputs[:limit]

    (GOLD_DIR / "inputs.jsonl").write_text(
        "".join(json.dumps({"i": i, "text": t}, ensure_ascii=False) + "\n"
                for i, t in enumerate(inputs))
    )
    print(f"inputs: {len(inputs)}")

    import himotoki
    himotoki.warm_up()
    from himotoki.db.connection import get_session
    from himotoki.segment import segment_text, join_substring_words
    from himotoki.output.format import segment_to_json

    session = get_session()

    def dump(name, fn):
        path = GOLD_DIR / f"{name}.jsonl"
        with path.open("w") as f:
            for i, text in enumerate(inputs):
                try:
                    rec = fn(i, text)
                except Exception as e:
                    rec = {"i": i, "text": text, "error": f"{type(e).__name__}: {e}"}
                f.write(json.dumps(rec, ensure_ascii=False, default=str) + "\n")
                if i % 100 == 0:
                    print(f"{name}: {i}/{len(inputs)}", file=sys.stderr)
        print(f"wrote {path}")

    if only in (None, "candidates"):
        def cand(i, text):
            sls = join_substring_words(session, text)
            return {
                "i": i, "text": text,
                "lists": [
                    {
                        "start": sl.start, "end": sl.end, "matches": sl.matches,
                        "segs": [serialize_segment(s) for s in sl.segments],
                    }
                    for sl in sls
                ],
            }
        dump("candidates", cand)

    if only in (None, "paths"):
        def paths(i, text):
            res = segment_text(session, text, limit=5)
            return {
                "i": i, "text": text,
                "paths": [
                    {"score": sc, "nodes": [serialize_path_node(n) for n in p]}
                    for p, sc in res
                ],
            }
        dump("paths", paths)

    if only in (None, "output"):
        def out(i, text):
            return {"i": i, "text": text,
                    "json": segment_to_json(session, text, limit=5)}
        dump("output", out)

    session.close()


if __name__ == "__main__":
    main()
