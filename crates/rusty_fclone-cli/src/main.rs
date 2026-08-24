use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use rusty_fclone_core::action::{self, ActionKind};
use rusty_fclone_core::{scan, DuplicateGroup, ScanEvent, ScanOptions};

/// What to do with redundant copies once a duplicate group is confirmed.
/// Mirrors `rusty_fclone_core::action::ActionKind`, plus `Report` (the
/// default: print groups, take no action) which has no core-side
/// equivalent since the core crate stays CLI-agnostic (ADR-0005).
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Action {
    /// Report duplicate groups; don't touch the filesystem. Default.
    Report,
    /// Delete every redundant copy, keeping one file per group.
    Delete,
    /// Replace every redundant copy with a hardlink to the kept file.
    Hardlink,
}

impl Action {
    fn as_core_kind(self) -> Option<ActionKind> {
        match self {
            Action::Report => None,
            Action::Delete => Some(ActionKind::Delete),
            Action::Hardlink => Some(ActionKind::Hardlink),
        }
    }
}

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

    /// Number of worker threads in the I/O-bound read pool. If omitted,
    /// auto-detected from the scan root's filesystem: oversubscribed on a
    /// rotational disk (Linux only, best-effort), core count otherwise.
    #[arg(long)]
    io_threads: Option<usize>,

    /// What to do with redundant copies once a group is confirmed.
    /// Without --apply, delete/hardlink only preview what would happen.
    #[arg(long, value_enum, default_value_t = Action::Report)]
    action: Action,

    /// Actually perform --action's effect. Without this flag, delete and
    /// hardlink only print a preview and touch nothing — a deliberate
    /// two-flag confirmation so a single typo can't cause data loss.
    #[arg(long)]
    apply: bool,

    /// Increase log verbosity (-v info, -vv debug, -vvv trace). Ignored if
    /// RUST_LOG is set, which always takes precedence (ADR-0010).
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    run(cli)
}

/// Sets up the `tracing-subscriber` output on stderr (so it never mixes
/// with the plain-text results on stdout). `RUST_LOG` always wins when
/// set; otherwise verbosity is driven by repeated `-v` flags (ADR-0010).
fn init_tracing(verbose: u8) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let level = match verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        };
        EnvFilter::new(format!(
            "rusty_fclone_core={level},rusty_fclone_cli={level}"
        ))
    });

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

/// The CLI's actual logic, separated from `main` so tests can construct a
/// `Cli` directly (bypassing real process argv) and assert on filesystem
/// side effects and the exit code.
fn run(cli: Cli) -> ExitCode {
    let action_kind = cli.action.as_core_kind();

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
    let mut total_bytes_reclaimed = 0u64;
    let mut total_files_acted_on = 0u64;

    for event in handle {
        match event {
            ScanEvent::DuplicateGroup(group) => {
                had_errors |= handle_group(
                    &group,
                    action_kind,
                    cli.apply,
                    &mut total_bytes_reclaimed,
                    &mut total_files_acted_on,
                );
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

    match action_kind {
        None => {}
        Some(kind) if cli.apply => {
            eprintln!(
                "{}: reclaimed {} bytes across {} files",
                action_word(kind),
                total_bytes_reclaimed,
                total_files_acted_on
            );
        }
        Some(kind) => {
            eprintln!(
                "dry run ({}): would reclaim {} bytes across {} files -- pass --apply to actually do this",
                action_word(kind),
                total_bytes_reclaimed,
                total_files_acted_on
            );
        }
    }

    if had_errors {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Prints one duplicate group and, if an action was requested, its plan
/// (and, with --apply, the result of actually running it). Returns whether
/// any per-file error occurred.
fn handle_group(
    group: &DuplicateGroup,
    action_kind: Option<ActionKind>,
    apply: bool,
    total_bytes_reclaimed: &mut u64,
    total_files_acted_on: &mut u64,
) -> bool {
    println!("--- {} bytes, {} copies ---", group.size, group.paths.len());
    for path in &group.paths {
        println!("{}", path.display());
    }

    let Some(kind) = action_kind else {
        return false;
    };

    let plan = action::plan(group, kind);
    if plan.actions.is_empty() {
        return false;
    }

    println!("  keep: {}", plan.kept.display());
    for file_action in &plan.actions {
        println!("  {}: {}", action_word(kind), file_action.path.display());
    }

    if !apply {
        *total_bytes_reclaimed += plan.bytes_reclaimed;
        *total_files_acted_on += plan.actions.len() as u64;
        return false;
    }

    let report = action::apply(&plan);
    *total_bytes_reclaimed += report.bytes_reclaimed;
    *total_files_acted_on += report.succeeded.len() as u64;

    let mut had_errors = false;
    for err in &report.failed {
        had_errors = true;
        eprintln!("warning: {err}");
    }
    had_errors
}

fn action_word(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Delete => "delete",
        ActionKind::Hardlink => "hardlink",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn base_cli(root: PathBuf) -> Cli {
        Cli {
            root,
            follow_symlinks: false,
            cross_filesystems: false,
            verify: false,
            small_file_threshold: ScanOptions::default().small_file_threshold,
            partial_hash_sample_size: ScanOptions::default().partial_hash_sample_size,
            io_threads: ScanOptions::default().io_threads,
            action: Action::Report,
            apply: false,
            verbose: 0,
        }
    }

    /// FR-007: default (`--action` omitted, i.e. `Report`) never enters the
    /// action code path at all, so it can't mutate the filesystem.
    #[test]
    fn default_report_action_leaves_files_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let exit = run(base_cli(dir.path().to_path_buf()));

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(a.exists());
        assert!(b.exists());
    }

    /// FR-006: `--action delete` without `--apply` must not touch the
    /// filesystem, only preview.
    #[test]
    fn action_without_apply_is_a_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Delete;
        cli.apply = false;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(a.exists(), "dry run must not delete the kept file");
        assert!(b.exists(), "dry run must not delete the redundant copy");
    }

    /// FR-006 (the other half): `--action delete --apply` must actually
    /// perform the action.
    #[test]
    fn action_with_apply_actually_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Delete;
        cli.apply = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(a.exists(), "the kept file must survive");
        assert!(!b.exists(), "the redundant copy must be gone");
    }

    #[test]
    fn action_with_apply_actually_hardlinks() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Hardlink;
        cli.apply = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(a.exists());
        assert!(b.exists(), "hardlink keeps the path, just repoints it");
        assert_eq!(
            rusty_fclone_core::action::plan(
                &DuplicateGroup {
                    size: 3,
                    paths: vec![a.clone().into(), b.clone().into()]
                },
                ActionKind::Delete
            )
            .actions
            .len(),
            0,
            "a and b must now share an inode -- nothing left to act on"
        );
    }

    #[test]
    fn rejects_nonexistent_root() {
        let exit = run(base_cli(PathBuf::from("/does/not/exist/at/all")));
        assert_eq!(exit, ExitCode::FAILURE);
    }
}
