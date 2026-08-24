# Specification Registry

| ID | Title | Version | Design | Implementation | Verification | Depends on | Owner | Location | Evidence |
|---|---|---:|---|---|---|---|---|---|---|
| `FCLONE-DETECTION-001` | Duplicate File Detection Engine | 0.1.4 | Accepted | Implemented | Verified (functional requirements + relative/regression benchmarks + comparison vs. fclones, matched or beaten on all 4 scenarios) | none | baileyrd | [`docs/specifications/detection/FCLONE-DETECTION-001.md`](detection/FCLONE-DETECTION-001.md) | `crates/rusty_fclone-core` test suite (25 tests) + `benches/detection.rs` + `docs/benchmarks/FCLONES-COMPARISON.md` |

## Not yet specified (roadmap, not scoped in this baseline)

These capability areas were explicitly deferred while this session focused
on detection (see `docs/roadmap/ROADMAP.md`); they have no spec ID yet
because no design work has been done on them:

- **Action layer** — deleting, hardlinking, or reflinking confirmed
  duplicates.
- **CLI/reporting UX** — output formats beyond the plain-text v1 CLI
  (JSON, machine-readable, progress reporting).
- **Observability** — structured logging/tracing, scan progress events.
