# rusty_fclone

A duplicate-file finder — a spiritual successor to
[fclones](https://github.com/pkolaczk/fclones): a fast detection engine
plus an action layer to delete, trash, hardlink, reflink, move, or copy
what it finds, usable from either a CLI or a desktop GUI.

## Status

Detection (staged hashing, benchmarked faster than fclones on most
workloads — see below), an action layer (delete/trash/hardlink/reflink/
move/copy, dry-run by default, rule-based keep selection, a protected/
reference-folder guardrail), richer CLI output (JSON, progress reporting,
an interactive confirmation prompt), a desktop GUI with inline media
preview for image and audio duplicates, an opt-in incremental hash cache,
opt-in SQLite scan-history with per-action audit detail and a `history`
query subcommand, opt-in import of an existing fclones hash cache, opt-in
folder-level duplicate detection, and include/exclude scan filters
(min/max size, extension, excluded paths) are all implemented. See
[`docs/PROJECT-STATUS.md`](docs/PROJECT-STATUS.md) for the current
checkpoint and [`docs/roadmap/ROADMAP.md`](docs/roadmap/ROADMAP.md) for
what's planned.

**By default, this tool only reports — it never deletes, trashes, links,
moves, or copies anything unless you pass both `--action
<delete|trash|hardlink|reflink|move|copy>` and `--apply`.**

## Usage

```sh
cargo run -p rusty_fclone-cli -- <path-to-scan>
```

```
Usage: rusty-fclone [OPTIONS] <ROOT>

Arguments:
  <ROOT>  Directory to scan for duplicates

Options:
      --follow-symlinks
          Follow symbolic links during traversal
      --cross-filesystems
          Cross filesystem/mount-point boundaries during traversal
      --verify
          Byte-compare hash-matched files before reporting them as duplicates
      --small-file-threshold <BYTES>
          Files at or below this size (bytes) skip the partial-hash stage [default: 131072]
      --partial-hash-sample-size <BYTES>
          Bytes sampled at the head, middle, and tail of a file during the
          partial-hash stage, for files larger than --small-file-threshold
          [default: 16384]
      --io-threads <N>
          Number of worker threads in the I/O-bound read pool. If omitted,
          auto-detected from the scan root's filesystem: oversubscribed on
          a rotational disk (Linux only, best-effort), core count otherwise
      --cache <PATH>
          Path to a full-file-hash cache (created if it doesn't exist). When
          set, a file whose size and modified-time haven't changed since a
          previous scan reuses that scan's hash instead of being re-read
          and re-hashed. Off by default
      --import-fclones-cache <PATH>
          Path to an existing `fclones --cache` database (e.g.
          `~/.cache/fclones` on Linux) to import full-file hashes from, so
          a tree fclones already scanned with `--hash-fn xxhash` doesn't
          need re-hashing here. Independent of --cache: usable on its own,
          or with --cache so an imported hash also persists for future
          rusty-fclone-only re-scans. Off by default
      --history <PATH>
          Path to a SQLite scan-history database (created if it doesn't
          exist). When set, a summary of this scan (files/bytes scanned,
          duplicate groups/files, and any action's result) is appended as
          one row after the scan completes, plus one row per file/pair an
          applied action actually acted on. Off by default. Query it back
          with `rusty-fclone history <list|stats>` -- see below
      --min-size <BYTES>
          Skip files smaller than this size (bytes). Applied during
          traversal, before any hashing
      --max-size <BYTES>
          Skip files larger than this size (bytes). Applied during
          traversal, before any hashing
      --include-ext <EXT>
          Only scan files with this extension (case-insensitive, without
          the leading `.`). Repeatable
      --exclude-ext <EXT>
          Skip files with this extension (case-insensitive, without the
          leading `.`), even if --include-ext would otherwise allow them.
          Repeatable
      --exclude-path <PATH>
          Skip this path and everything beneath it entirely -- not just
          from the results, but from traversal itself. Repeatable
      --find-duplicate-folders
          After the scan completes, also look for folders whose entire
          recursive file content duplicates -- or is a subset of --
          another folder's. Off by default
      --action <ACTION>
          What to do with redundant copies once a group is confirmed:
          report (default, just print groups), delete (permanent, no
          recovery path -- prefer trash unless this is specifically
          wanted), trash (move to the OS trash/recycle bin, recoverable),
          hardlink, reflink (copy-on-write clone, CoW-capable filesystems
          only), move (relocate into --archive-dir, mirroring its
          original path), or copy (archive into --archive-dir, leaving
          the original untouched -- reclaims nothing). Without --apply,
          delete/trash/hardlink/reflink/move/copy only preview what
          would happen. move/copy also require --archive-dir.
      --apply
          Actually perform --action's effect (required in addition to
          --action delete/trash/hardlink/reflink/move/copy — a two-flag
          confirmation so a single typo can't cause data loss)
      --keep-rule <RULE>
          Which copy to keep in each group when --action is set: alphabetical
          (default), newest, oldest, shortest-path, or longest-path. Applied
          across every group in one pass. No effect in the default Report
          mode, which doesn't designate a kept file at all
      --reference <PATH>
          Mark this path (a file or directory subtree) as protected -- never
          acted on. Repeatable. Overrides --keep-rule: a group containing a
          protected path always keeps it, and every other protected copy is
          excluded from the action too. A hard guardrail, not a suggestion --
          can't be bypassed by --keep-rule or path sort order
      --archive-dir <PATH>
          Destination folder for --action move/copy. Every redundant copy is
          relocated (move) or duplicated (copy) underneath this directory,
          mirroring its original path so files with the same name from
          different directories never collide. Required by, and only
          meaningful with, --action move/copy
  -y, --yes
          Skip the interactive confirmation prompt normally shown before
          --apply mutates anything
      --format <FORMAT>
          Output format: text (default, human-readable) or json
          (NDJSON, one object per line, machine-readable)
  -v, --verbose...
          Increase log verbosity (-v info, -vv debug, -vvv trace). Ignored
          if RUST_LOG is set, which always takes precedence
  -h, --help     Print help
  -V, --version  Print version
```

Examples:

```sh
# Preview what deleting redundant copies would do -- touches nothing.
rusty-fclone --action delete /path/to/scan

# Actually delete them, keeping one (alphabetically-first) copy per group,
# without the interactive confirmation prompt.
rusty-fclone --action delete --apply --yes /path/to/scan

# Same, but recoverable -- move redundant copies to the OS trash/recycle
# bin instead of deleting them outright.
rusty-fclone --action trash --apply /path/to/scan

# Keep the most recently modified copy in each group instead of the
# alphabetically-first one -- applied across every group in one pass.
rusty-fclone --action trash --keep-rule newest --apply /path/to/scan

# Never touch anything under a "master" archive folder, no matter which
# copy --keep-rule would otherwise pick -- the protected copy is always
# kept and every other copy of it is trashed instead.
rusty-fclone --action trash --reference /path/to/originals --apply /path/to/scan

# Reclaim the space without losing any path -- replace redundant copies
# with hardlinks to the kept file instead of deleting them.
rusty-fclone --action hardlink --apply /path/to/scan

# Same, but as a copy-on-write clone instead of a hardlink (CoW-capable
# filesystems only -- Btrfs, XFS with reflink, APFS, some ZFS setups).
rusty-fclone --action reflink --apply /path/to/scan

# Consolidate redundant copies into one folder instead of deleting them --
# relocated, not destroyed, and reclaims space at the scanned location
# just like delete/trash.
rusty-fclone --action move --archive-dir ~/duplicates-archive --apply /path/to/scan

# Cautious two-step cleanup: archive a copy of every redundant file first
# (originals untouched, nothing reclaimed yet), review the archive, then
# run a second --action delete/trash pass once you trust it's complete.
rusty-fclone --action copy --archive-dir ~/duplicates-archive --apply /path/to/scan

# Machine-readable NDJSON output, for piping into another tool.
rusty-fclone --format json /path/to/scan | jq .

# Speed up a repeated scan of the same tree: reuse full-file hashes from
# the last run instead of re-reading and re-hashing unchanged files.
rusty-fclone --cache ~/.cache/rusty-fclone/hashes.redb /path/to/scan

# Already have an fclones hash cache for this tree (from `fclones group
# --cache --hash-fn xxhash`)? Import it instead of starting from scratch.
rusty-fclone --import-fclones-cache ~/.cache/fclones /path/to/scan

# Keep a longer-term record of every scan (files/bytes scanned, duplicates
# found, action results) in a queryable SQLite database.
rusty-fclone --history ~/.local/share/rusty-fclone/history.sqlite /path/to/scan

# Skip node_modules/.git entirely (not even traversed), ignore anything
# under 1 KB, and only consider photos.
rusty-fclone --exclude-path ./node_modules --exclude-path ./.git \
  --min-size 1024 --include-ext jpg --include-ext png --include-ext heic \
  /path/to/scan

# Also report whole folders that are duplicates (or subsets) of each other
# -- e.g. a Photos/2024/vacation folder copied wholesale into a backup tree.
rusty-fclone --find-duplicate-folders /path/to/scan

# Combine the two: delete a whole duplicate folder in one shot instead of
# its files one at a time. Individually-acted-on files are skipped once
# their folder is already covered by a folder match; unrelated duplicate
# pairs outside any folder match are still acted on normally.
rusty-fclone --find-duplicate-folders --action delete --apply /path/to/scan
```

### Querying scan history

`--history <path>` records to a SQLite database; `rusty-fclone history
<SUBCOMMAND>` reads it back. `history` is a reserved top-level command --
`rusty-fclone <root>` still works exactly as before for any other first
argument, a real directory named `history` just needs `./history` or an
absolute path to disambiguate.

```sh
# The 20 most recent scans, newest first.
rusty-fclone history --db ~/.local/share/rusty-fclone/history.sqlite list

# Aggregate totals (scans, bytes reclaimed, files acted on, ...) across
# every scan started in a given window (Unix timestamps).
rusty-fclone history --db ~/.local/share/rusty-fclone/history.sqlite \
  stats --since 1735689600 --until 1738368000

# Machine-readable, same convention as the main command's --format json.
rusty-fclone history --db ~/.local/share/rusty-fclone/history.sqlite \
  --format json list --limit 5 | jq .
```

## GUI

A desktop GUI (`rusty_fclone-gui`, [`GUI-UX-001`](docs/specifications/gui-ux/GUI-UX-001.md))
covers the same scan-and-act workflow as the CLI above, through four
screens instead of a terminal: a Dashboard summarizing duplicates found
and space reclaimed this session, Scan Setup for picking a directory and
tuning options, Duplicate Review for stepping through file- and
folder-level duplicate matches and previewing or applying an action on
each, and a Rules & Automation preview. Duplicate Review's compare-cards
show an inline thumbnail for supported image files and a playable audio
control for supported audio files, instead of just a filename — falling
back to a generic file icon for unsupported types, oversized files, or
video. Light and dark themes.

```sh
cargo run -p rusty_fclone-gui
```

On Linux, building it needs the system webview development packages
first (Tauri's backend links against them — see
[ADR-0020](docs/decisions/ADR-0020-gui-via-tauri.md)):

```sh
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev libssl-dev libsoup-3.0-dev
```

On Windows, building it needs the MSVC C++ toolchain (a transitive
dependency, `embed-resource`, uses it to embed the app icon/manifest into
the `.exe`) — install the **"Desktop development with C++"** workload via
the Visual Studio Installer, and build from an **"x64 Native Tools
Command Prompt for VS"**, not a plain terminal (`cargo build` invoked
outside that environment fails with `cl.exe`/`windows.h` errors, since
`vcvars64.bat` hasn't run to set `INCLUDE`/`LIB`).

If installing Visual Studio isn't an option (e.g. no admin rights), the
GNU target works too: install a MinGW-w64 GCC (e.g. via
[MSYS2](https://www.msys2.org/), which can be installed to a user-owned
directory without admin), then:

```sh
rustup target add x86_64-pc-windows-gnu
rustup toolchain install stable-x86_64-pc-windows-gnu
```

That sidesteps the MSVC-only `vswhom-sys` dependency entirely — see
[ADR-0020](docs/decisions/ADR-0020-gui-via-tauri.md)'s consequences for
both paths.

No installer/bundle is published yet (`.deb`/`.AppImage`/`.dmg`/`.msi`) —
run it from source via `cargo run` above.

## Architecture

See [`docs/architecture/SYSTEM-ARCHITECTURE.md`](docs/architecture/SYSTEM-ARCHITECTURE.md)
for the detection pipeline, and [`docs/decisions/`](docs/decisions/) for the
ADRs behind it: staged hashing + xxh3-128, cross-platform I/O, the two-pool
concurrency model, traversal defaults, workspace shape, toolchain/license/
dependency policy, two benchmark-motivated tuning revisions (partial-hash
sample size, I/O thread pool sizing), the action layer (ADR-0009:
delete/hardlink, dry-run by default, safe hardlink-via-rename), the
Tauri-based GUI (ADR-0020), folder-level duplicate detection
(ADR-0021: a post-scan pass, not a streaming extension), and the GUI's
4-screen redesign against a design handoff (ADR-0022).

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

On Linux, `--workspace` now includes `rusty_fclone-gui`, so the system
webview packages listed under [GUI](#gui) above need to be installed
first.

The Rust toolchain is pinned via `rust-toolchain.toml`. See
[`AGENTS.md`](AGENTS.md) for repository conventions and
[`WORKFLOW.md`](WORKFLOW.md) for the development process.

## Benchmarks

```sh
cargo bench -p rusty_fclone-core
```

Runs the Criterion suite in
[`crates/rusty_fclone-core/benches/detection.rs`](crates/rusty_fclone-core/benches/detection.rs)
over four synthetic scan scenarios (many small duplicates, many unique
small files, few large duplicates, a mixed realistic tree), reporting
files/sec or bytes/sec. These are relative/regression benchmarks against
this crate's own history. CI only compiles the benchmarks (`cargo bench
--no-run`) on every push; run the command above locally for real numbers.

For a measured comparison against upstream fclones on the same synthetic
trees, see [`docs/benchmarks/FCLONES-COMPARISON.md`](docs/benchmarks/FCLONES-COMPARISON.md)
(reproduce with `scripts/bench-vs-fclones.sh`, requires `fclones` and
`hyperfine` on `PATH` — `cargo binstall fclones hyperfine`). Current result:
~2.6–2.7x faster than fclones on small-file-heavy trees, within measurement
noise on a large-file scenario.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
