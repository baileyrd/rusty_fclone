# ADR-0024: Trash action via the `trash` crate

- Status: Accepted
- Date: 2026-08-26
- Related: ADR-0009 (action layer, safety model this extends), ADR-0014
  (reflink action — same "dependency, not hand-rolled FFI" pattern for a
  platform-specific action), `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`
  (`ACTION-TRASH`, Phase 1 table stakes)

## Context

Every existing `ActionKind` variant is either non-destructive in the
recoverable sense (`Hardlink`/`Reflink` keep the redundant path readable,
just repointed) or permanently destructive (`Delete` calls `fs::remove_file`
directly — no recovery path once it succeeds). The competitive research
behind `DEDUP-GAP-IMPLEMENTATION-PLAN.md` found Recycle Bin/Trash-routed
deletion present in 7 of 8 comparable products and called out as near-
universal table stakes; this project had no recoverable delete path at all.

Two design questions, matching ADR-0014's shape for the last platform-
specific action added: how to talk to the platform (a dependency vs.
hand-rolled per-OS FFI), and how this interacts with the existing
`ActionKind` match arms and folder-level pruning logic.

## Decision

- **Dependency, not hand-rolled FFI**: use the `trash` crate (workspace
  dependency, `rusty_fclone-core`-only, default features). It implements
  the freedesktop.org trash spec on Linux, the Shell API Recycle Bin on
  Windows, and the Finder Trash on macOS behind one portable
  `trash::delete` function — the same "well-trodden systems code, don't
  hand-roll it" reasoning ADR-0014 already established for reflink.
  Verified in a scratch project that it builds cleanly on Linux with no
  C-toolchain requirement (`AGENTS.md`'s dependency policy) and confirmed
  working end-to-end in this environment (a real file moved into
  `~/.local/share/Trash/files/`, recoverable).
- **Keep the crate's default features**, including `chrono`: on Linux it's
  used to write a full-precision deletion timestamp into the `.trashinfo`
  restore metadata; without it, that timestamp degrades. Correctness of
  the recovery metadata is the entire point of choosing `Trash` over
  `Delete`, so this isn't a place to trim a dependency for its own sake.
- **A new variant, not a `Delete` flag**: `ActionKind::Trash` sits alongside
  `Delete` rather than a `--permanent`-style modifier on it, so `--action
  delete` unambiguously keeps its existing (permanent) behavior — no
  silent behavior change for anyone already scripting against it.
  `Delete`'s doc comment now recommends `Trash` unless permanent,
  unrecoverable deletion is specifically wanted.
- **Folder-level pruning treats `Trash` like `Delete`**: `folder_action::
  apply_folder`'s "prune the emptied `removed` directory tree" logic
  (previously gated on `plan.kind == ActionKind::Delete`) now runs for
  `Delete` or `Trash` — both remove every file from its original location
  (unlike `Hardlink`/`Reflink`, which replace files in place), so the same
  post-condition (an empty directory tree worth pruning) holds for both.

## Consequences

- New dependency: `trash` (plus its transitive deps — `chrono`,
  `urlencoding`, and, Windows-only, the `windows`/`windows-*` crate family,
  inert on Linux/macOS builds).
- `ActionKind` gains a fourth variant; every exhaustive match on it (the
  `apply` dispatch, `folder_action`'s directory-prune gate, the CLI's
  `Action` enum, and the GUI's `parse_action_kind`) needed updating —
  caught at compile time, not a runtime gap.
- `trash::Error` doesn't implement `Into<std::io::Error>` directly (it's a
  bespoke enum, not an `io::Error` wrapper); converted via
  `std::io::Error::other`, matching `FileError::source`'s existing
  `io::Error` field type without widening it to a more general error type
  project-wide.
- No temp-then-rename safety dance is needed the way `hardlink_over`/
  `reflink_over` use, unlike `Delete`: it's not replacing anything in
  place, so `action::apply`'s `Trash` arm calls `trash::delete` directly.
- Verified end-to-end in this environment (a real file trashed and found
  recoverable at `~/.local/share/Trash/files/`) in addition to the unit
  tests; not verified on Windows or macOS, where `trash`'s own
  platform-specific implementations are trusted rather than re-verified
  here — the same posture ADR-0014 took for reflink's non-Linux paths.
