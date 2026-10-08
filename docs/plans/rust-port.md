# Rust Port Plan — Himotoki

Port `himotoki` (Python, ~18.7k LOC library code) to Rust. The Rust crate lives
in this repo under `himotoki-rs/` and is developed against the **same SQLite
dictionary** (`data/himotoki.db`), which the Python build pipeline already
produces. Goal: bit-for-bit identical segmentations, scores, and CLI/JSON
output.

This document is the master plan. Each phase lists source files, target files,
tasks, and an exit gate. The Python implementation is the oracle throughout —
port file-by-file, keep names aligned, diff constantly.

---

## 1. Goals & non-goals

**Goals**
- `himotoki` Rust crate exposing `analyze(text) -> Vec<(Vec<WordInfo>, f64)>`
  plus `warm_up`, mirroring `himotoki/__init__.py`.
- `himotoki` CLI binary matching `himotoki/cli.py` flags: `-r/-f/-k/-j`,
  `-l/--limit`, `-d/--database`, `-v`, `setup`, `init-db`.
- Exact parity with Python: same winning paths, same scores, same WordInfo
  JSON. Verified against a golden corpus (§8).
- Comparable or better performance; keep the DB-compatible file formats so
  Python and Rust can share `~/.himotoki/`.

**Non-goals (initially)**
- Porting `scripts/` eval tooling (`llm_eval.py` etc.) — stays Python.
- Porting the DB *build* pipeline (JMdict parse + conjugation generation +
  errata). Phase 8, optional: Python builds the DB; Rust consumes it.
- PyO3 bindings (Phase 7, optional).
- Reimplementing `build/lib/` (stale setuptools artifacts — ignore).

---

## 2. Source inventory (what we're porting)

Runtime library, `himotoki/` (excludes `build/` copies and `scripts/`):

| Python source | LOC | Complexity | Rust target | Notes |
|---|---|---|---|---|
| `characters.py` | 682 | Low | `core/src/chars.rs` | Pure fns: char classes, kana conv, rendaku/geminate, `mora_length`, `romanize_word`, `basic_split`, regexes → hand-rolled ranges or `regex` |
| `constants.py` | 373 | Low | `core/src/constants.rs` | POS_TAGS, conj type IDs, SEQ_* constants, NOUN_PARTICLES |
| `raw_types.py` | 44 | Trivial | `core/src/db/rows.rs` | `RawKanaReading`, `RawKanjiReading` structs |
| `types.py` | 370 | Medium | `core/src/types.rs` | `Word` enum (Simple/Compound/Counter), `Segment`, `SegmentList`, `ConjData`, `adjoin_word` |
| `conjugation_hints.py` | 280 | Low | `core/src/hints.rs` | Static tables → `phf`/static maps |
| `trie.py` | 149 | Low | `core/src/index.rs` | `WordIndex` trait + `fst::Set` impl; `.fst` file beside DB |
| `db/connection.py` | 365 | Medium | `core/src/db/mod.rs` | rusqlite conn, pragmas, path resolution, cache helpers |
| `db/models.py` | 319 | Low | `core/src/db/rows.rs` | Row structs for 10 tables |
| `lookup/find_word.py` | 196 | Medium | `core/src/lookup.rs` | `find_word*`, `find_word_as_hiragana` |
| `lookup/conj_data.py` | 265 | Medium | `core/src/conj.rs` | `get_conj_data`, `get_word_conj_data`, BLOCKED_CONJUGATIONS |
| `lookup/constants.py` | 27 | Low | `core/src/conj.rs` | WEAK/SKIP_CONJ_FORMS |
| `scoring/caches.py` | 363 | Medium | `core/src/cache.rs` | LRU + archaic/uk/posi sets → fields on `Ctx` |
| `scoring/calc_score.py` | 993 | **High** | `core/src/score.rs` | Core heuristic; exact float/int op replication |
| `segment.py` | 739 | **High** | `core/src/segment.rs` | Sticky positions, substring enum, `TopArray`, `find_best_path`, `get_segment_splits` |
| `grammar/synergies.py` | 313 | Medium | `core/src/grammar/synergy.rs` | Registry, `Synergy`, `apply_segfilters` |
| `grammar/synergy_filters.py` | 261 | Medium | `core/src/grammar/filters.rs` | Filter combinators → `Box<dyn Fn>` factories |
| `grammar/synergy_rules.py` | 2078 | **High** | `core/src/grammar/rules.rs` | ~150 rule registrations; mostly declarative |
| `grammar/suffixes.py` | 1157 | **High** | `core/src/grammar/suffix.rs` | Suffix cache init, `find_word_suffix` (depth≤5 recursion), `could_have_suffix` |
| `grammar/suffix_handlers.py` | 619 | **High** | `core/src/grammar/handlers.rs` | ~35 handlers → `match` on suffix class |
| `grammar/counters.py` | 914 | Medium | `core/src/grammar/counter.rs` | Kanji/arabic number parsing, `counter_join` phonetics, `CounterText` |
| `grammar/splits.py` | 713 | Medium | `core/src/grammar/split.rs` | Split registry, `get_split`, `get_segsplit` |
| `output/types.py` | 127 | Low | `core/src/output/mod.rs` | `WordInfo`, `ConjStep`, `WordType` |
| `output/word_info.py` | 524 | Medium | `core/src/output/word_info.rs` | `fill_segment_path`, word-info builders |
| `output/meanings.py` | 747 | Medium | `core/src/output/meanings.rs` | Glosses/POS/senses queries |
| `output/conjugation_display.py` | 628 | Medium | `core/src/output/conj_display.rs` | Conj tree strings |
| `output/format.py` | 278 | Low | `core/src/output/format.rs` | `dict_segment`, `segment_to_json/text` |
| `__init__.py` | 363 | Low | `core/src/lib.rs` | `analyze`, `warm_up`, `shutdown`; NFC, length checks |
| `cli.py` | 458 | Low | `cli/src/main.rs` | clap port |
| **Phase 8 (optional)** | | | | |
| `loading/jmdict.py` | 467 | Medium | `load/src/jmdict.rs` | quick-xml streaming |
| `loading/conjugations.py` | 1786 | **High** | `load/src/conj.rs` | conjo.csv rule engine, batch insert, rayon |
| `loading/errata.py` | 1302 | Medium | `load/src/errata.rs` | SQL mutations |
| `setup.py` | 451 | Medium | `cli` setup cmd / `load` | Download JMdict, orchestrate build |

Static data inputs the Rust side also reads: `data/conj.csv` (14 rows),
`data/conjo.csv` (1138 conj rules), `data/kwpos.csv` (93 POS tags),
`data/himotoki.db` (1.8 GB), `data/himotoki.trie` (25 MB marisa — optional
input; Rust prefers its own `.fst`).

---

## 3. Architecture decisions

### 3.1 Shared SQLite database — don't port the loader first

`himotoki.db` is a plain SQLite file. `rusqlite` (bundled feature) opens it
with the same pragmas (`db/connection.py:92-99`):

```text
PRAGMA foreign_keys=ON; journal_mode=WAL; cache_size=-64000;
mmap_size=268435456; temp_store=MEMORY; synchronous=NORMAL;
```

DB path resolution mirrors `_get_default_db_path()` +
`get_db_path()`: `HIMOTOKI_DB_PATH`/`HIMOTOKI_DB` env → `~/.himotoki/himotoki.db`
→ `<repo>/data/himotoki.db`. This defers the ~3.5k-LOC loader port to an
optional late phase and guarantees data parity (same bytes, same seqs — the
codebase embeds JMdict seq numbers in constants everywhere).

### 3.2 Char indices, not bytes

Python slices by code point: `text[start:end]`, `find_sticky_positions`
returns char indices, `Segment.start/end` and `WordInfo.start/end` are char
indices, and `MAX_WORD_LENGTH = 50` counts chars. In Rust:

- Convert input once to `Vec<char>` (call it `Chars`) inside the segmenter.
- All substring materialization goes through `chars[start..end].iter().collect::<String>()`.
- Keep public API strings UTF-8; indices in results remain char positions to
  match Python output JSON.

### 3.3 Word polymorphism → enums

Python duck-types `WordMatch | CompoundWord | CounterText` and
`KanjiText | KanaText | Raw*Reading`. Rust:

```rust
enum Reading { Kanji(KanjiTextRow), Kana(KanaTextRow) }        // one row type each
enum Word { Simple(WordMatch), Compound(Box<CompoundWord>), Counter(Box<CounterText>) }
enum ConjIds { Root, List(Vec<i64>) }   // replaces Optional[List[int] | 'root']

impl Word {
    fn seq(&self) -> Option<i64>;
    fn text(&self) -> &str;
    fn reading(&self) -> Option<&Reading>;   // Counter's `source`
    fn ord(&self) -> i64;
    fn common(&self) -> Option<i64>;
    fn word_type(&self) -> WordType;          // Kanji | Kana
    fn conjugations(&self) -> Option<&ConjIds>;
    fn is_root(&self) -> bool;
    fn is_compound(&self) -> bool;
    fn components(&self) -> Vec<&str>;
    fn score_base(&self) -> &Word;
}
```

`adjoin_word` (types.py:246) mutates `CompoundWord` in place when extending —
use a `CompoundWord::push_word` equivalent; `score_mod` is always `f64` or a
list accumulated per-join (`SUFFIX_SCORES` / `get_sou_score`, suffixes.py:948-950)
→ model as `Vec<f64>`. The callable branch in `apply_score_mod`
(calc_score.py:854) is dead code — no caller passes a lambda; skip it but leave
a comment noting the omission.

Path elements are also heterogeneous: `find_best_path` payloads and
`get_segment_splits` results mix `SegmentList`, `Segment`, `Synergy`:

```rust
enum PathNode { List(Rc<SegmentList>), Seg(Rc<Segment>), Syn(Synergy) }
```

`Rc` because nodes are shared across candidate paths; `find_best_path`
clones path vecs per registration.

### 3.4 `info` dict → typed `ScoreInfo`

Key census (rg across `himotoki/`): `posi`, `seq_set`, `conj`, `common`,
`score_info`, `kpcl`, `counter`, plus `conj_type`/`neg`/`fml`/`source_text`
written later by the output layer.

```rust
struct ScoreInfo {
    posi: IndexSet<String>,                  // 'n','prt',... — see §3.7 ordering
    seq_set: HashSet<i64>,
    conj: Vec<ConjData>,
    common: Option<i64>,
    score_info: (f64, Vec<usize>, f64, Option<SplitInfo>),
    kpcl: Kpcl,                              // [kanji_or_katakana, primary, common, long]
    counter: bool,
    conj_type: Option<String>, neg: bool, fml: bool, source_text: Option<String>,
}
```

Consumers: `synergy_filters.rs` (`posi`, `seq_set`, `kpcl`, `conj`),
`splits.rs` (`seq_set`), `output/*` (`conj`, `conj_type`, `neg`, `fml`,
`source_text`). `Segment` keeps `info: ScoreInfo` + `filter_cache:
HashMap<u32,bool>` (ports `_filter_cache`, types.py:333).

### 3.5 Caches and registries live on a context object, not globals

Python uses module-level singletons (`_WORD_TRIE`, `_suffix_cache`,
`_ARCHAIC_CACHE`, `_session_factory`). Rust:

```rust
pub struct Himotoki {
    conn: Connection,
    trie: fst::Set,                     // or WordIndex impl
    caches: Caches,                     // RefCell'd LRU/sets (see below)
    grammar: GrammarRegistry,           // synergies, segfilters, penalties, splits, suffix cache
}
```

- Every function that took `session: Session` takes `&Himotoki` (or `&Ctx`).
- Interior mutability via `RefCell` for LRU caches — core is `!Sync`
  (SQLite `Connection` isn't `Sync` anyway). Thread-safe wrapper =
  `Mutex<Himotoki>` or `r2d2_sqlite` pool; Python already serializes on one
  connection, so this matches behavior.
- `warm_up()` (init order in `__init__.py:54`) becomes
  `Himotoki::warm_up()`: session → archaic cache → suffix cache → counter
  cache → word index, same timings dict for the CLI.

### 3.6 Trie → `fst::Set` (with optional marisa interop)

Python's trie is a pure membership/prefix filter over `kana_text.text ∪
kanji_text.text` (trie.py:60-66). Rust default: `fst::Set`

- Build: same UNION query → `SetBuilder` → save as `himotoki.fst` next to the
  DB; reuse file when `fst.mtime >= db.mtime` (same freshness rule as
  trie.py:90-101).
- Membership: `set.contains(part)`. (`trie_has_prefix` is dead code in
  Python — zero callers — so `fst` needs only `contains`; if a prefix query
  is ever wanted, `Str::new(prefix).starts_with()` automaton covers it.)
- Alternative if we want to read the existing 25 MB `himotoki.trie`: the
  `marisa` crate binding — optional feature, not default (avoids C++ dep).
- Fallback parity: Python degrades to "assume true" when trie is None —
  mirror with `Option<WordIndex>` semantics until index is built.

### 3.7 Float & ordering parity rules

- `calc_score` mixes ints and floats (`score // ratio` at calc_score.py:302
  is **floor division on a float** → `(score / 2.0).floor()`; `math.ceil` at
  :638 → `.ceil()`; `**` at :145 → `powi/powf`). Keep `score: f64`, replicate
  every operator verbatim. No fast-math, no reordering.
- `common` is `Option<i64>` where `None`/`0`/`>0` have distinct semantics
  (`compare_common`, calc_score.py:231).
- JSON: Python emits ints where values are integral (`score: int` in
  WordInfo). Serialize score as integer when `fract()==0` for byte-identical
  JSON, or accept the diff — decide in Phase 6.
- Python `set` iteration is non-contractual; anywhere order can leak to
  output use `IndexSet`/`BTreeSet` and replicate explicit `sorted(...)`
  calls (`conj_of_common` sort at calc_score.py:530 — key `(c or 1000,
  c==0)`; `cull_segments` sort at :951 — key `(-score, common or inf)`;
  `lex_compare` in loading).
- `TopArray.register` (segment.py:73) is a bounded insertion keeping
  insertion order on ties — port literally; equal-score path ordering is
  observable in output.

### 3.8 Grammar registries

`synergy_rules.py` registers ~150 entries through `register_synergy`,
`def_generic_synergy`, `register_penalty`, `def_generic_penalty`,
`register_segfilter`, `def_segfilter_must_follow`, plus hand-written
closures (e.g. `synergy_noun_particle` with the と+は exclusion).

Rust shape:

```rust
type SegFn = Box<dyn Fn(&Ctx, Option<&PathNode>, &SegmentList) -> Vec<(Option<PathNode>, SegmentList)>>;
struct GrammarRegistry {
    synergies: Vec<SynergyFn>,    // fn(left_list, right_list) -> Vec<(SegmentList, Synergy, PathNode)>
    penalties: Vec<PenaltyFn>,
    segfilters: Vec<SegFn>,
    splits: HashMap<i64, SplitFn>,
    suffix_cache: SuffixCache,
}
```

Filter factories (`filter_in_seq_set`, `filter_is_pos`, `filter_is_noun`,
`filter_short_kana`, `filter_is_compound_end`, `filter_is_conjugation`)
return `Box<dyn Fn(&Segment) -> bool>`; `cached_filter` uses the segment's
`filter_cache`. Build the registry in `GrammarRegistry::init(ctx)` —
one-to-one with `_init_synergies/_init_penalties/_init_segfilters`.

`suffix_handlers.py` (~35 `_handler_*` fns): dispatch via
`match suffix_class { "tai" => handler_tai(..), ... }` since handlers are
looked up by suffix-class string in `find_word_suffix`.

`find_word_suffix` recursion depth uses `contextvars`
(suffixes.py:149) → carry a `depth: u8` parameter in Rust instead.

### 3.9 Crate layout

```
himotoki-rs/
├── Cargo.toml                  # workspace
├── crates/
│   ├── himotoki-core/          # lib: everything except cli/loader
│   ├── himotoki-cli/           # bin `himotoki` (clap)
│   ├── himotoki-load/          # Phase 8: DB builder bin+lib
│   ├── himotoki-py/            # Phase 7: PyO3 (feature `python`)
│   └── xtask/                  # golden gen/check, corpus diff, bench
└── tests/golden/               # generated JSONL + harness
```

Module tree inside `himotoki-core` mirrors the Python package 1:1 (see §2
target column) so any line can be cross-referenced during review.

### 3.10 Dependencies (pin conservative versions)

| Need | Crate |
|---|---|
| SQLite | `rusqlite` (features `bundled`, optional `blob`/limits) |
| Index | `fst` |
| Unicode NFC | `unicode-normalization` |
| Sets/maps | `indexmap`/`indexset`, `rustc-hash` |
| JSON | `serde`, `serde_json` |
| CLI | `clap` (derive) |
| LRU | `lru` or hand-rolled (needs get_mut semantics) |
| Lazy | `std::sync::OnceLock` |
| HTTP download (setup) | `ureq` or `reqwest`+rustls |
| XML (Phase 8) | `quick-xml` |
| Parallel load (Phase 8) | `rayon` |
| PyO3 (Phase 7) | `pyo3` + `maturin` |
| Regex (only if hand-rolled ranges prove messy) | `regex` |

---

## 4. Phases

Dependency order is strict through Phase 5; phases 6-9 can reorder/parallelize.

### Phase 0 — Scaffolding & golden corpus (gate: harness runs)

- [ ] `cargo init` workspace at `himotoki-rs/`, crates per §3.9, CI job.
- [ ] `scripts/dump_gold.py`: for each input, dump per-candidate and
  per-path JSONL (two files):
  - `candidates.jsonl`: `{input, matches: [{start,end,text,seq,ord,common,word_type,score,conj_ids}]}`
    from `join_substring_words` (pre-DP state).
  - `paths.jsonl`: `{input, paths: [{score, segments: [{start,end,text,seq,kana}]}]}` via `segment_text` + `fill_segment_path` + `segment_to_json`.
- [ ] Corpus assembly (`tests/golden/inputs.txt`): `scripts/test_sentences.py`
  TEST_SENTENCES_500 (500), `output/llm_results.json` sentences (510),
  sentences embedded in `tests/test_*.py`, plus edge cases: empty-adjacent,
  all-katakana, number runs, ん-contractions, long-vowel endings, ASCII
  mixes. Target ≥1100 inputs.
- [ ] `xtask gold-check` skeleton that replays JSONL and diffs (fill in as
  stages land).

**Exit:** `python scripts/dump_gold.py` produces both JSONLs; Rust harness
parses them.

### Phase 1 — Pure data layer (no DB)

Port order + unit tests (port `tests/test_characters.py`, `test_trie.py`
logic):

- [ ] `chars.rs` — `get_char_class` (KANA_CHARS map), `word_matches_class`,
  `count_char_class`, `is_katakana/hiragana/kanji/kana`, `has_kanji`,
  `as_hiragana`, `as_katakana`, `rendaku`, `unrendaku`, `geminate`,
  `normalize`, `basic_split`, `mora_length`, `sequential_kanji_positions`,
  `kanji_prefix`, `kanji_mask`, `kanji_match`, `safe_subseq`,
  `romanize_word`, HALF_WIDTH/FULL_WIDTH maps, PUNCTUATION_MAP,
  ABNORMAL/NORMAL chars.
- [ ] `constants.rs` — conj IDs (two numbering systems! models.py:295-310
  DB-side vs constants.py:73-88 rule-side), SEQ_* table, POS_TAGS
  (`&'static str` suffices — interning is a perf detail), NOUN_PARTICLES,
  SPECIAL_CONJ_INFO, SUPPRESS_* sets.
- [ ] `types.rs` — §3.3 enums + `Segment`, `SegmentList`, `ConjData`,
  `adjoin_word`, `CounterText` struct fields (counters.py:251).
- [ ] `hints.rs` — COMPOUND_PHRASES + suffix tables → static maps.
- [ ] Numeric utilities: `parse_number`, `parse_kanji_number`,
  `number_to_kana`, `counter_join` phonetics — port early, they're pure.

**Exit:** `cargo test` on ported unit tests; `mora_length`/`romanize`/
`basic_split` fuzzed against Python over random strings (dump via a small
Python script for a few thousand inputs — cheap oracle).

### Phase 2 — Storage layer

- [ ] `db/rows.rs` — row structs for all 10 tables (models.py), plus
  `RawKanaReading`/`RawKanjiReading` column order (`id, seq, text, ord,
  common, best_*`).
- [ ] `db/mod.rs` — `open(path)`, pragmas, path resolution, `session_scope`
  equivalent (`with_conn`).
- [ ] `index.rs` — `WordIndex` trait (`contains`, `has_prefix`), `FstIndex`
  impl, `himotoki.fst` build + mtime freshness, `--build-index` xtask.
- [ ] Verify: row-level parity test — for ~200 sampled texts, assert
  `SELECT ... WHERE text IN (...)` returns identical tuples in both impls
  (compare via Python dump).

**Exit:** Rust can answer "does this surface exist" and fetch reading rows.

### Phase 3 — Lookup & scoring (core heuristic)

- [ ] `lookup.rs` — `find_word`, `find_word_as_hiragana` (katakana→hiragana
  fallback), `find_word_full`, `find_word_with_conj_prop/type`.
- [ ] `conj.rs` — `get_conj_data` (batched, `conj_ids`/`texts`/`from_seq`
  filters), `get_word_conj_data`, BLOCKED_CONJUGATIONS, WEAK/SKIP forms,
  `matches_conj_form`.
- [ ] `cache.rs` — `LRUCache` (caches.py:15), `preload_scoring_caches`
  (batch Entry/UK/POS), `build_archaic_cache`, `is_arch`,
  `is_prefer_kana`, `get_non_arch_posi`, `get_cached_entry`.
- [ ] `score.rs` — **the big one**: `calc_score` (calc_score.py:305-697)
  including compound recursion, `determine_primary_full`,
  `get_original_text_data` (recursive via-chain walk :771-845),
  `apply_score_mod`, `kanji_break_penalty`, `length_multiplier*`,
  `cull_segments`, `gen_score`, `gap_penalty`, all constant tables.
- [ ] Split hooks: `get_split` is called inside `calc_score` — port
  `grammar/split.rs` registry skeleton now (splits.py:74-160, 285-360;
  `init_splits` table port can be staged but must land before Phase 5
  gate).

**Exit:** candidate-score parity — `xtask gold-check --stage=candidates`
diffs `candidates.jsonl`; every candidate's `score` field must match exactly.
This gate isolates scoring bugs from DP bugs.

### Phase 4 — Grammar subsystem

- [ ] `filters.rs` — all `filter_*` combinators + `cached_filter`.
- [ ] `synergy.rs` — `Synergy`, registries, `get_synergies`, `get_penalties`,
  `apply_segfilters` (pair filtering/mutation semantics, synergies.py:296).
- [ ] `rules.rs` — port `_init_synergies`, `_init_penalties`,
  `_init_segfilters` literally; each `def_generic_*` → same-shaped Rust
  call; hand closures → `move` closures.
- [ ] `suffix.rs` — SUFFIX_DESCRIPTION/class tables, `init_suffixes`
  (loads kana forms + conjugated kana forms, builds `_suffix_cache` /
  `_suffix_ending_chars` / `_suffix_class` / `_suffix_text_class`),
  `get_suffix_map`, `get_suffixes`, `could_have_suffix`,
  `find_word_suffix` (recursive, depth limit, `match_unique`),
  `get_sou_score`, `find_word_with_*` helpers.
- [ ] `handlers.rs` — all `_handler_*` incl. abbreviation handlers
  (`abbr_nai`, `abbr_eba`, `abbr_ii`, …) and `_find_word_with_neg_prop_filtered`.
- [ ] `counter.rs` — `find_counter`, `find_counter_in_text`,
  `calc_counter_score`, `init_counter_cache`, COUNTER_* tables,
  DAYS/PEOPLE kun readings.
- [ ] `split.rs` — full `init_splits` table, `def_simple_split`,
  `def_de_split`, `def_toori_split`, `get_segsplit`,
  `_create_split_segment`.

**Exit:** candidates parity still green (suffix compounds and counters now
appear in candidate lists); new diff volume explains itself (previously
missing compound candidates).

### Phase 5 — Segmentation DP (first end-to-end milestone)

- [ ] `segment.rs` — `find_sticky_positions` (incl. the かーい long-vowel
  fix, segment.py:135-147), `is_long_vowel_modifier`,
  `consecutive_char_groups`, `find_substring_words` (trie filter + batched
  `IN` queries + suffix pass), `join_substring_words_impl`,
  `join_substring_words` (preload → `gen_score` → cutoff → cull → compound
  reorder :497-503), `TopArray`, `get_segment_score`,
  `get_initial_segments`, `get_segment_splits`, `find_best_path`,
  `segment_text`, `simple_segment`.
- [ ] `lib.rs` — `analyze` (NFC via `unicode-normalization`,
  `MAX_TEXT_LENGTH`, limit), `warm_up`, errors mirroring Python exceptions.

**Exit (M5, the big gate):** `paths.jsonl` parity — identical winner paths
(segment texts AND scores) across the whole corpus. Tolerate only diffs
traced to documented Python nondeterminism (fix by pinning order, §3.7).

### Phase 6 — Output & CLI

- [ ] `output/word_info.rs` — `word_info_from_*`, `fill_segment_path`
  (gap handling → `WordType::Gap`).
- [ ] `output/meanings.rs` — senses/gloss queries, POS string `[n,vs,vt]`,
  `populate_meanings`, conj info JSON, copula compound split
  (`_split_copula_compound_for_output`), `ReadingsCache`.
- [ ] `output/conj_display.rs` — conjugation tree formatting.
- [ ] `output/format.rs` — `dict_segment`, `segment_to_json`,
  `segment_to_text`.
- [ ] `cli/src/main.rs` — all flags + `init-db`; `setup` may initially shell
  out to Python loader or print guidance (Phase 8 replaces).

**Exit:** `himotoki "…"` text/JSON output byte-diffed vs Python CLI on the
corpus (modulo decided JSON int/float normalization).

### Phase 7 — PyO3 bindings (optional)

- [ ] `himotoki-py`: `py analyze(text) -> list` using PyO3 + `maturin`;
  lets the *existing pytest suite* run against Rust (`HIMOTOKI_IMPL=rust`).

### Phase 8 — DB build pipeline (optional but recommended for standalone Rust)

- [ ] `load/src/jmdict.rs` — streaming JMdict parse (quick-xml), entity
  expansion (parse_entity_definitions jmdict.py:28).
- [ ] `load/src/conj.rs` — conjo.csv rule engine, conjugate_word,
  secondary conjugations, `_find_existing_seq_for_readings` dedup, bulk
  insert (rayon replaces multiprocessing).
- [ ] `load/src/errata.rs` — every `apply_*`/`add_*` (errata.py) — must
  produce identical row sets.
- [ ] `cli setup` — download JMdict, run build, ANALYZE+VACUUM, build fst.

**Exit:** Rust-built DB passes row-count + checksum comparison vs
Python-built DB per table; then full corpus re-diffed on Rust-built DB.

### Phase 9 — Performance & release

- [ ] `xtask bench` vs `scripts/benchmark.py` corpus; target ≥ Python speed
  cold and warm (expect much faster — Python spends most time in DP/scoring).
- [ ] Cache tuning, `release` profile (lto, codegen-units=1), optional mmap
  for fst (fst is mmap-native already).
- [ ] `analyze_async` equivalent: `tokio::task::spawn_blocking` wrapper +
  connection pool for servers.
- [ ] Docs: README for `himotoki-rs`, publish prep (crate name availability),
  migration notes.

---

## 5. Verification strategy (the contract)

Equivalence definition (from `.cursor/docs/SYSTEM_WALKTHROUGH.md` §5):
identical candidates + identical `calc_score` + identical DP winners —
not merely "valid Japanese".

1. **Golden corpus** (Phase 0): `candidates.jsonl` gates Phases 3-4,
   `paths.jsonl` gates Phase 5, CLI JSON gates Phase 6.
2. **Stage-diff tooling**: `xtask gold-check --stage=candidates|paths|output`
   reports first-diverging input with a minimal repro.
3. **Unit ports**: `tests/test_characters.py`, `test_trie.py`,
   `test_score_equivalence.py`, property tests (`test_*_properties.py` —
   hypothesis → `proptest` mirroring the same invariants).
4. **Differential fuzz**: `xtask fuzz` — random strings over
   WORD_PATTERN/NUM_WORD_PATTERN alphabets → Python dumps paths → Rust
   compares. Run N≥10k before Phase 5 sign-off.
5. **Regression lock**: any discovered divergence → fix or file bd issue,
   add the sentence to `tests/golden/inputs.txt` permanently.
6. **CI**: pytest (Python, unchanged) + cargo test + golden check (needs
   `data/himotoki.db`; gate behind `#[ignore]`/env flag like conftest's
   skip-if-missing, tests/conftest.py:36).

**Tie-break/determinism rule:** when a diff appears, first check whether
Python relied on hash/set ordering; if so make Rust deterministic and note
the (rare) acceptable delta — document each in `tests/golden/KNOWN_DIFFS.md`.

---

## 6. Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Score float divergence (`//`, `**`, `ceil`) | High | §3.7 verbatim ops; per-candidate diff at Phase 3 catches early |
| Duck-typed `info`/word attrs missed | Medium | Key census done (§3.4); re-grep `info.get(` each phase |
| `adjoin_word` mutation semantics (compounds mutate in place) | Medium | Model `CompoundWord` with owned `Vec<Word>`; audit aliasing — Python shares the object, Rust may need `Rc<RefCell>` if any code observes mid-mutation state (grep `adjoin_word` callers in suffix handlers) |
| Synergy/segfilter semantic drift (they filter/rewrite SegmentLists) | Medium | Port `apply_segfilters` first, test pairs on corpus extracts before rules land |
| Trie filter subtlety: Python checks `part in trie` (exact) not prefix-pruned enumeration | Low | Keep identical enumeration (`all_substrings` + membership), fst `contains` |
| DB/schema drift (seq constants JMdict-version-sensitive, constants.py warning) | Medium | Sharing `himotoki.db` makes this a non-issue for the port; document `verify_seq_constants` equivalent as a Rust sanity test |
| Loader port (Phase 8) seq-number instability | High (if attempted) | Must reproduce `_find_existing_seq_for_readings` dedup exactly; checksum-compare every table; otherwise keep Python loader |
| String indexing bugs (bytes vs chars) | Medium | `Chars` abstraction §3.2, `#[deny]` clippy + proptest on index round-trips |
| marisa file unreadable → reindex cost | Low | fst build is one-time (~9M keys); store beside DB like Python does |

---

## 7. Open questions (decide before Phase 0/1)

1. Crate name: `himotoki` on crates.io availability vs `himotoki-rs`.
2. Threading model for servers: is `Mutex<Himotoki>` enough or do we need
   `r2d2` pool now? (Default: Mutex + document pool recipe.)
3. JSON int-vs-float for `score` — byte-identical vs cleaner JSON (Phase 6
   decision).
4. Keep `himotoki.trie` (marisa) compat, or `.fst` only? (Default: fst only;
   add marisa reader only if cross-impl dir sharing matters.)
5. Should `himotoki-cli setup` shell to Python until Phase 8, or stub it?
   (Default: stub with clear message; Phase 8 fills in.)

---

## 8. Fast orientation for the porting agent

- Hot path: `find_substring_words` (segment.py:225) → batched `IN` on
  `kana_text`/`kanji_text` → `find_word_suffix` → `join_substring_words` →
  `preload_scoring_caches` → `gen_score`/`calc_score` → `cull_segments` →
  `find_best_path` (synergies/segfilters inside) → `fill_segment_path` →
  `populate_meanings`.
- Hot SQL (must match exactly, incl. column order):
  - `SELECT id, seq, text, ord, common, best_kanji FROM kana_text WHERE text IN (…)`
  - `SELECT id, seq, text, ord, common, best_kana FROM kanji_text WHERE text IN (…)`
- Constants that silently break if DB rebuilt differently: everything in
  `constants.py` SEQ_* and scoring sets (`SKIP_WORDS`, `COPULAE`,
  `FINAL_PRT`, …).
- Python files to read first when porting a module: the target file itself,
  then `rg "from himotoki.<mod>"` importers to catch every used symbol.
- Gotchas indexed: `//` floor-div at calc_score.py:302; `math.ceil` at :638;
  `score_mod` may be callable/list (:848); `conjugations` is tri-state
  (None/'root'/list); `common` None vs 0 vs >0; `posi` is a *set*;
  suffix depth cap via contextvars → param; `SegmentList.top` holds DP
  scratch state; path nodes are heterogeneous (Segment/SegmentList/Synergy).

---

## 9. Suggested issue breakdown (bd)

Map each phase's checkbox groups to beads issues when work starts
(`bd create … --labels rust-port`). Phase 0 first; Phases 1-4 sequential;
6 after 5; 7-9 optional/parallel. Keep the golden diff green at each merge —
that's the whole game.
