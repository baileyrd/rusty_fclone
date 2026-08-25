# ADR-0021: Folder-level duplicate detection

- Status: Accepted
- Date: 2026-08-25
- Related: ADR-0001 (staged hashing this builds on), ADR-0004 (engine
  API/streaming contract — this is deliberately a post-scan step, not an
  extension of it), ADR-0009 (action-layer safety model — this ADR
  explicitly does not extend it)

## Context

The engine finds duplicate *files*. A user with `Photos/2024/vacation/`
copied wholesale into `Backup/2024/vacation/` currently sees that as many
individual `DuplicateGroup`s — one per file — with no signal that the
whole folder is redundant. Asked for directly: "is it possible to
identify if folders of files are duplicates?"

Two distinct relationships are useful:
- **Exact**: two (or more) folders whose entire recursive file contents
  are pairwise identical — same relative paths, same content, nothing
  extra on either side.
- **Contained**: one folder's entire recursive file content exists,
  path-for-path and byte-for-byte, inside another folder, which may have
  additional files the smaller one doesn't (e.g. a folder that was copied
  into a larger archive alongside other things).

Both were asked for.

## Decision

- **Post-scan analysis, not a scan-time extension.** A new function,
  `rusty_fclone_core::folder_dedup::find_folder_duplicates(root, groups,
  options) -> Result<Vec<FolderMatch>, ScanError>`, called after a normal
  scan completes with its full `Vec<DuplicateGroup>`. This deliberately
  does not touch `scan()`/`ScanEvent`'s streaming contract (ADR-0004): a
  folder verdict needs the *whole* tree's picture (a folder can't be
  ruled out as "fully duplicated" until every file under it has been
  seen), so it can't be produced incrementally the way a `DuplicateGroup`
  can. It runs its own lightweight second traversal (reusing
  `traversal::traverse`, stat-only, no hashing — hashing was already done
  by the scan that produced `groups`) purely to learn the complete file
  set per directory, including files that have zero duplicates anywhere
  (`scan()` never surfaces those, by design, so a second pass is the only
  way to know a directory has one).
- **"Fully duplicated" is the gate for being a match's smaller/subset
  side.** A directory (recursively, including subdirectories) can only be
  an `Exact` participant or a `Contained` subset if *every* file under it
  has at least one duplicate somewhere in the tree — a single unmatched
  file anywhere in its subtree disqualifies it, since that file
  structurally cannot be present in any candidate partner. A directory
  can still be the *superset* side of a `Contained` match with unmatched
  files of its own — a superset is only required to contain the subset's
  files, not to consist entirely of them.
- **Candidate discovery via path-suffix matching on existing groups, not
  O(n²) directory comparison.** For a fully-duplicated directory `A` with
  recursive signature `[(rel_path, group_id), ...]`, a candidate partner
  directory is found by taking, for each `(rel_path, group_id)` pair,
  every *other* path already recorded in that `DuplicateGroup`, and
  checking whether it ends in `rel_path` — if so, stripping that suffix
  gives a candidate base directory. Intersecting the candidate-base sets
  across every one of `A`'s files gives exactly the directories
  containing a full duplicate of `A` at the right relative offsets.
  Bounded by total group membership, not by the number of directories in
  the tree squared.
- **Two-tier output, not a flat list.** `FolderMatch::Exact { folders:
  Vec<PathBuf> }` for a cluster of 2+ mutually-identical directories
  (grouped by exact signature equality — same `(rel_path, group_id)` set);
  `FolderMatch::Contained { subset: PathBuf, superset: PathBuf }` for a
  strict, one-directional containment (excluded from `Exact` reporting
  when the sides are actually equal — that's already covered there).
- **Shallowest-first, claim-and-skip-descendants redundancy suppression.**
  Without this, a genuine top-level folder match would recursively imply
  a separate reported match for *every* subdirectory pair inside it —
  correct, but useless noise at any real depth. Directories are
  considered in increasing path-depth order; once a directory is claimed
  by a reported `Exact` cluster or as a `Contained` subset, none of its
  descendants are considered as further subset/exact candidates (a match
  found among them would be entirely implied by the parent-level one
  already reported). A directory that was never itself claimed still has
  its descendants considered normally, so an independent match nested
  inside an otherwise-unmatched directory is still found. Superset
  directories are *not* claimed this way — the same folder can legitimately
  be the superset for several unrelated subset matches.
- **Detection and reporting only — no new action kind.** This ADR
  deliberately does not add a "delete/replace this whole folder" action.
  The action layer (ADR-0009) still operates per-file; a user acting on a
  reported folder match today does so via the existing per-file
  delete/hardlink/reflink actions on the files that folder match's
  underlying `DuplicateGroup`s point at. A folder-level action is a real,
  separate, more dangerous capability (recursive delete/replace) — out of
  scope here, not attempted.
- **CLI first, GUI as a follow-up.** `rusty_fclone-cli` gets a new
  `--find-duplicate-folders` flag (text and `--format json` output); the
  GUI is a natural next surface for the same capability but is scoped as
  a separate follow-up, not bundled into this change.

## Consequences

- New `rusty_fclone-core` module, `folder_dedup`, with its own public
  types (`FolderMatch`, `FolderMatchKind`) — no change to `scan()`,
  `ScanEvent`, or any existing type's shape.
- A caller who wants folder-level results pays for a second, stat-only
  traversal of the tree (no re-hashing) — acceptable since it's entirely
  opt-in (`--find-duplicate-folders`), not part of the default scan path.
- Redundancy suppression means the reported set is deliberately not
  "every true relationship" — a nested match fully implied by an
  already-reported ancestor match is intentionally omitted. This is a
  usability decision, not a completeness bug: the implied nested matches
  are still true, just not separately listed.
- Empty directories are never matched (a folder-duplicate concept is
  about file content; this mirrors the base engine already ignoring
  directories as non-file entities).
- No new destructive capability — this ADR's scope is strictly detection
  and reporting.
