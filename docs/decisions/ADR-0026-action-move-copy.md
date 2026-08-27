# ADR-0026: Archive-folder actions — `ActionKind::Move`/`ActionKind::Copy`

- Status: Accepted
- Date: 2026-08-27
- Related: ADR-0009 (action-layer safety model, extended here), ADR-0014
  (reflink — the last time this project chose "fail loud on a cross-
  filesystem limitation" over a silent fallback, reused here for `Move`),
  ADR-0023 (folder-level action, whose directory-prune step this extends
  too), `docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`
  (`ACTION-MOVE-COPY`, Phase 2)

## Context

Every existing `ActionKind` either destroys the redundant copy (`Delete`,
`Trash`) or replaces it with a space-sharing link in place (`Hardlink`,
`Reflink`). None of them can consolidate redundant copies somewhere a
user can review before committing to removal — the gap the plan calls out
as "dupeGuru offers Delete/Move/Copy explicitly; several others have an
archive-folder option."

Two design questions came up turning that into a concrete `ActionKind`
addition:

1. Where does the archive destination live — a new parameter threaded
   through `plan`/`plan_with_keep`/`plan_folder` alongside `kind`, or
   carried by the enum variant itself?
2. What does `Copy` actually mean here — copy-then-delete (identical end
   state to `Move`, just a different mechanism), or copy-and-leave-the-
   original? The former makes `Copy` a redundant twin of `Move`; only the
   latter is a distinct, useful capability.

## Decision

- **The archive directory is data carried by the `ActionKind` variant
  itself**: `ActionKind::Move(PathBuf)` / `ActionKind::Copy(PathBuf)`,
  not a new parameter threaded alongside `reference_paths` through
  `plan`/`plan_with_keep`/`plan_folder`. `kind: ActionKind` is already a
  parameter on every one of those functions — putting the destination
  inside it means zero new parameters ripple through the API surface
  `ACTION-REFERENCE-FOLDERS` just finished threading `reference_paths`
  through, and every existing exhaustive `match` on `ActionKind` catches
  the two new variants at compile time (the same "caught at compile
  time, not a runtime gap" property ADR-0024 recorded for `Trash`).
  Consequence: `ActionKind` can no longer derive `Copy` (a `PathBuf` in
  two variants isn't `Copy`) — every call site that used to rely on an
  implicit copy now clones explicitly (cheap: a discriminant for four
  variants, one `PathBuf` clone for the other two, never in a hot loop)
  or borrows (`action_word`, `confirm_apply`, `report_action_summary` in
  the CLI all now take `&ActionKind` instead of consuming one, since none
  of them need ownership).
- **`Copy` leaves the original in place and reclaims nothing.** The
  alternative (copy the file to the archive, then delete the original)
  produces the exact same end state as `Move` — gone from its original
  path, present at the archived one — through a different mechanism, so
  it isn't actually a distinct capability worth a second `ActionKind`.
  `Copy` as "consolidate a copy into the archive folder for review,
  touch nothing else" is what dupeGuru-style flexibility actually adds:
  a cautious, two-step workflow (build a trusted archive first, decide
  what to permanently remove later) this project's existing
  preview-then-apply safety model already supports well.
  `ApplyReport::bytes_reclaimed`/`FolderApplyReport::bytes_reclaimed` are
  `0` for every successful `Copy` action — an honest, deliberate
  asymmetry with every other kind, not a bug.
- **Archived paths mirror the original path's structure under the
  archive directory** (every path component except a root/prefix/`.`/
  `..` is preserved and joined underneath), rather than a flat
  `archive_dir/<filename>`. Two redundant copies named `photo.jpg` from
  different original directories would otherwise collide and one would
  silently clobber the other. `..` components are dropped rather than
  preserved, closing off a crafted relative path escaping the archive
  directory.
- **Never silently clobber an existing archive destination.** If a
  second scan finds the same redundant file already archived from an
  earlier `Move`/`Copy` run, the destination already exists — treated as
  a per-file failure (`io::ErrorKind::AlreadyExists`), not a silent
  overwrite, matching every other "never silently destroy data" choice
  in this module.
- **`Move` doesn't fall back to copy-then-remove on a cross-device
  `rename` failure** — it surfaces as a normal per-file error instead,
  the same choice ADR-0014 already made for reflink's own cross-
  filesystem limitation. A caller archiving across filesystems can
  choose `Copy` (which uses `fs::copy`, filesystem-agnostic) and clean
  up the originals separately; adding asymmetric fallback behavior found
  nowhere else in this module wasn't worth it for a case the existing
  `ActionKind` set already has an answer to.
- **Folder-level directory pruning (ADR-0023) now also fires for
  `Move`**, alongside `Delete`/`Trash` — all three leave `removed`
  file-less on full success, the same post-condition the prune step
  already checks for. `Copy` never prunes: every original file stays
  exactly where it was.
- **CLI**: `--action move`/`--action copy`, plus a new `--archive-dir
  <PATH>` flag (required by, and only meaningful with, those two — a
  clap-level "required if" would work but wouldn't name which action
  needs it, so this is validated explicitly with a specific error
  message instead). **GUI**: a new "Archive folder" field appears in the
  Review action bar only when Move or Copy is selected, threaded into
  `run_action`/`run_folder_action` (not `choose_keep`, which has no
  `ActionKind` concept); Apply stays disabled until it's filled in for
  those two kinds. The Review screen's reclaim-estimate text is rewritten
  for `Copy` specifically ("will be copied to the archive folder --
  nothing reclaimed") rather than showing a byte figure that would never
  actually be freed.

## Consequences

- `ActionKind` loses its `Copy` derive — a real, if narrow, migration
  cost paid once across `action.rs`, `folder_action.rs`, and every CLI/
  GUI call site (`.clone()` where ownership is genuinely needed, `&`
  where it isn't). No behavior changed for the four existing variants;
  every existing test for them still passes unmodified.
- `rusty_fclone_core`'s public surface gains `ActionKind::Move`/`Copy`
  and a private `archived_path`/`move_into`/`copy_into` helper set in
  `action.rs`. No existing function's semantics changed for `Delete`/
  `Trash`/`Hardlink`/`Reflink`.
- A `Copy`-only workflow (archive everything, delete nothing) means a
  scan's "bytes reclaimed" total can legitimately be `0` even after a
  successful, fully-applied action pass — surfaced honestly rather than
  computed as if space had been freed.
- Verified end-to-end in this environment against a real filesystem: a
  `Move` correctly relocated a redundant file to its mirrored archive
  path and left the kept file untouched; a `Copy` left both the original
  and a new archived copy in place with zero bytes reported reclaimed; a
  repeated `Copy` run against the same tree failed cleanly on the
  already-archived destination without touching either the original or
  the first run's archived file.
