# ADR-0032: Reviewable plan files, an apply journal, and undo for moves

- Status: Accepted
- Date: 2026-10-25
- Related: ADR-0009 (action layer, preview-by-default), ADR-0016 (size+mtime freshness), ADR-0026 (`Move`/`Copy`), ADR-0027 (`history` reserved keyword)

## Context

Today a destructive run is all-or-nothing within one process: `--action
<kind>` previews, `--action <kind> --apply` re-scans and acts as groups
stream in. On a large, slow tree (a 20 TB NTFS drive full of scattered
duplicates is the motivating case), that leaves three gaps:

1. **No review artifact.** A preview scrolls past on stdout. There is no
   file to read at leisure, diff, or edit ("not this group") before acting.
2. **Preview and apply are different scans.** Anything that changes in
   between is acted on without having been reviewed.
3. **No undo.** `--history` (ADR-0027) records *what* happened per file,
   but not enough to reverse it (a `Move`'s destination isn't stored), and
   it's opt-in analytics, not a safety mechanism.

## Decision

- **`--plan-out <FILE>`** writes the previewed plan as pretty JSON and acts
  on nothing. It requires a non-`report` `--action` and conflicts with
  `--apply` (applying is the plan's job) and `--find-duplicate-folders`
  (folder plans aren't serialized yet — see Consequences).
- **Format:** `{format, version, root, created_at, action, groups:[{size,
  kept:{path, mtime_ns}, targets:[{path, mtime_ns}]}]}`. `action` is a
  tagged enum, so `move`/`copy` can't exist without `archive_dir`. All
  paths are made absolute (`std::path::absolute`, no filesystem access), so
  a plan applies identically from any working directory. Non-UTF-8 paths
  can't go in JSON; such a group is left out with a warning (non-zero
  exit), never silently dropped and never failing the whole write.
- **`rusty-fclone plan apply <FILE>`** previews by default; `--apply` (plus
  the usual prompt, skippable with `-y`) executes. That keeps the ADR-0009
  two-step rule intact for the new entry point.
- **Freshness, per group, skipped whole.** Before acting, every file in the
  group (kept *and* targets) must still be a regular file with the recorded
  size and mtime — the same freshness heuristic as the hash cache
  (ADR-0016). Any mismatch skips the entire group: if the kept file changed,
  deleting its "duplicates" could destroy the only copy of the old content.
  Stale groups are reported and make the exit code non-zero.
- **Execution reuses the core unchanged.** Each group is rebuilt as a
  `DuplicateGroup` and planned through `action::plan_with_keep` with the
  recorded kept path, then run through `action::apply`. Hardlink-alias
  filtering, byte accounting, and every action kind's safety properties
  are therefore identical to a live run. A plan file can't do anything a
  live `--apply` couldn't.
- **Journal: append-only JSONL, separate from `--history`.** Each completed
  action appends one tagged line (`{"op":"move","from":…,"to":…}`, etc.) to
  `<PLAN>.journal.jsonl` (or `--journal <PATH>`), fsynced once per group.
  It's separate from the SQLite `--history` database because it's always
  on whenever a plan is applied (you can't forget to enable it), sits next
  to the plan it belongs to, and needs no schema migration of an existing
  database. `--history` stays the cross-scan analytics store.
- **`rusty-fclone plan undo <JOURNAL>`** reverses `move` entries newest
  first by renaming them back, previewing unless `--apply`. It never
  overwrites anything already at the original path. Other operations are
  reported as not undoable rather than guessed at: delete is gone, trash
  recovery is OS-specific, hardlink/reflink undo would need a copy, and
  copy left the original untouched anyway.
- **`plan` is a reserved top-level keyword**, pre-dispatched in `main`
  exactly like `history` (ADR-0027). Every existing invocation is
  unaffected. A directory literally named `plan` needs `./plan`.
- **Core change: `action::archived_path` becomes `pub`**, so the journal
  records exactly the destination `apply` used instead of re-deriving the
  mirroring scheme. Serialization stays in the CLI crate (AGENTS.md).
- **No new dependencies.** `serde`/`serde_json` are already CLI
  dependencies; `std::path::absolute` is std.

## Consequences

- Recommended workflow for a large drive: `--action move --archive-dir
  <same-volume quarantine> --plan-out plan.json` → review/edit → `plan
  apply plan.json --apply` → keep the quarantine until satisfied → `plan
  undo` if not. `Move` within one volume is a rename, so it's fast and
  never copies data; a cross-device move fails per file rather than copying.
- Known limits, all deliberate for v1:
  - Freshness is size+mtime, not content. A same-size edit that preserves
    mtime (or lands within the filesystem's mtime granularity) isn't
    detected. A `--verify` pass that byte-compares targets to the kept file
    at apply time is the natural follow-up.
  - The archive layout mirrors the absolute path (`<archive>/mnt/drive/…`),
    because plan paths are absolute. That's unambiguous but deeper than a
    live run with a relative root.
  - The journal is written after each group completes, so a crash
    mid-group can lose that group's entries (at most one group).
  - Folder-level plans (`--find-duplicate-folders`) aren't serialized.
  - `plan` subcommands print text only (no `--format json`).
