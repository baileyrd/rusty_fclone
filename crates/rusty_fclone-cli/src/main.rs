use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use rusty_fclone_core::{scan, ScanEvent, ScanOptions};

/// Find duplicate files, fast.
#[derive(Parser)]
#[command(name = "rusty-fclone", version, about)]
struct Cli {
    /// Directory to scan for duplicates.
    root: PathBuf,

    /// Follow symbolic links during traversal.
    #[arg(long)]
    follow_symlinks: bool,

    /// Cross filesystem/mount-point boundaries during traversal.
    #[arg(long)]
    cross_filesystems: bool,

    /// Byte-compare hash-matched files before reporting them as duplicates.
    #[arg(long)]
    verify: bool,

    /// Files at or below this size (bytes) skip the partial-hash stage.
    #[arg(long, default_value_t = ScanOptions::default().small_file_threshold)]
    small_file_threshold: u64,

    /// Bytes sampled at the head, middle, and tail of a file during the
    /// partial-hash stage (for files larger than --small-file-threshold).
    #[arg(long, default_value_t = ScanOptions::default().partial_hash_sample_size)]
    partial_hash_sample_size: u64,

    /// Number of worker threads in the I/O-bound read pool.
    #[arg(long, default_value_t = ScanOptions::default().io_threads)]
    io_threads: usize,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let options = ScanOptions {
        follow_symlinks: cli.follow_symlinks,
        cross_filesystems: cli.cross_filesystems,
        verify_matches: cli.verify,
        small_file_threshold: cli.small_file_threshold,
        partial_hash_sample_size: cli.partial_hash_sample_size,
        io_threads: cli.io_threads,
    };

    let handle = match scan(cli.root, options) {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut had_errors = false;
    for event in handle {
        match event {
            ScanEvent::DuplicateGroup(group) => {
                println!("--- {} bytes, {} copies ---", group.size, group.paths.len());
                for path in &group.paths {
                    println!("{}", path.display());
                }
            }
            ScanEvent::Error(err) => {
                had_errors = true;
                eprintln!("warning: {err}");
            }
            ScanEvent::Finished(summary) => {
                eprintln!(
                    "scanned {} files ({} bytes), found {} duplicate groups ({} files)",
                    summary.files_scanned,
                    summary.bytes_scanned,
                    summary.duplicate_groups,
                    summary.duplicate_files
                );
            }
        }
    }

    if had_errors {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
