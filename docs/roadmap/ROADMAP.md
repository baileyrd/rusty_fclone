# Roadmap

| Unit | Outcome | Depends on | Specs | Exit gate | Status | Evidence |
|---|---|---|---|---|---|---|
| `DETECTION-BASELINE` | Working duplicate-detection engine + CLI, cross-platform | none | `FCLONE-DETECTION-001` | `cargo fmt`/`clippy`/`test` green; CLI finds duplicates on a real tree | Done | initial workspace commit |
| `DETECTION-BENCHMARK` | Criterion benchmark suite over synthetic trees (many small duplicates, many unique files, few large duplicates, a mixed realistic tree), reporting files/sec and bytes/sec | `DETECTION-BASELINE` | `FCLONE-DETECTION-001` | `cargo bench` exists, compiles in CI (`--no-run`), and runs manually with real numbers | Done | `crates/rusty_fclone-core/benches/detection.rs`; sample run recorded in `docs/PROJECT-STATUS.md` |
| `DETECTION-BENCHMARK-VS-FCLONES` | Head-to-head comparison against upstream fclones on the same synthetic trees, so "fastest possible" becomes a measured claim rather than an architectural intent | `DETECTION-BENCHMARK` | `FCLONE-DETECTION-001` | Documented comparison (methodology + numbers) against an installed fclones build | Not Started | — |
| `DETECTION-STREAMING-OVERLAP` | Hashing begins before traversal finishes (full pipeline overlap), per ADR-0002/ADR-0004's noted scope cut | `DETECTION-BASELINE` | `FCLONE-DETECTION-001` (revision) | Benchmark shows reduced time-to-first-result on a large tree | Not Started | — |
| `DETECTION-LINUX-FASTPATH` | Optional Linux-specific I/O fast path (io_uring / `FIEMAP` extent ordering) behind a feature flag, without breaking the cross-platform default | `DETECTION-BENCHMARK` | new spec | Benchmark shows meaningful improvement on Linux; cross-platform build unaffected | Not Started | — |
| `ACTION-LAYER` | Delete/hardlink/reflink confirmed duplicates, safely | `DETECTION-BASELINE` | new spec | Dry-run mode, confirmation flow, and tests for every destructive path | Not Started | — |
| `CLI-UX` | Richer CLI output (JSON, progress reporting, `--dry-run`) | `DETECTION-BASELINE` | new spec | Documented output contract with tests | Not Started | — |

## Known gaps carried from the v1 baseline (not blocking, but tracked)

- No structured logging/progress observability yet (`tracing` or similar).
- No cycle-detection test for `--follow-symlinks`.
- Full-file hashing buffers the whole file in memory; large files aren't
  streamed through the hasher (ADR-0002 implementation note).
- Path storage is the naive `HashMap`-based model; prefix-compression
  deferred until benchmarked as necessary (ADR-0004).
