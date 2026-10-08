#!/usr/bin/env python3
"""
Compare Rust dump output against the Python golden corpus.

Usage:
    uv run python scripts/diff_gold.py [--rust DIR] [--gold DIR]
        [--only candidates|paths|output] [--show N]

Semantic JSON comparison: dict key order ignored; int==float equal;
reports a per-input status plus the first differing field paths.
"""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).parent.parent
GOLD = ROOT / "himotoki-rs" / "tests" / "golden"
RUST = Path("/tmp/rust-gold")


def load(path):
    recs = {}
    if not path.exists():
        return recs
    for line in path.read_text().splitlines():
        if line.strip():
            r = json.loads(line)
            recs[r["i"]] = r
    return recs


def diffs(a, b, path="", out=None, limit=20):
    """Yield dotted paths where a != b (structural compare)."""
    if out is None:
        out = []
    if len(out) >= limit:
        return out
    if type(a) != type(b) and not (
        isinstance(a, (int, float)) and isinstance(b, (int, float))
    ):
        out.append(f"{path}: type {type(a).__name__}!={type(b).__name__}")
        return out
    if isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a:
                out.append(f"{path}.{k}: missing in A")
            elif k not in b:
                out.append(f"{path}.{k}: missing in B")
            else:
                diffs(a[k], b[k], f"{path}.{k}", out, limit)
    elif isinstance(a, list):
        if len(a) != len(b):
            out.append(f"{path}: len {len(a)}!={len(b)}")
        for i, (x, y) in enumerate(zip(a, b)):
            diffs(x, y, f"{path}[{i}]", out, limit)
    elif a != b:
        out.append(f"{path}: {a!r} != {b!r}")
    return out


def main():
    only = None
    show = 10
    args = sys.argv[1:]
    if "--only" in args:
        only = args[args.index("--only") + 1]
    if "--show" in args:
        show = int(args[args.index("--show") + 1])
    if "--rust" in args:
        global RUST
        RUST = Path(args[args.index("--rust") + 1])
    if "--gold" in args:
        global GOLD
        GOLD = Path(args[args.index("--gold") + 1])

    kinds = [only] if only else ["candidates", "paths", "output"]
    for kind in kinds:
        gold = load(GOLD / f"{kind}.jsonl")
        rust = load(RUST / f"{kind}.jsonl")
        same, diff, missing = 0, [], []
        for i, g in gold.items():
            r = rust.get(i)
            if r is None:
                missing.append(i)
                continue
            d = diffs(g, r)
            if d:
                diff.append((i, g["text"], d))
            else:
                same += 1
        extra = sorted(set(rust) - set(gold))
        print(f"== {kind}: {same} identical, {len(diff)} differ, "
              f"{len(missing)} missing in rust, {len(extra)} extra")
        for i, text, d in diff[:show]:
            print(f"  #{i} {text[:40]}")
            for line in d[:6]:
                print(f"      {line}")


if __name__ == "__main__":
    main()
