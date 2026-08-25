# rusty_fclone

A duplicate-file finder — a spiritual successor to
[fclones](https://github.com/pkolaczk/fclones): a fast detection engine
plus an action layer to delete, hardlink, or reflink what it finds,
usable from either a CLI or a desktop GUI.

## Status

Detection (staged hashing, benchmarked faster than fclones on most
workloads — see below), an action layer (delete/hardlink/reflink, dry-run
by default), richer CLI output (JSON, progress reporting, an interactive
confirmation prompt), a desktop GUI, an opt-in incremental hash cache,
opt-in SQLite scan-history, and opt-in import of an existing fclones hash
cache are all implemented. See
[`docs/PROJECT-STATUS.md`](docs/PROJECT-STATUS.md) for the current
checkpoint and [`docs/roadmap/ROADMAP.md`](docs/roadmap/ROADMAP.md) for
what's planned.

**By default, this tool only reports — it never deletes or links anything
unless you pass both `--action <delete|hardlink|reflink>` and `--apply`.**

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
          one row after the scan completes. Off by default
      --action <ACTION>
          What to do with redundant copies once a group is confirmed:
          report (default, just print groups), delete, hardlink, or
          reflink (copy-on-write clone, CoW-capable filesystems only).
          Without --apply, delete/hardlink/reflink only preview what would
          happen.
      --apply
          Actually perform --action's effect (required in addition to
          --action delete/hardlink/reflink — a two-flag confirmation so a
          single typo can't cause data loss)
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

# Reclaim the space without losing any path -- replace redundant copies
# with hardlinks to the kept file instead of deleting them.
rusty-fclone --action hardlink --apply /path/to/scan

# Same, but as a copy-on-write clone instead of a hardlink (CoW-capable
# filesystems only -- Btrfs, XFS with reflink, APFS, some ZFS setups).
rusty-fclone --action reflink --apply /path/to/scan

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
```

## GUI

A desktop GUI (`rusty_fclone-gui`, [`GUI-UX-001`](docs/specifications/gui-ux/GUI-UX-001.md))
covers the same scan-and-act workflow as the CLI above, through a window
instead of a terminal: enter a directory, scan, review duplicate groups as
they're found, and preview or apply an action per group.

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

No installer/bundle is published yet (`.deb`/`.AppImage`/`.dmg`/`.msi`) —
run it from source via `cargo run` above.

## Architecture

See [`docs/architecture/SYSTEM-ARCHITECTURE.md`](docs/architecture/SYSTEM-ARCHITECTURE.md)
for the detection pipeline, and [`docs/decisions/`](docs/decisions/) for the
ADRs behind it: staged hashing + xxh3-128, cross-platform I/O, the two-pool
concurrency model, traversal defaults, workspace shape, toolchain/license/
dependency policy, two benchmark-motivated tuning revisions (partial-hash
sample size, I/O thread pool sizing), the action layer (ADR-0009:
delete/hardlink, dry-run by default, safe hardlink-via-rename), and the
Tauri-based GUI (ADR-0020).

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
