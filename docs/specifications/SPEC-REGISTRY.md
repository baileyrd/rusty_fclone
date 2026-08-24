# Specification Registry

| ID | Title | Version | Design | Implementation | Verification | Depends on | Owner | Location | Evidence |
|---|---|---:|---|---|---|---|---|---|---|
| `FCLONE-DETECTION-001` | Duplicate File Detection Engine | 0.1.7 | Accepted | Implemented | Verified (functional requirements + relative/regression benchmarks + comparison vs. fclones, matched or beaten on all 4 scenarios) | none | baileyrd | [`docs/specifications/detection/FCLONE-DETECTION-001.md`](detection/FCLONE-DETECTION-001.md) | `crates/rusty_fclone-core` test suite + `benches/detection.rs` + `docs/benchmarks/FCLONES-COMPARISON.md` |
| `FCLONE-ACTION-001` | Duplicate Action Layer (delete/hardlink/reflink) | 0.2.0 | Accepted | Implemented | Verified (all functional requirements have a dedicated test) | `FCLONE-DETECTION-001` | baileyrd | [`docs/specifications/action/FCLONE-ACTION-001.md`](action/FCLONE-ACTION-001.md) | `crates/rusty_fclone-core` `action` module (7 tests) + `crates/rusty_fclone-cli` `main` module (5 tests) |
| `CLI-UX-001` | CLI Output, Progress, and Confirmation | 0.1.0 | Accepted | Implemented | Verified (functional requirements have a dedicated test or documented manual smoke test) | `FCLONE-DETECTION-001`, `FCLONE-ACTION-001` | baileyrd | [`docs/specifications/cli-ux/CLI-UX-001.md`](cli-ux/CLI-UX-001.md) | `crates/rusty_fclone-cli` `main` module (9 tests) |
