# Roadmap

| Unit | Outcome | Depends on | Specs | Exit gate | Status | Evidence |
|---|---|---|---|---|---|---|
| `DETECTION-BASELINE` | Working duplicate-detection engine + CLI, cross-platform | none | `FCLONE-DETECTION-001` | `cargo fmt`/`clippy`/`test` green; CLI finds duplicates on a real tree | Done | initial workspace commit |
| `DETECTION-BENCHMARK` | Criterion benchmark suite over synthetic trees (many small duplicates, many unique files, few large duplicates, a mixed realistic tree), reporting files/sec and bytes/sec | `DETECTION-BASELINE` | `FCLONE-DETECTION-001` | `cargo bench` exists, compiles in CI (`--no-run`), and runs manually with real numbers | Done | `crates/rusty_fclone-core/benches/detection.rs`; sample run recorded in `docs/PROJECT-STATUS.md` |
| `DETECTION-BENCHMARK-VS-FCLONES` | Head-to-head comparison against upstream fclones on the same synthetic trees, so "fastest possible" becomes a measured claim rather than an architectural intent | `DETECTION-BENCHMARK` | `FCLONE-DETECTION-001` | Documented comparison (methodology + numbers) against an installed fclones build | Done | `docs/benchmarks/FCLONES-COMPARISON.md`; `scripts/gen_bench_trees.py`, `scripts/bench-vs-fclones.sh` |
| `DETECTION-ADAPTIVE-SAMPLE-SIZE` | Decouple the partial-hash sample size from the small-file threshold so large files aren't over-sampled during pruning | `DETECTION-BENCHMARK-VS-FCLONES` | `FCLONE-DETECTION-001` (revision) | `few_large_duplicates` benchmark scenario matches or beats fclones | Done, but see evidence — this alone didn't move the target scenario | ADR-0007; new `partial_hash_sample_size` field + `--partial-hash-sample-size` flag. Re-measured after: no meaningful change on `few_large_duplicates` (every file there is a real duplicate, so nothing gets pruned regardless of sample size) — see `docs/benchmarks/FCLONES-COMPARISON.md` |
| `DETECTION-IO-THREAD-SIZING` | Size the I/O pool to core count instead of an oversubscribed multiple, after benchmarking showed oversubscription hurting throughput on every tested scenario | `DETECTION-ADAPTIVE-SAMPLE-SIZE` | `FCLONE-DETECTION-001` (revision) | `few_large_duplicates` benchmark scenario matches or beats fclones | Done | ADR-0008; `few_large_duplicates` moved from 1.21x slower to within noise of fclones' best config and beats its default; small-file scenarios improved from ~1.9-2.0x to ~2.6-2.7x faster. Exit gate met by this unit, not `DETECTION-ADAPTIVE-SAMPLE-SIZE` — see `docs/benchmarks/FCLONES-COMPARISON.md` |
| `DETECTION-TRAVERSAL-COLLAPSE-FUSION` | Traversal and hardlink-collapse run as one streaming pass — each candidate is folded into the file-id map as jwalk produces it, instead of first materializing a `Vec<Candidate>` and looping over it separately | `DETECTION-BASELINE` | `FCLONE-DETECTION-001` (revision) | `cargo fmt`/`clippy`/`test`/`bench --no-run` green; manual CLI smoke test unchanged | Done | ADR-0012; `traversal::traverse` takes an `on_candidate` callback instead of returning a `Vec`; `pipeline::run_scan` folds hardlink-collapse into that callback directly |
| `DETECTION-STREAMING-OVERLAP` | Hashing begins before traversal finishes (full pipeline overlap), per ADR-0002/ADR-0004's noted scope cut | `DETECTION-TRAVERSAL-COLLAPSE-FUSION` | `FCLONE-DETECTION-001` (revision) | Benchmark shows reduced time-to-first-result on a large tree | Not Started | `DETECTION-TRAVERSAL-COLLAPSE-FUSION` above is a narrower, already-done step toward this (one pass instead of three), deliberately stopping short of overlap: hashing still only starts once traversal fully completes, since starting it earlier would let a `DuplicateGroup` be emitted, then later revised as more of the tree is walked — breaking `ScanEvent`'s "no group revision after emission" finality contract (ADR-0004). Doing this for real needs that contract redesigned first, which is a bigger, separate decision than this unit's scope (see ADR-0012's consequences) |
| `DETECTION-DEVICE-AWARE-IO-SIZING` | Pick `io_threads`'s default from whether the scan root's storage is rotational (Linux, best-effort via `/proc/self/mountinfo` + `/sys/dev/block/*/queue/rotational`) instead of one guessed constant | `DETECTION-IO-THREAD-SIZING` | `FCLONE-DETECTION-001` (revision) | `cargo fmt`/`clippy`/`test`/`bench --no-run` green; manual CLI smoke test confirms auto-detect and explicit `--io-threads` override both work | Done | ADR-0013; `rusty_fclone_core::device::default_io_threads`; `ScanOptions::io_threads`/CLI `--io-threads` change from `usize` to `Option<usize>` (`None` = auto-detect) |
| `DETECTION-LINUX-FASTPATH` | Optional Linux-specific I/O fast path: io_uring / `FIEMAP` extent-ordered reads, behind a feature flag, without breaking the cross-platform default | `DETECTION-DEVICE-AWARE-IO-SIZING` | new spec | Benchmark shows meaningful improvement on Linux; cross-platform build unaffected | Not Started | `DETECTION-DEVICE-AWARE-IO-SIZING` above closes this unit's thread-sizing half; io_uring/`FIEMAP` needs an async runtime and unsafe FFI, deserving its own ADR — deliberately not attempted here |
| `ACTION-LAYER` | Delete/hardlink confirmed duplicates, safely | `DETECTION-BASELINE` | `FCLONE-ACTION-001` | Dry-run-by-default, two-flag confirmation, and tests for every destructive path | Done | ADR-0009; `crates/rusty_fclone-core/src/action.rs` (6 tests); `--action`/`--apply` CLI flags; manual smoke tests for delete, hardlink, and unchanged default (report) behavior |
| `ACTION-REFLINK` | Copy-on-write clone as a third `ActionKind`, alongside delete/hardlink | `ACTION-LAYER` | `FCLONE-ACTION-001` (revision) | Reflink works on at least one CFS filesystem (Btrfs/XFS/APFS) without breaking the cross-platform default elsewhere | Not Started | deferred by ADR-0009 — platform-specific, needs a new dependency or unsafe FFI |
| `CLI-UX` | Richer CLI output (JSON, progress reporting, interactive confirmation prompt for actions) | `DETECTION-BASELINE`, `ACTION-LAYER` | new spec | Documented output contract with tests | Not Started | — |

## Known gaps carried from the v1 baseline (not blocking, but tracked)

None currently open — see "Closed" below. Path storage still uses a plain
`HashMap`-based model rather than fclones-style prefix-compressed storage,
but that's a deliberate, standing design choice (ADR-0004), not a tracked
gap: it's explicitly conditioned on benchmark evidence from a real
multi-million-file tree showing it's the actual bottleneck, which doesn't
exist yet. The redundant-clone cost that *was* a genuine, unconditional gap
is closed (see below).

Closed: cycle-detection test for `--follow-symlinks`
(`traversal::tests::follow_symlinks_terminates_on_a_cycle`, confirms jwalk's
loop detection actually works, with a bounded timeout so a regression fails
loudly instead of hanging); full-file hashing and `--verify` now stream in
fixed-size chunks instead of buffering whole files (ADR-0002 addendum);
structured logging/observability via `tracing` spans/events on the
traversal and pipeline stages, with a CLI `-v`/`--verbose` flag and
`RUST_LOG` support (ADR-0010); redundant path-clone cost in the detection
pipeline, by switching internal path storage from `PathBuf` to `Arc<Path>`
so cloning a path through the grouping stages is a refcount bump instead of
a fresh allocation and copy (ADR-0011).
