# rusty_fclone vs. fclones — a measured comparison

Closes the `DETECTION-BENCHMARK-VS-FCLONES` roadmap unit: the last piece
standing between "architected to be fast" and an actual measured claim
against the tool this project takes after.

## Methodology

- **Tool under test**: `rusty-fclone` (this repo, release build,
  `cargo build --release -p rusty_fclone-cli`).
- **Baseline**: [fclones](https://github.com/pkolaczk/fclones) 0.35.0,
  installed via `cargo binstall fclones` (prebuilt binary, not built from
  this repo's toolchain).
- **Corpus**: the same four synthetic trees used by
  `crates/rusty_fclone-core/benches/detection.rs`
  (`many_small_duplicates`, `many_unique_small_files`,
  `few_large_duplicates`, `mixed_realistic_tree`), generated once as real
  on-disk files by `scripts/gen_bench_trees.py` — a byte-for-byte port of
  the benchmark's tree builders — so both tools scan an *identical* corpus,
  not independently-generated approximations of it.
- **Timing**: [hyperfine](https://github.com/sharkdp/hyperfine), 3 warmup
  runs + minimum 10 measured runs per command, via
  `scripts/bench-vs-fclones.sh`. Both tools' stdout is redirected to
  `/dev/null` so terminal I/O doesn't skew results.
- **fclones invocations**: two, to separate two different questions —
  - *out-of-the-box default* (`fclones group <dir>`) — what a user gets
    without reading the manual; fclones defaults to the MetroHash hash
    function and a 16 KiB/4 KiB (HDD/SSD-detected) prefix+suffix partial
    check.
  - *matched hash algorithm* (`fclones group --hash-fn xxhash <dir>`) —
    isolates pipeline/architecture efficiency from hash-algorithm choice,
    since this project uses xxh3-128 (ADR-0001) and fclones' default is a
    different hash function.
- **Environment**: single 4-core Linux container, run 2026-08-24. These are
  relative numbers on one machine, not a portability or absolute-performance
  claim — see the caveats below before generalizing them.

A real bug surfaced during setup: the benchmark corpus generator's
"unique file" filler content only actually varied by `seed mod 256` (a ramp
pattern that repeats every 256 seeds), so the 2,000-file "unique" scenario
silently contained 256 duplicate groups instead of zero. Both tools agreeing
on a nonzero duplicate count for a tree that was supposed to have none is
what caught it — genuinely useful cross-validation from doing this
comparison at all. Fixed in both `benches/detection.rs` and
`gen_bench_trees.py` by encoding the seed directly into each file's first 8
bytes, guaranteeing distinct content per distinct seed.

## Results

Time is mean ± standard deviation across the runs; "Relative" is normalized
to the fastest command in that row group (`1.00` = fastest).

### `many_small_duplicates` (2,000 files, 1 KiB each, 200 duplicate groups of 10)

| Command | Mean | Relative |
|---|---:|---:|
| **rusty-fclone** | **42.6 ms ± 3.5 ms** | **1.00** |
| fclones (default: metro hash) | 81.6 ms ± 5.3 ms | 1.91× slower |
| fclones (xxhash, matched) | 80.2 ms ± 8.9 ms | 1.88× slower |

### `many_unique_small_files` (2,000 files, 1 KiB each, no duplicates)

| Command | Mean | Relative |
|---|---:|---:|
| **rusty-fclone** | **39.5 ms ± 2.9 ms** | **1.00** |
| fclones (default: metro hash) | 78.6 ms ± 6.3 ms | 1.99× slower |
| fclones (xxhash, matched) | 77.5 ms ± 8.0 ms | 1.96× slower |

### `few_large_duplicates` (20 files, 8 MiB each, 4 duplicate groups of 5)

| Command | Mean | Relative |
|---|---:|---:|
| fclones (xxhash, matched) | **44.2 ms ± 1.9 ms** | **1.00** |
| fclones (default: metro hash) | 46.9 ms ± 3.4 ms | 1.06× slower |
| rusty-fclone | 53.7 ms ± 4.9 ms | 1.21× slower |

**rusty_fclone loses this one.** See "Why we lose on large files" below.

### `mixed_realistic_tree` (1,018 files, mostly unique, 3 small duplicate groups)

| Command | Mean | Relative |
|---|---:|---:|
| **rusty-fclone** | **27.3 ms ± 6.2 ms** | **1.00** |
| fclones (xxhash, matched) | 44.7 ms ± 4.4 ms | 1.64× slower |
| fclones (default: metro hash) | 45.4 ms ± 4.2 ms | 1.66× slower |

## Reading these results honestly

- **rusty_fclone wins decisively (~1.9–2.0×) on small-file-heavy trees**,
  including the realistic mixed-tree scenario. This is where most real
  filesystems' file counts live, so it's a meaningful result, not a cherry-
  picked one.
- **rusty_fclone loses (~1.2×) on the large-file scenario.** The likely
  cause: ADR-0001's "one shared constant" design uses the same 128 KiB
  value as both the small-file threshold *and* the partial-hash sample
  size. For an 8 MiB file, that means reading 3 × 128 KiB = 384 KiB just
  for the pruning stage, before the full 8 MiB read — measurably more
  partial-hash I/O than fclones' much smaller default (16 KiB total for
  HDD, 8 KiB for SSD, single prefix+suffix vs. our three samples). ADR-0002's
  full-file buffering (reading the entire file into memory before hashing,
  rather than streaming) may also contribute. Neither has been isolated by
  further profiling yet — this is a documented, not yet root-caused,
  finding.
- **Hash algorithm choice barely matters** in these results (fclones'
  default-vs-matched-hash runs are within noise of each other in every
  scenario). The architecture — not the hash function — is what's driving
  the difference in both directions.
- These are single-machine, container-environment numbers with real
  variance (see the ± figures) — read them as "which architecture wins on
  which shape of workload," not as a precise universal multiplier.

## Follow-ups this comparison motivates

Added to `docs/roadmap/ROADMAP.md`:

- `DETECTION-ADAPTIVE-SAMPLE-SIZE` — decouple the partial-hash sample size
  from the small-file threshold (ADR-0001 flagged this as a simplicity
  tradeoff at the time; this comparison is the first evidence it costs
  something concrete on large files) and/or cap absolute sample size well
  below 128 KiB regardless of file size.
- `DETECTION-STREAMING-OVERLAP` and streaming full-file hashing (already
  tracked) are also plausible contributors to the large-file gap and worth
  revisiting with this result in mind.

## Reproducing this

```sh
cargo binstall fclones hyperfine   # or: cargo install fclones hyperfine
scripts/bench-vs-fclones.sh        # writes bench-results/*.md
```
