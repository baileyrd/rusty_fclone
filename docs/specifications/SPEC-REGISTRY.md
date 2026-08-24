# Specification Registry

| ID | Title | Version | Design | Implementation | Verification | Depends on | Owner | Location | Evidence |
|---|---|---:|---|---|---|---|---|---|---|
| `FCLONE-DETECTION-001` | Duplicate File Detection Engine | 0.1.0 | Accepted | Implemented | Not Verified (no benchmark yet) | none | baileyrd | [`docs/specifications/detection/FCLONE-DETECTION-001.md`](detection/FCLONE-DETECTION-001.md) | `crates/rusty_fclone-core` test suite |

## Not yet specified (roadmap, not scoped in this baseline)

These capability areas were explicitly deferred while this session focused
on detection (see `docs/roadmap/ROADMAP.md`); they have no spec ID yet
because no design work has been done on them:

- **Action layer** — deleting, hardlinking, or reflinking confirmed
  duplicates.
- **CLI/reporting UX** — output formats beyond the plain-text v1 CLI
  (JSON, machine-readable, progress reporting).
- **Observability** — structured logging/tracing, scan progress events.
