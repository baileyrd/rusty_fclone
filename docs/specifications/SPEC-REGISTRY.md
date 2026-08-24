# Specification Registry

| ID | Title | Version | Design | Implementation | Verification | Depends on | Owner | Location | Evidence |
|---|---|---:|---|---|---|---|---|---|---|
| `FCLONE-DETECTION-001` | Duplicate File Detection Engine | 0.1.6 | Accepted | Implemented | Verified (functional requirements + relative/regression benchmarks + comparison vs. fclones, matched or beaten on all 4 scenarios) | none | baileyrd | [`docs/specifications/detection/FCLONE-DETECTION-001.md`](detection/FCLONE-DETECTION-001.md) | `crates/rusty_fclone-core` test suite + `benches/detection.rs` + `docs/benchmarks/FCLONES-COMPARISON.md` |
| `FCLONE-ACTION-001` | Duplicate Action Layer (delete/hardlink/reflink) | 0.2.0 | Accepted | Implemented | Verified (all functional requirements have a dedicated test) | `FCLONE-DETECTION-001` | baileyrd | [`docs/specifications/action/FCLONE-ACTION-001.md`](action/FCLONE-ACTION-001.md) | `crates/rusty_fclone-core` `action` module (7 tests) + `crates/rusty_fclone-cli` `main` module (5 tests) |

## Not yet specified (roadmap, not scoped in this baseline)

These capability areas have no spec ID yet because no design work has been
done on them (see `docs/roadmap/ROADMAP.md`):

- **CLI/reporting UX** — output formats beyond the plain-text v1 CLI
  (JSON, machine-readable, progress reporting, confirmation prompts).
