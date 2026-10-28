# ADR-0036: Index-driven walk: skip listing unchanged directories

- Status: Accepted
- Date: 2026-10-28
- Related: ADR-0003 (traversal defaults), ADR-0034 (durable walk index),
  ADR-0035 (bounded-memory traversal)

## Context

ADR-0034 made a re-run skip the per-file `stat`, but every directory was
still listed: jwalk reads a directory before any callback can see it. On
the 22M-file ntfs-3g drive a re-run was walking at roughly 75 syscalls/s
after an hour, almost all of it directory listing, where each FUSE
`opendir`/`readdir`/`releasedir` reads the directory's index from a
rotational disk. Measured on a 4M-file, 40k-directory tree, a warm re-run
made six filesystem calls per directory (`openat`, two `getdents64`,
`fstat`, `close`, `statx`).

## Decision

- Walk records now also hold the names of the directory's real
  subdirectories, appended after the files. A record without that section
  (written by v0.4) still reuses its file stats, but the directory is
  listed and the record is rewritten in the new form.
- jwalk is replaced by a small walker in `traversal`: one rayon task per
  directory, candidates sent over the existing bounded queue. Each
  directory is `stat`-ed once, by its parent, for its mtime. When the mtime
  matches its record, the record supplies both the listing and the file
  stats, so the directory isn't opened at all.
- `exclude_paths` are applied to every listing, recorded or read. A
  directory with an excluded file isn't recorded, so a narrower scan never
  narrows the record.
- With `follow_symlinks`, directories are always listed, since a symlink's
  target can change without the parent's mtime changing. Such walks reuse
  file stats only and record no subdirectories, so a later non-following
  walk never descends a symlink. Loops are refused by checking each
  directory's file id against its ancestors, and reported as errors.
- A subdirectory on another filesystem is no longer entered unless
  `cross_filesystems` is set (ADR-0003). Before, it was walked and its
  files were dropped one by one.

## Consequences

- On the 4M-file tree, a warm re-run makes one `statx` per directory
  (40,216 calls) instead of about 241k filesystem calls, and drops from
  1.3 s to 0.7-1.0 s on a local SSD. The saving is larger where each call
  is a FUSE round trip to a spinning disk. A cold walk is unchanged (6.3 s
  vs 6.5 s), and peak RSS fell from 94 MB to 80 MB.
- The first run after upgrading lists every directory once to upgrade its
  record (2.4 s on that tree), then skips listing from the next run on.
- The walk index's blind spot is unchanged (ADR-0034): a file rewritten in
  place in an untouched directory keeps its recorded size until
  `--rescan-all`.
- One fewer dependency. Parallelism is rayon's global pool, as jwalk's
  default was.
