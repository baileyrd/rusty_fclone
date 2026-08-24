# rusty_fclone vs. fclones — a measured comparison

Closes the `DETECTION-BENCHMARK-VS-FCLONES` roadmap unit: the last piece
standing between "architected to be fast" and an actual measured claim
against the tool this project takes after. Updated after
`DETECTION-ADAPTIVE-SAMPLE-SIZE` and a follow-on I/O-thread-sizing fix
(ADR-0007, ADR-0008) closed the large-file gap found in the first pass —
see "Closing the large-file gap" below for the investigation, kept rather
than deleted because the first hypothesis tested there was wrong and that's
worth knowing.

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
- **rusty-fclone invocation**: default `ScanOptions` throughout (no
  per-scenario tuning flags) — the point is the tool's out-of-the-box
  behavior, same as fclones' "default" column.
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

## Results (current)

Time is mean ± standard deviation across the runs; "Relative" is normalized
to the fastest command in that row group (`1.00` = fastest).

### `many_small_duplicates` (2,000 files, 1 KiB each, 200 duplicate groups of 10)

| Command | Mean | Relative |
|---|---:|---:|
| **rusty-fclone** | **32.2 ms ± 3.5 ms** | **1.00** |
| fclones (default: metro hash) | 84.3 ms ± 7.6 ms | 2.61× slower |
| fclones (xxhash, matched) | 85.4 ms ± 10.3 ms | 2.65× slower |

### `many_unique_small_files` (2,000 files, 1 KiB each, no duplicates)

| Command | Mean | Relative |
|---|---:|---:|
| **rusty-fclone** | **31.3 ms ± 3.0 ms** | **1.00** |
| fclones (default: metro hash) | 82.7 ms ± 9.6 ms | 2.64× slower |
| fclones (xxhash, matched) | 84.7 ms ± 9.9 ms | 2.71× slower |

### `few_large_duplicates` (20 files, 8 MiB each, 4 duplicate groups of 5)

| Command | Mean | Relative |
|---|---:|---:|
| fclones (xxhash, matched) | **38.6 ms ± 3.4 ms** | **1.00** |
| rusty-fclone | 40.2 ms ± 4.8 ms | 1.04× slower |
| fclones (default: metro hash) | 46.2 ms ± 3.2 ms | 1.20× slower |

rusty_fclone now beats fclones' own out-of-the-box default (40.2 ms vs.
46.2 ms) and is within measurement noise of fclones' best configuration —
1.04× with a ±0.15 confidence band that spans below 1.0. Down from 1.21×
slower than fclones' best configuration in the original pass.

### `mixed_realistic_tree` (1,018 files, mostly unique, 3 small duplicate groups)

| Command | Mean | Relative |
|---|---:|---:|
| **rusty-fclone** | **17.1 ms ± 2.4 ms** | **1.00** |
| fclones (default: metro hash) | 44.4 ms ± 3.9 ms | 2.59× slower |
| fclones (xxhash, matched) | 47.7 ms ± 6.2 ms | 2.78× slower |

## Reading these results honestly

- **rusty_fclone wins decisively (~2.6–2.7×) on small-file-heavy trees**,
  including the realistic mixed-tree scenario. This is where most real
  filesystems' file counts live, so it's a meaningful result, not a
  cherry-picked one.
- **rusty_fclone matches fclones (within noise) on the large-file
  scenario**, and beats fclones' own default configuration outright. Not an
  unqualified win — fclones' best (hash-matched) configuration is still
  numerically ahead by a hair, inside the confidence interval.
- **Hash algorithm choice barely matters** in these results (fclones'
  default-vs-matched-hash runs are within noise of each other in every
  scenario). The architecture — not the hash function — is what drives the
  difference.
- These are single-machine, container-environment numbers with real
  variance (see the ± figures) — read them as "which architecture wins on
  which shape of workload," not as a precise universal multiplier.

## Closing the large-file gap: what actually worked

The original pass (see git history for this file) found rusty_fclone
losing ~1.2× on `few_large_duplicates` and hypothesized ADR-0001's shared
"one constant for both the small-file cutoff and the partial-hash sample
size" as the cause — 3×128 KiB of partial-hash I/O per file looked like
wasted work compared to fclones' much smaller default samples.

**That hypothesis was tested and refuted.** ADR-0007 decoupled the two
constants (new default partial-hash sample: 16 KiB, matching fclones' own
HDD default). Re-measuring `few_large_duplicates` alone afterward: no
meaningful change (~1.21× slower, statistically unchanged). The reason,
obvious in hindsight: every file in this scenario *is* a real duplicate, so
nothing gets pruned by the partial-hash stage regardless of its sample
size — all 20 files proceed to a mandatory full 8 MiB read either way. The
partial-hash sample was never more than ~4% of this scenario's total I/O
volume, nowhere near enough to explain a 20% gap.

**The actual cause was I/O thread pool oversubscription** (ADR-0002's
original `cores * 4` default). Sweeping `--io-threads` from 1 to 16 on
`few_large_duplicates` (4-core container) found 4 threads clearly optimal
(36.8 ms) against every oversubscribed value tested (44.6 ms at 8 threads,
50.8 ms at 16 — the previous default) *and* every undersubscribed value (41.5
ms at 2, 52.2 ms at 1). Checking whether this was large-file-specific: the
same sweep on all three small-file scenarios showed the identical pattern —
4 threads beat 8 and 16 in every one of them too. Oversubscription was
hurting uniformly, not helping small files as ADR-0002's original
latency-hiding rationale assumed for this environment's storage.

ADR-0008 changed the default from `cores * 4` (capped 64) to `cores`,
keeping `--io-threads` as an override for storage where the original
oversubscription reasoning still applies (e.g. real spinning disks or
network filesystems, neither tested here). This is the change that actually
moved `few_large_duplicates` from 1.21× slower to parity, and — since the
small-file scenarios were *also* being hurt by oversubscription, just not
losing outright — pushed their already-winning margin from ~1.9–2.0× up to
~2.6–2.7×.

Both ADR-0007 and ADR-0008 are kept: the sample-size decoupling is a
genuine improvement for realistic large-file trees where most files
*aren't* duplicates (unlike this synthetic all-duplicates scenario), even
though it wasn't what this specific benchmark needed.

## Roadmap status

- `DETECTION-ADAPTIVE-SAMPLE-SIZE`: Done (ADR-0007), though see above for
  why it wasn't sufficient alone.
- The I/O-thread-sizing fix (ADR-0008) that actually closed the gap wasn't
  a pre-existing roadmap item; it's recorded as its own ADR since it
  revises ADR-0002's default.
- Still open: `DETECTION-LINUX-FASTPATH` (principled device-type-aware I/O
  tuning, rather than one guessed constant either way) and
  `DETECTION-STREAMING-OVERLAP`.

## Reproducing this

```sh
cargo binstall fclones hyperfine   # or: cargo install fclones hyperfine
scripts/bench-vs-fclones.sh        # writes bench-results/*.md
```
