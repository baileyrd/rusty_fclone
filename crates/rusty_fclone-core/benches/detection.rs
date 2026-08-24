//! End-to-end throughput benchmarks for the detection pipeline
//! (`FCLONE-DETECTION-001-DETECTION-BENCHMARK` on the roadmap).
//!
//! Each scenario builds a synthetic directory tree once, then repeatedly
//! re-scans that same (unmodified — scanning is read-only) tree, reporting
//! files/sec or bytes/sec via Criterion's `Throughput`. These are relative/
//! regression benchmarks, not an absolute claim against any other tool —
//! see `docs/roadmap/ROADMAP.md` for the still-open "compare against
//! fclones" follow-up.
//!
//! Run with `cargo bench -p rusty_fclone-core`.

use std::fs;
use std::path::Path;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use rusty_fclone_core::{scan, ScanEvent, ScanOptions, ScanSummary};
use tempfile::TempDir;

/// Deterministic, non-degenerate filler so xxh3 isn't hashing an all-zero
/// (or otherwise trivially compressible) buffer.
fn filler_content(seed: usize, size: usize) -> Vec<u8> {
    let mut content = vec![0u8; size];
    for (i, byte) in content.iter_mut().enumerate() {
        *byte = seed.wrapping_mul(2_654_435_761).wrapping_add(i) as u8;
    }
    content
}

/// `groups` distinct content groups, each with `files_per_group` byte-
/// identical files of `file_size` bytes.
fn build_duplicate_tree(groups: usize, files_per_group: usize, file_size: usize) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for g in 0..groups {
        let content = filler_content(g, file_size);
        for f in 0..files_per_group {
            fs::write(dir.path().join(format!("g{g:04}_f{f:04}.bin")), &content).unwrap();
        }
    }
    dir
}

/// `count` files, every one with unique content — the worst case for the
/// pruning stages, since no hash match ever eliminates a candidate early.
fn build_unique_tree(count: usize, file_size: usize) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..count {
        let content = filler_content(i, file_size);
        fs::write(dir.path().join(format!("u{i:05}.bin")), &content).unwrap();
    }
    dir
}

/// A mostly-unique tree with a handful of duplicate groups mixed in —
/// closer to a real filesystem than either all-duplicate or all-unique
/// extremes above.
fn build_mixed_tree() -> (TempDir, usize) {
    let dir = tempfile::tempdir().unwrap();
    let unique_count = 1000;
    for i in 0..unique_count {
        let content = filler_content(i, 2048);
        fs::write(dir.path().join(format!("u{i:05}.bin")), &content).unwrap();
    }
    let dup_sizes = [4096usize, 65_536, 200_000];
    let files_per_dup_group = 6;
    for (g, size) in dup_sizes.iter().enumerate() {
        let content = filler_content(g * 97, *size);
        for f in 0..files_per_dup_group {
            fs::write(dir.path().join(format!("dup_g{g}_f{f}.bin")), &content).unwrap();
        }
    }
    let total_files = unique_count + dup_sizes.len() * files_per_dup_group;
    (dir, total_files)
}

fn run_scan(root: &Path) -> ScanSummary {
    let handle = scan(root, ScanOptions::default()).unwrap();
    let mut summary = None;
    for event in handle {
        if let ScanEvent::Finished(s) = event {
            summary = Some(s);
        }
    }
    summary.expect("scan always sends Finished as its last event")
}

fn bench_many_small_duplicates(c: &mut Criterion) {
    let groups = 200;
    let files_per_group = 10;
    let file_size = 1024;
    let dir = build_duplicate_tree(groups, files_per_group, file_size);

    let mut group = c.benchmark_group("many_small_duplicates");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);
    group.throughput(Throughput::Elements((groups * files_per_group) as u64));
    group.bench_function("scan", |b| b.iter(|| run_scan(dir.path())));
    group.finish();
}

fn bench_many_unique_small_files(c: &mut Criterion) {
    let count = 2000;
    let file_size = 1024;
    let dir = build_unique_tree(count, file_size);

    let mut group = c.benchmark_group("many_unique_small_files");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);
    group.throughput(Throughput::Elements(count as u64));
    group.bench_function("scan", |b| b.iter(|| run_scan(dir.path())));
    group.finish();
}

fn bench_few_large_duplicates(c: &mut Criterion) {
    let groups = 4;
    let files_per_group = 5;
    let file_size = 8 * 1024 * 1024; // 8 MiB
    let dir = build_duplicate_tree(groups, files_per_group, file_size);
    let total_bytes = (groups * files_per_group * file_size) as u64;

    let mut group = c.benchmark_group("few_large_duplicates");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(10);
    group.throughput(Throughput::Bytes(total_bytes));
    group.bench_function("scan", |b| b.iter(|| run_scan(dir.path())));
    group.finish();
}

fn bench_mixed_realistic_tree(c: &mut Criterion) {
    let (dir, total_files) = build_mixed_tree();

    let mut group = c.benchmark_group("mixed_realistic_tree");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);
    group.throughput(Throughput::Elements(total_files as u64));
    group.bench_function("scan", |b| b.iter(|| run_scan(dir.path())));
    group.finish();
}

criterion_group!(
    benches,
    bench_many_small_duplicates,
    bench_many_unique_small_files,
    bench_few_large_duplicates,
    bench_mixed_realistic_tree
);
criterion_main!(benches);
