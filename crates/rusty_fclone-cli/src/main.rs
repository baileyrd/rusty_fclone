mod history;

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, ValueEnum};
use rusty_fclone_core::action::{self, ActionKind, ActionPlan, ApplyReport};
use rusty_fclone_core::{
    find_folder_duplicates, scan, DuplicateGroup, FileError, FolderMatch, ScanEvent, ScanOptions,
    ScanProgress, ScanSummary,
};
use serde::Serialize;

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
    /// Replace every redundant copy with a copy-on-write clone (reflink)
    /// of the kept file. Only works on CoW-capable filesystems (Btrfs,
    /// XFS with reflink, APFS, some ZFS setups) -- fails per-file,
    /// reported as a warning, wherever it isn't.
    Reflink,
}

impl Action {
    fn as_core_kind(self) -> Option<ActionKind> {
        match self {
            Action::Report => None,
            Action::Delete => Some(ActionKind::Delete),
            Action::Hardlink => Some(ActionKind::Hardlink),
            Action::Reflink => Some(ActionKind::Reflink),
        }
    }
}

/// Output format for scan results (ADR-0015, `CLI-UX-001`).
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    /// Human-readable text. Default.
    Text,
    /// One JSON object per line (NDJSON), machine-readable.
    Json,
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

    /// Path to a full-file-hash cache (created if it doesn't exist). When
    /// set, a file whose size and modified-time haven't changed since a
    /// previous scan reuses that scan's hash instead of being re-read and
    /// re-hashed (ADR-0016). Off by default -- opt in explicitly, since it
    /// means writing a file to disk.
    #[arg(long)]
    cache: Option<PathBuf>,

    /// Path to an existing `fclones --cache` database (e.g.
    /// `~/.cache/fclones` on Linux) to import full-file hashes from, so a
    /// tree fclones already scanned with `--hash-fn xxhash` doesn't need
    /// re-hashing here (ADR-0019). Independent of --cache: usable on its
    /// own for a one-off import, or with --cache so an imported hash is
    /// also persisted for future rusty-fclone-only re-scans. Off by
    /// default. Every other fclones hash function (its default `metro`,
    /// `blake3`, `sha256`, ...) computes a different digest and is never
    /// imported.
    #[arg(long)]
    import_fclones_cache: Option<PathBuf>,

    /// Path to a SQLite scan-history database (created if it doesn't
    /// exist). When set, a summary of this scan (files/bytes scanned,
    /// duplicate groups/files, and any action's result) is appended as one
    /// row after the scan completes, for longer-term analytics across
    /// repeated scans (ADR-0017). Off by default.
    #[arg(long)]
    history: Option<PathBuf>,

    /// What to do with redundant copies once a group is confirmed.
    /// Without --apply, delete/hardlink/reflink only preview what would
    /// happen.
    #[arg(long, value_enum, default_value_t = Action::Report)]
    action: Action,

    /// Actually perform --action's effect. Without this flag, delete,
    /// hardlink, and reflink only print a preview and touch nothing — a
    /// deliberate two-flag confirmation so a single typo can't cause data
    /// loss.
    #[arg(long)]
    apply: bool,

    /// Skip the interactive confirmation prompt normally shown before
    /// --apply mutates anything (ADR-0015).
    #[arg(short = 'y', long)]
    yes: bool,

    /// After the scan completes, also look for folders whose entire
    /// recursive file content duplicates -- or is a subset of -- another
    /// folder's (ADR-0021). Requires collecting every duplicate group in
    /// memory (normally streamed and discarded once printed) and a second,
    /// stat-only traversal of the root -- off by default since it's extra
    /// work most scans don't need.
    #[arg(long)]
    find_duplicate_folders: bool,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,

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

    if let Some(kind) = action_kind {
        if cli.apply && !cli.yes && !confirm_apply(&cli.root, kind) {
            eprintln!("aborted");
            return ExitCode::SUCCESS;
        }
    }

    // Captured before `cli.root` moves into `scan()` below -- needed for
    // the history record, if `--history` is set (ADR-0017), and for the
    // folder-duplicate pass, if `--find-duplicate-folders` is set
    // (ADR-0021).
    let root_display = cli.root.display().to_string();
    let started_at = unix_timestamp_now();
    let folder_dedup_root = cli.find_duplicate_folders.then(|| cli.root.clone());

    let options = ScanOptions {
        follow_symlinks: cli.follow_symlinks,
        cross_filesystems: cli.cross_filesystems,
        verify_matches: cli.verify,
        small_file_threshold: cli.small_file_threshold,
        partial_hash_sample_size: cli.partial_hash_sample_size,
        io_threads: cli.io_threads,
        cache_path: cli.cache,
        fclones_import_path: cli.import_fclones_cache,
    };
    let folder_dedup_options = folder_dedup_root.is_some().then(|| options.clone());

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
    let mut progress_line = ProgressLine::new(cli.format);
    let mut final_summary = ScanSummary::default();
    let mut collected_groups: Vec<DuplicateGroup> = Vec::new();

    for event in handle {
        match event {
            ScanEvent::DuplicateGroup(group) => {
                progress_line.finish();
                had_errors |= handle_group(
                    &group,
                    action_kind,
                    cli.apply,
                    cli.format,
                    &mut total_bytes_reclaimed,
                    &mut total_files_acted_on,
                );
                if folder_dedup_root.is_some() {
                    collected_groups.push(group);
                }
            }
            ScanEvent::Error(err) => {
                progress_line.finish();
                had_errors = true;
                report_error(cli.format, &err);
            }
            ScanEvent::Progress(progress) => {
                report_progress(cli.format, &progress, &mut progress_line);
            }
            ScanEvent::Finished(summary) => {
                progress_line.finish();
                report_finished(cli.format, &summary);
                final_summary = summary;
            }
        }
    }

    if let Some(kind) = action_kind {
        report_action_summary(
            cli.format,
            kind,
            cli.apply,
            total_bytes_reclaimed,
            total_files_acted_on,
        );
    }

    if let (Some(root), Some(options)) = (folder_dedup_root, folder_dedup_options) {
        match find_folder_duplicates(&root, &collected_groups, &options) {
            Ok(folder_matches) => report_folder_matches(cli.format, &folder_matches),
            Err(err) => {
                had_errors = true;
                eprintln!("error: {err}");
            }
        }
    }

    if let Some(history_path) = &cli.history {
        let record = history::ScanRecord {
            root: root_display,
            started_at,
            files_scanned: final_summary.files_scanned,
            bytes_scanned: final_summary.bytes_scanned,
            duplicate_groups: final_summary.duplicate_groups,
            duplicate_files: final_summary.duplicate_files,
            action_kind: action_kind.map(action_word),
            action_applied: action_kind.map(|_| cli.apply),
            bytes_reclaimed: action_kind.map(|_| total_bytes_reclaimed),
            files_acted_on: action_kind.map(|_| total_files_acted_on),
        };
        if let Err(err) = history::record_scan(history_path, &record) {
            eprintln!("warning: failed to record scan history: {err}");
        }
    }

    if had_errors {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Current time as Unix seconds, for the history record's `started_at`
/// (ADR-0017). Falls back to `0` on a pre-1970 system clock, which never
/// happens in practice but keeps this infallible.
fn unix_timestamp_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Prompts on stderr and reads a yes/no answer from stdin, describing what
/// `--apply` is about to do. Exact totals aren't known upfront -- the scan
/// hasn't run yet, and groups/actions are applied incrementally as they're
/// found (ADR-0004's streaming design) -- so this is a general warning
/// naming the root and action, not a precise preview (ADR-0015).
fn confirm_apply(root: &Path, kind: ActionKind) -> bool {
    eprint!(
        "This will scan {} and {} redundant files as duplicates are found. Proceed? [y/N] ",
        root.display(),
        action_word(kind)
    );
    let _ = io::stderr().flush();
    confirm(io::stdin().lock())
}

/// The confirmation prompt's actual yes/no decision, factored out from
/// `confirm_apply` so it's testable without a real stdin/terminal.
fn confirm(mut reader: impl BufRead) -> bool {
    let mut input = String::new();
    if reader.read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Tracks the live-updating "scanning..." line on stderr (Text format,
/// real terminal only -- ADR-0015). Each update overwrites the previous
/// line in place via `\r`, padded to erase any leftover characters from a
/// longer prior line; `finish` clears it before other output is printed,
/// so a duplicate group or error never collides with it mid-line.
struct ProgressLine {
    active: bool,
    last_len: usize,
}

impl ProgressLine {
    fn new(format: Format) -> Self {
        Self {
            active: format == Format::Text && io::stderr().is_terminal(),
            last_len: 0,
        }
    }

    fn update(&mut self, progress: &ScanProgress) {
        if !self.active {
            return;
        }
        let line = format!(
            "scanning... {} files, {} bytes",
            progress.files_scanned, progress.bytes_scanned
        );
        let pad = " ".repeat(self.last_len.saturating_sub(line.len()));
        eprint!("\r{line}{pad}");
        let _ = io::stderr().flush();
        self.last_len = line.len();
    }

    fn finish(&mut self) {
        if !self.active || self.last_len == 0 {
            return;
        }
        eprint!("\r{}\r", " ".repeat(self.last_len));
        let _ = io::stderr().flush();
        self.last_len = 0;
    }
}

fn report_progress(format: Format, progress: &ScanProgress, progress_line: &mut ProgressLine) {
    match format {
        Format::Text => progress_line.update(progress),
        Format::Json => print_json(&JsonEvent::Progress {
            files_scanned: progress.files_scanned,
            bytes_scanned: progress.bytes_scanned,
        }),
    }
}

fn report_error(format: Format, err: &FileError) {
    match format {
        Format::Text => eprintln!("warning: {err}"),
        Format::Json => print_json(&JsonEvent::Error {
            path: err.path.display().to_string(),
            message: err.source.to_string(),
        }),
    }
}

fn report_finished(format: Format, summary: &ScanSummary) {
    match format {
        Format::Text => eprintln!(
            "scanned {} files ({} bytes), found {} duplicate groups ({} files)",
            summary.files_scanned,
            summary.bytes_scanned,
            summary.duplicate_groups,
            summary.duplicate_files
        ),
        Format::Json => print_json(&JsonEvent::Finished {
            files_scanned: summary.files_scanned,
            bytes_scanned: summary.bytes_scanned,
            duplicate_groups: summary.duplicate_groups,
            duplicate_files: summary.duplicate_files,
        }),
    }
}

fn report_action_summary(
    format: Format,
    kind: ActionKind,
    applied: bool,
    bytes_reclaimed: u64,
    files: u64,
) {
    match format {
        Format::Text if applied => eprintln!(
            "{}: reclaimed {bytes_reclaimed} bytes across {files} files",
            action_word(kind)
        ),
        Format::Text => eprintln!(
            "dry run ({}): would reclaim {bytes_reclaimed} bytes across {files} files -- pass --apply to actually do this",
            action_word(kind)
        ),
        Format::Json => print_json(&JsonEvent::ActionSummary {
            kind: action_word(kind),
            applied,
            bytes_reclaimed,
            files,
        }),
    }
}

/// Handles one duplicate group: prints it (and, if an action was
/// requested, its plan and, with --apply, the result of actually running
/// it) in the requested format. Returns whether any per-file error
/// occurred.
fn handle_group(
    group: &DuplicateGroup,
    action_kind: Option<ActionKind>,
    apply: bool,
    format: Format,
    total_bytes_reclaimed: &mut u64,
    total_files_acted_on: &mut u64,
) -> bool {
    let Some(kind) = action_kind else {
        print_group(format, group, None);
        return false;
    };

    let plan = action::plan(group, kind);
    if plan.actions.is_empty() {
        print_group(format, group, None);
        return false;
    }

    let report = if apply {
        Some(action::apply(&plan))
    } else {
        None
    };

    let (bytes_reclaimed, files_acted_on, had_errors) = match &report {
        Some(report) => (
            report.bytes_reclaimed,
            report.succeeded.len() as u64,
            !report.failed.is_empty(),
        ),
        None => (plan.bytes_reclaimed, plan.actions.len() as u64, false),
    };
    *total_bytes_reclaimed += bytes_reclaimed;
    *total_files_acted_on += files_acted_on;

    print_group(format, group, Some((kind, &plan, apply, report.as_ref())));

    if let Some(report) = &report {
        for err in &report.failed {
            eprintln!("warning: {err}");
        }
    }

    had_errors
}

fn print_group(
    format: Format,
    group: &DuplicateGroup,
    action: Option<(ActionKind, &ActionPlan, bool, Option<&ApplyReport>)>,
) {
    match format {
        Format::Text => print_group_text(group, action),
        Format::Json => print_group_json(group, action),
    }
}

fn print_group_text(
    group: &DuplicateGroup,
    action: Option<(ActionKind, &ActionPlan, bool, Option<&ApplyReport>)>,
) {
    println!("--- {} bytes, {} copies ---", group.size, group.paths.len());
    for path in &group.paths {
        println!("{}", path.display());
    }
    let Some((kind, plan, ..)) = action else {
        return;
    };
    println!("  keep: {}", plan.kept.display());
    for file_action in &plan.actions {
        println!("  {}: {}", action_word(kind), file_action.path.display());
    }
}

fn print_group_json(
    group: &DuplicateGroup,
    action: Option<(ActionKind, &ActionPlan, bool, Option<&ApplyReport>)>,
) {
    let action = action.map(|(kind, plan, applied, report)| {
        let (succeeded, failed) = match report {
            Some(report) => (
                paths_to_strings(&report.succeeded),
                report
                    .failed
                    .iter()
                    .map(|e| e.path.display().to_string())
                    .collect(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        JsonAction {
            kind: action_word(kind),
            kept: plan.kept.display().to_string(),
            applied,
            planned: plan
                .actions
                .iter()
                .map(|a| a.path.display().to_string())
                .collect(),
            succeeded,
            failed,
            bytes_reclaimed: report.map_or(plan.bytes_reclaimed, |r| r.bytes_reclaimed),
        }
    });
    print_json(&JsonEvent::DuplicateGroup {
        size: group.size,
        paths: arc_paths_to_strings(&group.paths),
        action,
    });
}

fn report_folder_matches(format: Format, matches: &[FolderMatch]) {
    for m in matches {
        match format {
            Format::Text => print_folder_match_text(m),
            Format::Json => print_folder_match_json(m),
        }
    }
}

fn print_folder_match_text(m: &FolderMatch) {
    match m {
        FolderMatch::Exact {
            folders,
            file_count,
            bytes,
        } => {
            println!("--- duplicate folders: {bytes} bytes, {file_count} files ---");
            for f in folders {
                println!("{}", f.display());
            }
        }
        FolderMatch::Contained {
            subset,
            superset,
            file_count,
            bytes,
        } => {
            println!("--- folder contained in another: {bytes} bytes, {file_count} files ---");
            println!("  subset:   {}", subset.display());
            println!("  superset: {}", superset.display());
        }
    }
}

fn print_folder_match_json(m: &FolderMatch) {
    match m {
        FolderMatch::Exact {
            folders,
            file_count,
            bytes,
        } => print_json(&JsonEvent::FolderExact {
            folders: paths_to_strings(folders),
            file_count: *file_count,
            bytes: *bytes,
        }),
        FolderMatch::Contained {
            subset,
            superset,
            file_count,
            bytes,
        } => print_json(&JsonEvent::FolderContained {
            subset: subset.display().to_string(),
            superset: superset.display().to_string(),
            file_count: *file_count,
            bytes: *bytes,
        }),
    }
}

fn paths_to_strings(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

fn arc_paths_to_strings(paths: &[Arc<Path>]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

fn print_json(event: &JsonEvent) {
    println!(
        "{}",
        serde_json::to_string(event).expect("JsonEvent always serializes")
    );
}

/// NDJSON event shape for `--format json` (ADR-0015, `CLI-UX-001`).
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum JsonEvent {
    DuplicateGroup {
        size: u64,
        paths: Vec<String>,
        action: Option<JsonAction>,
    },
    Error {
        path: String,
        message: String,
    },
    Progress {
        files_scanned: u64,
        bytes_scanned: u64,
    },
    Finished {
        files_scanned: u64,
        bytes_scanned: u64,
        duplicate_groups: u64,
        duplicate_files: u64,
    },
    ActionSummary {
        kind: &'static str,
        applied: bool,
        bytes_reclaimed: u64,
        files: u64,
    },
    FolderExact {
        folders: Vec<String>,
        file_count: u64,
        bytes: u64,
    },
    FolderContained {
        subset: String,
        superset: String,
        file_count: u64,
        bytes: u64,
    },
}

#[derive(Serialize)]
struct JsonAction {
    kind: &'static str,
    kept: String,
    applied: bool,
    planned: Vec<String>,
    succeeded: Vec<String>,
    failed: Vec<String>,
    bytes_reclaimed: u64,
}

fn action_word(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Delete => "delete",
        ActionKind::Hardlink => "hardlink",
        ActionKind::Reflink => "reflink",
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
            cache: ScanOptions::default().cache_path,
            import_fclones_cache: ScanOptions::default().fclones_import_path,
            history: None,
            action: Action::Report,
            apply: false,
            yes: false,
            find_duplicate_folders: false,
            format: Format::Text,
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
    /// perform the action. `--yes` bypasses the confirmation prompt so
    /// this doesn't block on real stdin (FR-009 covers the prompt itself).
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
        cli.yes = true;
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
        cli.yes = true;
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

    /// FR-009: `--apply` without `--yes` is gated on the confirmation
    /// prompt; since tests don't have a real interactive stdin answering
    /// "y", the prompt is declined (empty/EOF input) and no mutation
    /// happens. See `confirm`'s own tests for the prompt's decision logic
    /// in isolation.
    #[test]
    fn apply_without_yes_is_blocked_by_the_unanswered_confirmation_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Delete;
        cli.apply = true;
        cli.yes = false;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS, "declining is not a failure");
        assert!(a.exists());
        assert!(b.exists(), "nothing must be mutated without confirmation");
    }

    #[test]
    fn confirm_accepts_y_and_yes_case_insensitively() {
        assert!(confirm(io::Cursor::new(b"y\n" as &[u8])));
        assert!(confirm(io::Cursor::new(b"Y\n" as &[u8])));
        assert!(confirm(io::Cursor::new(b"yes\n" as &[u8])));
        assert!(confirm(io::Cursor::new(b"YES\n" as &[u8])));
    }

    #[test]
    fn confirm_rejects_anything_else() {
        assert!(!confirm(io::Cursor::new(b"n\n" as &[u8])));
        assert!(!confirm(io::Cursor::new(b"\n" as &[u8])));
        assert!(!confirm(io::Cursor::new(b"" as &[u8])));
        assert!(!confirm(io::Cursor::new(b"maybe\n" as &[u8])));
    }

    #[test]
    fn rejects_nonexistent_root() {
        let exit = run(base_cli(PathBuf::from("/does/not/exist/at/all")));
        assert_eq!(exit, ExitCode::FAILURE);
    }

    #[test]
    fn json_format_reports_duplicates_as_ndjson() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.format = Format::Json;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
    }

    /// ADR-0021, FR-010: `--find-duplicate-folders` runs the folder-level
    /// pass after the scan and doesn't error on a tree containing an exact
    /// folder duplicate.
    #[test]
    fn find_duplicate_folders_flag_succeeds_on_an_exact_folder_match() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("1.txt"), b"dup").unwrap();
        fs::write(b.join("1.txt"), b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.find_duplicate_folders = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
    }

    /// Same as above, but for `--format json` -- exercises the
    /// `FolderExact`/`FolderContained` JSON event variants.
    #[test]
    fn find_duplicate_folders_flag_succeeds_with_json_format() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("extra.txt"), b"only in big").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.find_duplicate_folders = true;
        cli.format = Format::Json;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
    }

    #[test]
    fn history_flag_records_one_row_per_scan() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"dup").unwrap();
        fs::write(dir.path().join("b.txt"), b"dup").unwrap();
        let history_dir = tempfile::tempdir().unwrap();
        let history_path = history_dir.path().join("history.sqlite");

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.history = Some(history_path.clone());
        let exit = run(cli);
        assert_eq!(exit, ExitCode::SUCCESS);

        let conn = rusqlite::Connection::open(&history_path).unwrap();
        let (files_scanned, duplicate_groups, action_kind): (i64, i64, Option<String>) = conn
            .query_row(
                "SELECT files_scanned, duplicate_groups, action_kind FROM scans",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(files_scanned, 2);
        assert_eq!(duplicate_groups, 1);
        assert_eq!(
            action_kind, None,
            "no action was requested (Action::Report)"
        );
    }

    #[test]
    fn history_flag_records_the_action_result_when_an_action_runs() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"dup").unwrap();
        fs::write(dir.path().join("b.txt"), b"dup").unwrap();
        let history_dir = tempfile::tempdir().unwrap();
        let history_path = history_dir.path().join("history.sqlite");

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.history = Some(history_path.clone());
        cli.action = Action::Delete;
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);
        assert_eq!(exit, ExitCode::SUCCESS);

        let conn = rusqlite::Connection::open(&history_path).unwrap();
        let (action_kind, applied, reclaimed): (Option<String>, Option<bool>, Option<i64>) = conn
            .query_row(
                "SELECT action_kind, action_applied, bytes_reclaimed FROM scans",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(action_kind.as_deref(), Some("delete"));
        assert_eq!(applied, Some(true));
        assert_eq!(reclaimed, Some(3));
    }
}
