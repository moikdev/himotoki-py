#!/usr/bin/env python3
"""Python-side benchmark harness — mirrors `himotoki bench --json-out`.

Usage:
    python3 bench_py.py --inputs corpus.jsonl --rounds 3 --json-out py.json

Emits {"impl","inputs","rounds","segments","per_input_ms","peak_rss_mb"}.
"""

import argparse
import json
import os
import resource
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--inputs", required=True)
    ap.add_argument("--rounds", type=int, default=1)
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("--json-out", required=True)
    ap.add_argument("--db", default=None)
    args = ap.parse_args()

    texts = []
    for line in Path(args.inputs).read_text(encoding="utf-8").splitlines():
        if line.strip():
            texts.append(json.loads(line)["text"])
            if args.limit and len(texts) >= args.limit:
                break

    if args.db:
        os.environ["HIMOTOKI_DB_PATH"] = args.db

    import himotoki
    from himotoki.db.connection import get_session
    from himotoki.segment import segment_text

    t0 = time.perf_counter()
    session = get_session(args.db)
    himotoki.warm_up()
    warm = time.perf_counter() - t0
    print(f"warm_up: {warm:.2f}s", file=sys.stderr)

    nsegs = 0
    round_times = []
    for _ in range(args.rounds):
        times = []
        for t in texts:
            s = time.perf_counter()
            paths = segment_text(session, t, 5)
            times.append((time.perf_counter() - s) * 1000.0)
            nsegs += len(paths)
        round_times.append(times)

    total = sum(sum(r) for r in round_times)
    n = len(texts) * args.rounds
    print(
        f"bench: {len(texts)} inputs x {args.rounds} rounds, {nsegs} segs, "
        f"{total/1000:.3f}s total, {total/n:.1f} ms/input",
        file=sys.stderr,
    )

    peak_rss_mb = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024.0
    with open(args.json_out, "w", encoding="utf-8") as f:
        json.dump(
            {
                "impl": "python",
                "inputs": len(texts),
                "rounds": args.rounds,
                "segments": nsegs,
                "per_input_ms": round_times,
                "peak_rss_mb": peak_rss_mb,
                "warm_s": warm,
            },
            f,
        )


if __name__ == "__main__":
    main()
