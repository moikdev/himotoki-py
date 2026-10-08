#!/usr/bin/env bash
# Benchmark runner — executed inside the container.
# Runs Rust + Python harnesses on the same corpus/DB, then summarizes.
set -euo pipefail
cd /app

ROUNDS="${ROUNDS:-3}"
mkdir -p /app/out

echo "=== himotoki-rs bench (rust) ==="
himotoki-rs -d /app/data/himotoki.db bench \
    --inputs /app/bench/corpus.jsonl \
    --rounds "$ROUNDS" \
    --json-out /app/out/rust.json

echo "=== himotoki bench (python) ==="
python3 /app/bench/bench_py.py \
    --inputs /app/bench/corpus.jsonl \
    --rounds "$ROUNDS" \
    --json-out /app/out/python.json \
    --db /app/data/himotoki.db

echo "=== comparison ==="
python3 /app/bench/summarize.py \
    /app/out/rust.json /app/out/python.json /app/bench/corpus.jsonl \
    | tee /app/out/summary.txt
