#!/usr/bin/env python3
"""Compare rust.json + python.json bench outputs, with per-category stats.

Usage: summarize.py rust.json python.json corpus.jsonl
"""

import json
import statistics as st
import sys
from collections import defaultdict


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def pct(vals, p):
    if not vals:
        return 0.0
    vals = sorted(vals)
    k = (len(vals) - 1) * p / 100
    f = int(k)
    c = min(f + 1, len(vals) - 1)
    return vals[f] + (vals[c] - vals[f]) * (k - f)


def describe(vals):
    return {
        "n": len(vals),
        "mean": st.fmean(vals),
        "median": st.median(vals),
        "p95": pct(vals, 95),
        "p99": pct(vals, 99),
        "min": min(vals),
        "max": max(vals),
        "total_s": sum(vals) / 1000.0,
    }


def flat_best_round(rec):
    """Per-input time = min across rounds (least noise)."""
    rounds = rec["per_input_ms"]
    if not rounds:
        return []
    best = rounds[0]
    for r in rounds[1:]:
        best = [min(a, b) for a, b in zip(best, r)]
    return best


def main():
    rust = load(sys.argv[1])
    py = load(sys.argv[2])
    cats = []
    if len(sys.argv) > 3:
        with open(sys.argv[3], encoding="utf-8") as f:
            cats = [json.loads(l)["cat"] for l in f if l.strip()]

    out = {}
    for name, rec in [("rust", rust), ("python", py)]:
        best = flat_best_round(rec)
        d = describe(best)
        d["peak_rss_mb"] = rec.get("peak_rss_mb", 0)
        d["rounds"] = rec["rounds"]
        d["segments"] = rec.get("segments")
        d["per_round_s"] = [round(sum(r) / 1000.0, 2) for r in rec["per_input_ms"]]
        out[name] = d
        if cats:
            by_cat = defaultdict(list)
            for c, t in zip(cats, best):
                by_cat[c].append(t)
            d["by_cat"] = {c: describe(v) for c, v in sorted(by_cat.items())}

    print(json.dumps(out, indent=1))

    r, p = out["rust"], out["python"]
    print("\n=== summary ===")
    print(f"{'':12}{'rust':>12}{'python':>12}{'ratio(py/rs)':>14}")
    for k in ("mean", "median", "p95", "p99", "max"):
        ratio = p[k] / r[k] if r[k] else 0
        print(f"{k:12}{r[k]:>10.1f}ms{p[k]:>10.1f}ms{ratio:>13.2f}x")
    print(f"{'total_s':12}{r['total_s']:>11.1f}s{p['total_s']:>11.1f}s"
          f"{p['total_s']/r['total_s']:>13.2f}x")
    print(f"{'peak_rss':12}{r['peak_rss_mb']:>10.0f}MB"
          f"{p['peak_rss_mb']:>10.0f}MB{r['peak_rss_mb']/max(p['peak_rss_mb'],1):>13.2f}x")
    if cats:
        print("\n=== by category (best-round ms/input) ===")
        catset = sorted(set(cats))
        print(f"{'cat':10}{'n':>5}{'rust_mean':>11}{'py_mean':>10}"
              f"{'rs_p95':>9}{'py_p95':>9}{'ratio':>8}")
        for c in catset:
            rc = r["by_cat"].get(c)
            pc = p["by_cat"].get(c)
            if rc and pc:
                print(f"{c:10}{rc['n']:>5}{rc['mean']:>10.1f}{pc['mean']:>10.1f}"
                      f"{rc['p95']:>9.1f}{pc['p95']:>9.1f}"
                      f"{pc['mean']/rc['mean']:>7.2f}x")


if __name__ == "__main__":
    main()
