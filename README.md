# rusty_fclone

A duplicate-file finder — a spiritual successor to
[fclones](https://github.com/pkolaczk/fclones), starting from the fastest
practical detection engine and growing an action layer (delete/hardlink/
reflink duplicates) on top of it.

## Status

Early: the detection engine and a minimal CLI exist and work; there is no
action layer yet (nothing deletes or links files — this only *reports*
duplicates). See [`docs/PROJECT-STATUS.md`](docs/PROJECT-STATUS.md) for the
current checkpoint and [`docs/roadmap/ROADMAP.md`](docs/roadmap/ROADMAP.md)
for what's planned.

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
          Number of worker threads in the I/O-bound read pool
          [default: number of CPU cores]
  -h, --help                         Print help
  -V, --version                      Print version
```

## Architecture

See [`docs/architecture/SYSTEM-ARCHITECTURE.md`](docs/architecture/SYSTEM-ARCHITECTURE.md)
for the detection pipeline, and [`docs/decisions/`](docs/decisions/) for the
ADRs behind it (staged hashing + xxh3-128, cross-platform I/O, the two-pool
concurrency model, traversal defaults, workspace shape, toolchain/license/
dependency policy, and two benchmark-motivated tuning revisions — partial-
hash sample size and I/O thread pool sizing).

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

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
