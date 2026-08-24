#!/usr/bin/env python3
"""Generates the same synthetic directory trees used by
crates/rusty_fclone-core/benches/detection.rs, as real on-disk files, so an
external tool (e.g. fclones) can be pointed at an identical corpus for a
fair comparison. See docs/benchmarks/FCLONES-COMPARISON.md.

Usage: gen_bench_trees.py <output-dir>

Creates <output-dir>/{many_small_duplicates,many_unique_small_files,
few_large_duplicates,mixed_realistic_tree}/.
"""
import os
import sys


def filler_content(seed: int, size: int) -> bytes:
    """Deterministic, non-degenerate filler -- mirrors filler_content() in
    benches/detection.rs byte-for-byte, so both benchmark harnesses exercise
    identical content. The first 8 bytes encode `seed` as a little-endian
    u64 so distinct seeds are guaranteed distinct content for size >= 8 --
    see the comment on the Rust version for why the ramp pattern alone
    isn't enough (it only varies by `seed mod 256`)."""
    out = bytearray(size)
    seed_bytes = (seed & 0xFFFFFFFFFFFFFFFF).to_bytes(8, "little")
    prefix_len = min(len(seed_bytes), size)
    out[:prefix_len] = seed_bytes[:prefix_len]
    for i in range(prefix_len, size):
        out[i] = (seed * 2_654_435_761 + i) & 0xFF
    return bytes(out)


def build_duplicate_tree(root: str, groups: int, files_per_group: int, file_size: int) -> None:
    os.makedirs(root, exist_ok=True)
    for g in range(groups):
        content = filler_content(g, file_size)
        for f in range(files_per_group):
            with open(os.path.join(root, f"g{g:04d}_f{f:04d}.bin"), "wb") as fh:
                fh.write(content)


def build_unique_tree(root: str, count: int, file_size: int) -> None:
    os.makedirs(root, exist_ok=True)
    for i in range(count):
        content = filler_content(i, file_size)
        with open(os.path.join(root, f"u{i:05d}.bin"), "wb") as fh:
            fh.write(content)


def build_mixed_tree(root: str) -> None:
    os.makedirs(root, exist_ok=True)
    unique_count = 1000
    for i in range(unique_count):
        content = filler_content(i, 2048)
        with open(os.path.join(root, f"u{i:05d}.bin"), "wb") as fh:
            fh.write(content)
    dup_sizes = [4096, 65_536, 200_000]
    files_per_dup_group = 6
    for g, size in enumerate(dup_sizes):
        content = filler_content(g * 97, size)
        for f in range(files_per_dup_group):
            with open(os.path.join(root, f"dup_g{g}_f{f}.bin"), "wb") as fh:
                fh.write(content)


def main() -> None:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(1)
    out = sys.argv[1]

    build_duplicate_tree(os.path.join(out, "many_small_duplicates"), groups=200, files_per_group=10, file_size=1024)
    build_unique_tree(os.path.join(out, "many_unique_small_files"), count=2000, file_size=1024)
    build_duplicate_tree(os.path.join(out, "few_large_duplicates"), groups=4, files_per_group=5, file_size=8 * 1024 * 1024)
    build_mixed_tree(os.path.join(out, "mixed_realistic_tree"))

    print(f"Generated benchmark trees under {out}")


if __name__ == "__main__":
    main()
