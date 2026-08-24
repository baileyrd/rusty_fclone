# ADR-0003: Traversal defaults — symlinks and filesystem boundaries

- Status: Accepted
- Date: 2026-08-24

## Context

Two traversal defaults materially affect both correctness and safety:

1. **Symlinks.** Following them risks cycles and double-reporting the same
   inode as its own duplicate. Skipping them is safer but misses real
   dedup opportunities some users want.
2. **Filesystem/mount boundaries.** Crossing into other mounted filesystems
   by default (like plain `find`) is more thorough but riskier — it can
   wander onto slow network mounts or unrelated volumes the user didn't mean
   to scan.

## Decision

- **Symlinks**: skipped by default. `ScanOptions::follow_symlinks` (CLI:
  `--follow-symlinks`) opts in. jwalk's `follow_links(false)` means a
  symlinked entry's `file_type()` reports the symlink itself, not its
  target, so the existing "regular files only" filter in
  `traversal::traverse` already excludes them with no extra logic.
- **Filesystem boundaries**: traversal stays on the filesystem the scan root
  is on, unless `ScanOptions::cross_filesystems` (CLI: `--cross-filesystems`)
  is set. Implemented via the `file-id` crate: each candidate's platform
  file-id carries a device/volume component (`device_id` on Unix,
  `volume_serial_number` on Windows), compared against the root's.
- **Free correctness win**: the same file-id lookup used for the mount-
  boundary check also identifies existing hardlinks (ADR-0001's pre-hashing
  collapse step) — one mechanism serves both purposes.

## Consequences

- A scan silently stops at mount points by default; users scanning a tree
  that spans multiple filesystems on purpose must pass
  `--cross-filesystems`.
- Symlinked *directories* are never descended into by default, and symlinked
  *files* are never treated as scan candidates by default. `--follow-symlinks`
  changes both.
- No cycle-detection code exists yet for the `follow_symlinks = true` case
  beyond what jwalk itself provides. This is a known gap tracked in the
  roadmap, not exercised by v1's default configuration.
