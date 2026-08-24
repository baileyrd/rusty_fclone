#!/usr/bin/env bash
# Compares the rusty-fclone CLI against upstream fclones on identical
# synthetic directory trees (see gen_bench_trees.py). Requires `fclones`
# and `hyperfine` on PATH -- both installable via:
#   cargo binstall fclones hyperfine
# (or `cargo install fclones hyperfine` to build from source).
#
# Usage: scripts/bench-vs-fclones.sh [output-dir]
# Writes one Markdown results file per scenario into output-dir
# (default: ./bench-results).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-"$REPO_ROOT/bench-results"}"
TREES_DIR="$(mktemp -d)"
trap 'rm -rf "$TREES_DIR"' EXIT

command -v fclones >/dev/null || { echo "fclones not found on PATH" >&2; exit 1; }
command -v hyperfine >/dev/null || { echo "hyperfine not found on PATH" >&2; exit 1; }

echo "Building rusty-fclone in release mode..."
cargo build --release -p rusty_fclone-cli --manifest-path "$REPO_ROOT/Cargo.toml"
RF="$REPO_ROOT/target/release/rusty-fclone"

echo "Generating synthetic trees under $TREES_DIR..."
python3 "$REPO_ROOT/scripts/gen_bench_trees.py" "$TREES_DIR"

mkdir -p "$OUT_DIR"

for scenario in many_small_duplicates many_unique_small_files few_large_duplicates mixed_realistic_tree; do
    dir="$TREES_DIR/$scenario"
    echo
    echo "=== $scenario ==="
    hyperfine \
        --warmup 3 \
        --min-runs 10 \
        --export-markdown "$OUT_DIR/$scenario.md" \
        -n "rusty-fclone" "$RF '$dir' > /dev/null" \
        -n "fclones (default: metro hash)" "fclones group '$dir' > /dev/null" \
        -n "fclones (xxhash, matched hash algorithm)" "fclones group --hash-fn xxhash '$dir' > /dev/null"
done

echo
echo "Results written to $OUT_DIR/*.md"
