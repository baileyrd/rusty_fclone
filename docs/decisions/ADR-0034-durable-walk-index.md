# ADR-0034: Durable per-directory walk index

- Status: Accepted
- Date: 2026-10-27
- Related: ADR-0012 (traversal must finish before hashing), ADR-0016 (redb
  hash cache), ADR-0033 (compact traversal storage)

## Context

On a ~1.3M-file NTFS (ntfs-3g) drive, the walk alone runs long, and a
killed run lost all of it: only full-file hashes were ever persisted, and
hashing starts only after the walk ends (ADR-0012). Re-runs paid the full
walk again too. On FUSE, the per-file `stat` dominates walk cost; a
directory listing is comparatively cheap.

## Decision

- With `--cache`, the same redb file also holds `walk_index_v1`: per
  directory, its mtime plus `(name, size, dev, ino)` of each regular file.
- jwalk's `process_read_dir` callback now `stat`s a directory's files on
  the worker thread that listed it (so stats run in parallel). If the
  directory's mtime equals its recorded one, recorded stats are reused and
  no file is `stat`-ed. Otherwise each file is `stat`-ed and, if all
  succeed, the directory is queued for recording.
- A writer thread commits queued directories every 512 directories or 2 s,
  so a kill loses at most that much of the walk.
- Directories modified within the last 2 s aren't recorded: a change in
  the same mtime tick after listing would otherwise go unseen ("racy git").
- `--rescan-all` stats everything and refreshes the index.
- `WalkIndex` holds the database weakly: jwalk may drop its callback after
  the walk returns, and a strong reference would keep the redb file locked
  against the next in-process scan (GUI).

## Consequences

- Measured on a 400k-file tree: a re-run makes 420 `stat` calls instead of
  ~400k; a run killed partway resumed with ~106k files already recorded.
- Adding, removing, or renaming a file bumps its directory's mtime, so the
  listing is always current. A file rewritten in place is not: it's
  grouped under its recorded size until `--rescan-all`. This can hide a
  duplicate but never invent one -- every file that reaches hashing is
  `stat`-ed and read live, and `plan apply` re-checks size and mtime.
- Records for deleted directories are never pruned; at roughly 50 bytes
  per file this stays small next to the hash cache.
- Paths that aren't UTF-8, and non-Unix file ids, aren't
  recorded; those directories are simply `stat`-ed every time.
