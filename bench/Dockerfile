# Same-environment benchmark for himotoki Python vs the Rust port.
# Both implementations run against the same corpus + same SQLite DB file,
# in the same container (sequential processes, identical CPU/mem cgroup).
#
# Build:  docker build -f bench/Dockerfile -t himotoki-bench .
# Run:    docker run --rm himotoki-bench
#         ROUNDS=5 docker run --rm -e ROUNDS=5 himotoki-bench
#         docker run --rm -v $PWD/out:/app/out himotoki-bench

FROM rust:1-slim-bookworm AS rust-build
RUN apt-get update && apt-get install -y --no-install-recommends gcc \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY himotoki-rs /build/himotoki-rs
RUN cd himotoki-rs && cargo build --release -p himotoki-cli

FROM python:3.12-slim-bookworm
# gcc for marisa-trie if no aarch64 wheel is published for this version
RUN apt-get update && apt-get install -y --no-install-recommends gcc g++ \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY pyproject.toml README.md /app/
COPY himotoki /app/himotoki
RUN pip install --no-cache-dir .
COPY data/himotoki.db /app/data/himotoki.db
COPY bench /app/bench
COPY --from=rust-build /build/himotoki-rs/target/release/himotoki /usr/local/bin/himotoki-rs
RUN chmod +x /app/bench/run.sh
CMD ["bash", "/app/bench/run.sh"]
