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
      --follow-symlinks              Follow symbolic links during traversal
      --cross-filesystems            Cross filesystem/mount-point boundaries during traversal
      --verify                       Byte-compare hash-matched files before reporting them as duplicates
      --small-file-threshold <BYTES> Files at or below this size skip the partial-hash stage [default: 131072]
  -h, --help                         Print help
  -V, --version                      Print version
```

## Architecture

See [`docs/architecture/SYSTEM-ARCHITECTURE.md`](docs/architecture/SYSTEM-ARCHITECTURE.md)
for the detection pipeline, and [`docs/decisions/`](docs/decisions/) for the
ADRs behind it (staged hashing + xxh3-128, cross-platform I/O, the two-pool
concurrency model, traversal defaults, workspace shape, and toolchain/
license/dependency policy).

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
this crate's own history — not yet a measured comparison against fclones
(see `DETECTION-BENCHMARK-VS-FCLONES` in
[`docs/roadmap/ROADMAP.md`](docs/roadmap/ROADMAP.md)). CI only compiles the
benchmarks (`cargo bench --no-run`) on every push; run the command above
locally for real numbers.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
