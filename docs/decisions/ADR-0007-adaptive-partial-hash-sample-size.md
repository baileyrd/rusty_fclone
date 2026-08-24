# ADR-0007: Decouple partial-hash sample size from the small-file threshold

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0001 (original "one shared constant" decision),
  `docs/benchmarks/FCLONES-COMPARISON.md`

## Context

ADR-0001 deliberately used one constant (`small_file_threshold`, 128 KiB)
for two different jobs: "should a file skip the partial-hash stage
entirely" and "how many bytes to sample at head/mid/tail during the
partial-hash stage" — chosen for simplicity, with an explicit note to
revisit "once a benchmark demonstrates it's the bottleneck." The fclones
comparison (`DETECTION-BENCHMARK-VS-FCLONES`) found rusty_fclone losing
~1.2x on a large-file scenario and hypothesized this constant as the cause:
sampling 3×128 KiB = 384 KiB per file before the full read is a lot of
partial-hash I/O for files where fclones' own defaults sample only
4–16 KiB total.

## Decision

Split `small_file_threshold` into two independent `ScanOptions` fields:

- `small_file_threshold` (unchanged default: 128 KiB) — still the cutoff
  below which a file skips the partial-hash stage and goes straight to one
  full hash.
- `partial_hash_sample_size` (new, default: 16 KiB) — the head/mid/tail
  sample chunk length for files larger than the threshold. Chosen to match
  fclones' own HDD default as a reasonable, well-precedented single value,
  since this project doesn't yet do device-type detection (see ADR-0002).

Both are exposed as CLI flags (`--small-file-threshold`,
`--partial-hash-sample-size`).

## Consequences (and an honest correction)

This change is a real improvement in general: for workloads where the
partial-hash stage actually eliminates non-matching candidates (i.e. most
realistic large-file trees, where not every large file is a duplicate), a
smaller sample means less wasted I/O per eliminated candidate.

**It did not close the specific benchmark gap that motivated it.** The
`few_large_duplicates` benchmark scenario has every file as a real
duplicate — nothing gets pruned by the partial-hash stage regardless of
sample size, since all candidates survive to the mandatory full-hash read
anyway. Re-measuring after this change alone showed no meaningful
improvement (~1.21x slower, unchanged from before). The actual fix for that
gap was unrelated: I/O thread pool oversubscription — see ADR-0008. Both
changes are kept because both are independently justified; this ADR's
change just wasn't the one the benchmark needed. Documented here rather
than quietly dropped, since the original hypothesis being wrong is itself
useful information for the next person reading ADR-0001's "revisit once a
benchmark demonstrates it's the bottleneck" note.

No new automated test asserts "these two fields are wired independently":
because the pipeline always confirms a partial-hash match with a full hash
before reporting a duplicate (ADR-0001), final scan *results* are
provably invariant to the sample size chosen — only *performance* is
sensitive to it. A black-box test comparing final duplicate groups can't
distinguish a correct wiring from a bug here; the existing `sample_ranges`
unit tests already cover the sampling math itself, and the two pipeline
tests that previously reused `small_file_threshold` as a proxy for sample
size (`finds_duplicates_larger_than_sample_size`,
`no_duplicates_when_only_prefix_matches`) were updated to set
`partial_hash_sample_size` explicitly so they keep testing what they were
designed to test.
