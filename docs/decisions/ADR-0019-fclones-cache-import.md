# ADR-0019: Import full-file hashes from an fclones cache

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0016 (the incremental hash cache this feeds into),
  ADR-0004 (error-tolerance contract this follows)

## Context

A user migrating from upstream fclones (or simply also using it) may
already have a populated `fclones --cache` database for a tree. Re-running
`rusty-fclone` against the same tree would otherwise re-read and re-hash
every file from scratch, even though fclones already paid that cost.

fclones' cache has no documented on-disk format or public API for reading
it from another program. Everything below was reverse-engineered directly
from fclones 0.35.0's own source (`cache.rs`, `hasher.rs`, `file.rs`,
`group.rs`, `device.rs`, and the `typed-sled` crate it depends on) and then
verified against a real database this cache produces, not assumed:

- It's a `sled` database (pinned to `sled = "0.34"`, matching fclones'
  own pin exactly, since sled's on-disk format is not guaranteed portable
  across arbitrary version gaps) with one tree per
  `(hash function, transform)` pair, named `hash_db:{Debug of HashFn}:{transform
  or "<none>"}` -- e.g. `hash_db:Xxhash:<none>`.
- Keys and values are `bincode` (v1, default/fixint config) encodings of
  fclones' own `Key { file_id: FileId, chunk_pos: FilePos, chunk_len:
  FileLen }` and `CachedFileInfo { modified_timestamp_ms, file_len,
  data_len, hash }` structs. `FileId` is `{ device: u64, inode: u64 }` on
  Unix (`u128` inode on Windows -- out of scope here, see below).
  `FilePos`/`FileLen` are single-field newtypes that serde/bincode encode
  transparently as a plain `u64`.
- The hash itself serializes as a **hex string of its little-endian
  bytes** (`FileHash`'s `Serialize` impl does `collect_str(hex::encode(&self.0))`,
  and `FileHash::from(u128)` writes the value via
  `write_u128::<LittleEndian>`) -- not the integer's standard big-endian
  hex representation. Verified independently: computing
  `xxhash_rust::xxh3::xxh3_128` directly over a test file's bytes and
  formatting the result as `hex::encode(value.to_le_bytes())` produced a
  string that matched, character for character, both fclones' JSON report
  output for that file and the hex string stored in its cache.
- Only fclones' `xxhash3` hash function (`--hash-fn xxhash`, tree name
  `hash_db:Xxhash:<none>`) computes the same digest this project already
  does: fclones' `HashFn::Xxhash` hashes with the exact same
  `xxhash_rust::xxh3` crate `rusty_fclone-core` depends on for its own
  hashing. Every other fclones hash function (its default `metro`,
  plus `blake3`/`sha256`/`sha512`/`sha3-256`/`sha3-512`) computes a
  genuinely different digest and is never read.
- A cache entry is fclones' real full-content hash (from its
  `group_by_contents` stage, `FileChunk::new(path, FilePos(0), fi.len)`)
  only for a file at or above fclones' own prefix-sample length
  (`group_by_contents(&ctx, prefix_len, ...)` in `group.rs` -- files
  smaller than `prefix_len` are filtered out of that stage entirely).
  A smaller file instead only ever gets a *prefix*-hash entry, keyed by
  the **unclamped** prefix length itself (`group.rs`'s `group_by_prefix`
  requests `prefix_len` bytes regardless of the file's actual size) --
  the read naturally stops at EOF, so the resulting hash *is* the
  full-content hash even though the key says `chunk_len == prefix_len`,
  not `chunk_len == size`. Verified against a real 30-byte two-file
  duplicate pair: fclones cached exactly one entry per file, each keyed
  `chunk_len = 16384` (its documented HDD/unknown-device default prefix
  length; `device.rs` also has 4096 for SSD), not `chunk_len = 30`.

## Decision

- **A read-only `sled`/`bincode` reimplementation of just enough of
  fclones' schema to look up one value, not a dependency on fclones
  itself or on `typed-sled`**: fclones isn't a library usable this way,
  and `typed-sled` is a thin thirty-line wrapper not worth a dependency
  for. `rusty_fclone-core::fclones_import` mirrors the two relevant
  structs field-for-field instead.
- **Opt-in via `--import-fclones-cache <path>` /
  `ScanOptions::fclones_import_path: Option<PathBuf>`**, off by default,
  matching every other flag in this project. Independent of `--cache`:
  usable standalone for a one-off import (saves work for this scan only),
  or together with `--cache` so an imported hit is also persisted into
  rusty-fclone's own cache for future rusty-fclone-only re-scans (reusing
  the existing `cache_write` plumbing from ADR-0016 -- an import hit is
  treated exactly like a freshly-computed hash for that purpose).
- **Tried after a `--cache` miss, before any real file I/O**: cheapest
  option first, matching the existing full-hash stage's ordering.
- **A lookup tries `chunk_len == size` first, then fclones' two
  documented default prefix lengths (4 KiB, 16 KiB) when `size` is small
  enough that one of them would have covered the whole file**: this
  recovers the common small-file case above without ever risking a wrong
  result -- every candidate, including the guessed ones, is still gated
  by the same `(file_len, modified_timestamp_ms)` staleness check fclones
  itself performs. A file cached under an explicit non-default
  `--max-prefix-size` isn't recoverable this way (that value isn't
  recorded anywhere the on-disk format exposes) and is a missed
  optimization, never a wrong hash.
- **Unix only (`file-id`'s `FileId::Inode` variant)**: fclones' own
  Windows file identity encoding (`device: u64, inode: u128` from its own
  platform-specific code, not from the `file-id` crate this project
  already depends on) isn't exposed by any variant `file-id` produces on
  Windows, so reproducing it correctly wasn't attempted here. Any
  non-`Inode` `FileId` (i.e. any Windows result) is a clean miss, never
  an error.
- **A missing cache directory is treated as a loud warning, not silently
  created**: unlike `--cache` (where creating a fresh database on first
  run is the point), `sled::open`'s own create-if-missing behavior would
  otherwise let a typo'd `--import-fclones-cache` path silently produce
  an empty database with zero indication anything was wrong. An explicit
  existence check runs first.
- **Every failure mode -- missing path, unreadable database, missing
  tree, undecodable entry, unsupported platform -- degrades to a clean
  miss (falling through to a real hash), never aborts the scan**,
  matching ADR-0004/ADR-0016's error-tolerance stance.

## Consequences

- New dependencies: `sled = "0.34"` (pinned to match fclones' own pin,
  since sled's format compatibility isn't guaranteed across bigger
  version gaps) and `bincode = "1"`, `rusty_fclone-core` only.
- `ScanOptions` gains a `fclones_import_path: Option<PathBuf>` field; the
  CLI gains `--import-fclones-cache <path>`.
- Verified end-to-end against the real `fclones` 0.35.0 binary in this
  development environment, not just against hand-built fixtures: a real
  `fclones group --cache --hash-fn xxhash` run against both a small
  (30-byte) duplicate pair and a larger (50 KB) duplicate pair produced
  cache databases that `rusty-fclone --import-fclones-cache <dir>`
  correctly recognized (`fclones cache import hit` trace events for every
  file, correct duplicate group reported either way) -- exercising both
  the exact-match path and the default-prefix-length fallback path this
  ADR describes. `cargo test` additionally covers both from hand-built
  fixtures (portable to CI, which has no `fclones` binary), the
  non-default-prefix-size miss case, the wrong-hash-function-tree miss
  case, staleness (size/mtime) invalidation, and the nonexistent-path
  warning path.
- This is a read path only: nothing here ever writes to fclones' own
  cache, and a lookup running while fclones itself is actively writing to
  the same database is not a scenario this was designed or tested for
  (matches ADR-0016's similar non-goal for concurrent access to
  rusty-fclone's own cache).
