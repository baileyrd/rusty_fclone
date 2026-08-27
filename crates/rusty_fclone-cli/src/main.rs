mod history;

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, ValueEnum};
use rusty_fclone_core::action::{self, ActionKind, ActionPlan, ApplyReport};
use rusty_fclone_core::folder_action;
use rusty_fclone_core::select::{self, Rule as SelectRule};
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
    /// Delete every redundant copy permanently, keeping one file per group.
    /// No recovery path once this succeeds -- prefer `trash` unless a
    /// permanent, unrecoverable delete is specifically wanted.
    Delete,
    /// Move every redundant copy to the operating system's trash/recycle
    /// bin instead of deleting it outright, keeping one file per group.
    /// Recoverable through the OS's own trash UI.
    Trash,
    /// Replace every redundant copy with a hardlink to the kept file.
    Hardlink,
    /// Replace every redundant copy with a copy-on-write clone (reflink)
    /// of the kept file. Only works on CoW-capable filesystems (Btrfs,
    /// XFS with reflink, APFS, some ZFS setups) -- fails per-file,
    /// reported as a warning, wherever it isn't.
    Reflink,
    /// Relocate every redundant copy into `--archive-dir`, mirroring its
    /// original path underneath it. The redundant copy is gone from its
    /// original location afterward (like `delete`/`trash`) but survives at
    /// its new archived path. Requires `--archive-dir`.
    Move,
    /// Copy every redundant copy into `--archive-dir` (same path-mirroring
    /// scheme as `move`), leaving the original untouched. Reclaims no
    /// space -- a consolidate-for-review step, not a cleanup one. Requires
    /// `--archive-dir`.
    Copy,
}

impl Action {
    /// `archive_dir` is required for `move`/`copy` and ignored otherwise --
    /// enforced here rather than via clap's own required-if machinery, so
    /// the error names the specific action that needs it (`ACTION-MOVE-COPY`).
    fn as_core_kind(self, archive_dir: Option<&Path>) -> Result<Option<ActionKind>, String> {
        match self {
            Action::Report => Ok(None),
            Action::Delete => Ok(Some(ActionKind::Delete)),
            Action::Trash => Ok(Some(ActionKind::Trash)),
            Action::Hardlink => Ok(Some(ActionKind::Hardlink)),
            Action::Reflink => Ok(Some(ActionKind::Reflink)),
            Action::Move => archive_dir
                .map(|dir| Some(ActionKind::Move(dir.to_path_buf())))
                .ok_or_else(|| "--action move requires --archive-dir <PATH>".to_string()),
            Action::Copy => archive_dir
                .map(|dir| Some(ActionKind::Copy(dir.to_path_buf())))
                .ok_or_else(|| "--action copy requires --archive-dir <PATH>".to_string()),
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

/// Which copy to keep in a group, applied across every group in one pass
/// instead of the previous per-group-only alphabetically-first default.
/// Mirrors `rusty_fclone_core::select::Rule` (`SELECTION-RULES`).
#[derive(Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
enum KeepRule {
    /// Keep the alphabetically-first path. Default -- matches this
    /// project's behavior before `--keep-rule` existed.
    #[default]
    Alphabetical,
    /// Keep the most recently modified copy.
    Newest,
    /// Keep the least recently modified copy.
    Oldest,
    /// Keep the copy at the shallowest path.
    ShortestPath,
    /// Keep the copy at the deepest path.
    LongestPath,
}

impl KeepRule {
    fn as_core_rule(self) -> SelectRule {
        match self {
            KeepRule::Alphabetical => SelectRule::AlphabeticallyFirst,
            KeepRule::Newest => SelectRule::Newest,
            KeepRule::Oldest => SelectRule::Oldest,
            KeepRule::ShortestPath => SelectRule::ShortestPath,
            KeepRule::LongestPath => SelectRule::LongestPath,
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

    /// Skip files smaller than this size (bytes). Applied during traversal,
    /// before any hashing (`DETECTION-SCAN-FILTERS`).
    #[arg(long)]
    min_size: Option<u64>,

    /// Skip files larger than this size (bytes). Applied during traversal,
    /// before any hashing (`DETECTION-SCAN-FILTERS`).
    #[arg(long)]
    max_size: Option<u64>,

    /// Only scan files with this extension (case-insensitive, without the
    /// leading `.`). Repeatable. A file with no extension is skipped if
    /// this is set (`DETECTION-SCAN-FILTERS`).
    #[arg(long = "include-ext")]
    include_extensions: Vec<String>,

    /// Skip files with this extension (case-insensitive, without the
    /// leading `.`), even if `--include-ext` would otherwise allow them.
    /// Repeatable (`DETECTION-SCAN-FILTERS`).
    #[arg(long = "exclude-ext")]
    exclude_extensions: Vec<String>,

    /// Skip this path and everything beneath it entirely -- not just from
    /// the results, but from traversal itself. Repeatable. Matched as a
    /// literal path prefix against the path as traversed; pass it in the
    /// same form (relative/absolute) as `root` for reliable matching
    /// (`DETECTION-SCAN-FILTERS`).
    #[arg(long = "exclude-path")]
    exclude_paths: Vec<PathBuf>,

    /// What to do with redundant copies once a group is confirmed. Without
    /// --apply, delete/trash/hardlink/reflink/move/copy only preview what
    /// would happen. move/copy also require --archive-dir.
    #[arg(long, value_enum, default_value_t = Action::Report)]
    action: Action,

    /// Which copy to keep in each group when --action is set. Applied
    /// across every group in one pass -- no effect in the default Report
    /// mode, which doesn't designate a kept file at all.
    #[arg(long, value_enum, default_value_t = KeepRule::Alphabetical)]
    keep_rule: KeepRule,

    /// Mark this path (a file or a directory subtree) as protected --
    /// never selected as a redundant copy to act on. Repeatable. Overrides
    /// --keep-rule when a group contains a protected path: that path is
    /// always kept, and every other protected copy is excluded from the
    /// action too, even if it isn't the one kept. A hard, fails-closed
    /// guardrail, not a suggestion -- it can't be bypassed by --keep-rule
    /// or by which path a group happens to sort first
    /// (`ACTION-REFERENCE-FOLDERS`). No effect in the default Report mode.
    #[arg(long = "reference")]
    reference_paths: Vec<PathBuf>,

    /// Destination folder for `--action move`/`--action copy`. Every
    /// redundant copy is relocated (`move`) or duplicated (`copy`)
    /// underneath this directory, mirroring its original path so files
    /// with the same name from different directories never collide.
    /// Required by, and only meaningful with, `--action move`/`copy`
    /// (`ACTION-MOVE-COPY`).
    #[arg(long)]
    archive_dir: Option<PathBuf>,

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

/// Queries a `--history` database written by earlier scans
/// (`CLI-HISTORY-AUDIT`). A separate top-level command, reached as
/// `rusty-fclone history <SUBCOMMAND>` -- `history` is reserved as a
/// subcommand keyword rather than folded into `Cli` as
/// `#[command(subcommand)]`, since `Cli`'s own `root` is a required
/// positional argument and clap can't disambiguate "a subcommand name"
/// from "a directory named the same as a subcommand" at the same
/// argument position. `main` pre-dispatches on `args[1] == "history"`
/// before `Cli::parse` ever runs, so every existing `rusty-fclone <ROOT>
/// ...` invocation is completely unaffected (ADR-0027).
#[derive(Parser)]
#[command(name = "rusty-fclone history")]
struct HistoryCli {
    /// Path to the `--history` database to query.
    #[arg(long)]
    db: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,

    #[command(subcommand)]
    command: HistoryCommand,
}

#[derive(clap::Subcommand)]
enum HistoryCommand {
    /// List the most recent scans, newest first.
    List {
        /// Maximum number of scans to list.
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Aggregate totals (scans, bytes reclaimed, files acted on, ...)
    /// across every scan in an optional date range.
    Stats {
        /// Only include scans started at or after this Unix timestamp
        /// (seconds).
        #[arg(long)]
        since: Option<i64>,
        /// Only include scans started at or before this Unix timestamp
        /// (seconds).
        #[arg(long)]
        until: Option<i64>,
    },
}

fn main() -> ExitCode {
    // `args[0]` is the program name; `args[1]`, if present, is either
    // "history" (routed to `HistoryCli`) or the start of a normal scan
    // invocation (routed to `Cli` exactly as before). `parse_from` only
    // reads `argv[0]` for its own program-name display, so passing
    // "history" there (rather than the real program name) is harmless --
    // `--help`/error text under `rusty-fclone history ...` just reads
    // "rusty-fclone history" instead, which is arguably more correct
    // anyway (see `HistoryCli`'s `#[command(name = ...)]`).
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("history") {
        let cli = HistoryCli::parse_from(&args[1..]);
        return run_history(cli);
    }

    let cli = Cli::parse();
    init_tracing(cli.verbose);
    run(cli)
}

/// Runs `rusty-fclone history <SUBCOMMAND>` -- read-only queries against a
/// `--history` database, no scanning involved (`CLI-HISTORY-AUDIT`).
fn run_history(cli: HistoryCli) -> ExitCode {
    match cli.command {
        HistoryCommand::List { limit } => match history::list_scans(&cli.db, limit) {
            Ok(rows) => {
                for row in rows {
                    match cli.format {
                        Format::Text => println!(
                            "{} @ {}  files={} bytes={} dup_groups={} dup_files={}  action={} applied={} reclaimed={} acted_on={}",
                            row.root,
                            row.started_at,
                            row.files_scanned,
                            row.bytes_scanned,
                            row.duplicate_groups,
                            row.duplicate_files,
                            row.action_kind.as_deref().unwrap_or("none"),
                            opt_to_string(row.action_applied),
                            opt_to_string(row.bytes_reclaimed),
                            opt_to_string(row.files_acted_on),
                        ),
                        Format::Json => print_history_json(&HistoryJsonEvent::Scan {
                            id: row.id,
                            root: row.root,
                            started_at: row.started_at,
                            files_scanned: row.files_scanned,
                            bytes_scanned: row.bytes_scanned,
                            duplicate_groups: row.duplicate_groups,
                            duplicate_files: row.duplicate_files,
                            action_kind: row.action_kind,
                            action_applied: row.action_applied,
                            bytes_reclaimed: row.bytes_reclaimed,
                            files_acted_on: row.files_acted_on,
                        }),
                    }
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        },
        HistoryCommand::Stats { since, until } => match history::stats(&cli.db, since, until) {
            Ok(totals) => {
                match cli.format {
                    Format::Text => println!(
                        "{} scans, {} files scanned ({} bytes), {} duplicate groups ({} files), {} bytes reclaimed across {} files",
                        totals.scans,
                        totals.files_scanned,
                        totals.bytes_scanned,
                        totals.duplicate_groups,
                        totals.duplicate_files,
                        totals.bytes_reclaimed,
                        totals.files_acted_on,
                    ),
                    Format::Json => print_history_json(&HistoryJsonEvent::Stats {
                        scans: totals.scans,
                        files_scanned: totals.files_scanned,
                        bytes_scanned: totals.bytes_scanned,
                        duplicate_groups: totals.duplicate_groups,
                        duplicate_files: totals.duplicate_files,
                        bytes_reclaimed: totals.bytes_reclaimed,
                        files_acted_on: totals.files_acted_on,
                    }),
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

/// `history list`'s text-format placeholder for a field that's `None`
/// because the scan it belongs to had no `--action` (`report` mode).
fn opt_to_string<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_string(), |v| v.to_string())
}

fn print_history_json(event: &HistoryJsonEvent) {
    println!(
        "{}",
        serde_json::to_string(event).expect("HistoryJsonEvent always serializes")
    );
}

/// NDJSON event shape for `rusty-fclone history --format json`
/// (`CLI-HISTORY-AUDIT`). Field names are snake_case, matching
/// `JsonEvent`'s own convention.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HistoryJsonEvent {
    Scan {
        id: i64,
        root: String,
        started_at: i64,
        files_scanned: u64,
        bytes_scanned: u64,
        duplicate_groups: u64,
        duplicate_files: u64,
        action_kind: Option<String>,
        action_applied: Option<bool>,
        bytes_reclaimed: Option<u64>,
        files_acted_on: Option<u64>,
    },
    Stats {
        scans: u64,
        files_scanned: u64,
        bytes_scanned: u64,
        duplicate_groups: u64,
        duplicate_files: u64,
        bytes_reclaimed: u64,
        files_acted_on: u64,
    },
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
    let action_kind = match cli.action.as_core_kind(cli.archive_dir.as_deref()) {
        Ok(kind) => kind,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(kind) = &action_kind {
        if cli.apply && !cli.yes && !confirm_apply(&cli.root, kind, cli.find_duplicate_folders) {
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
        min_size: cli.min_size,
        max_size: cli.max_size,
        include_extensions: (!cli.include_extensions.is_empty()).then_some(cli.include_extensions),
        exclude_extensions: (!cli.exclude_extensions.is_empty()).then_some(cli.exclude_extensions),
        exclude_paths: cli.exclude_paths,
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
    // Per-action detail rows for `--history` (`CLI-HISTORY-AUDIT`) --
    // only collected when a history database is actually configured, so
    // a scan with `--history` unset pays nothing for this.
    let mut history_actions = cli.history.is_some().then(Vec::new);

    for event in handle {
        match event {
            ScanEvent::DuplicateGroup(group) => {
                progress_line.finish();
                if folder_dedup_root.is_some() {
                    // Defer both printing and any action for this group
                    // until after the folder-dedup pass runs below.
                    // Applying --action live here could delete the very
                    // files a folder match depends on before
                    // find_folder_duplicates ever sees them -- a
                    // fully-duplicated directory's defining evidence is
                    // exactly the individual file duplicates this loop
                    // would otherwise already be consuming (ADR-0023).
                    collected_groups.push(group);
                } else {
                    had_errors |= handle_group(
                        &group,
                        action_kind.clone(),
                        cli.keep_rule.as_core_rule(),
                        &cli.reference_paths,
                        cli.apply,
                        cli.format,
                        &mut total_bytes_reclaimed,
                        &mut total_files_acted_on,
                        history_actions.as_mut(),
                    );
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

    if let (Some(root), Some(options)) = (folder_dedup_root, folder_dedup_options) {
        // `folder_matches` defaults to empty on error -- the fallback
        // below then treats every collected group as unclaimed and
        // handles it exactly as the non-folder-dedup path would.
        let folder_matches = match find_folder_duplicates(&root, &collected_groups, &options) {
            Ok(matches) => matches,
            Err(err) => {
                had_errors = true;
                eprintln!("error: {err}");
                Vec::new()
            }
        };

        had_errors |= report_folder_matches(
            cli.format,
            &folder_matches,
            &collected_groups,
            &options,
            action_kind.as_ref(),
            &cli.reference_paths,
            cli.apply,
            &mut total_bytes_reclaimed,
            &mut total_files_acted_on,
            history_actions.as_mut(),
        );

        // Every group not entirely covered by a folder match above still
        // needs its own report (and, if requested, action) -- exactly
        // the behavior the non-folder-dedup path already provides, just
        // deferred to here so folder-dedup got first look at the
        // unmodified tree. A fully-covered group is deliberately not
        // printed again here -- its folder-level summary already covers
        // it, rather than also flooding the output with every file it
        // contains.
        let claimed_folders = folder_match_roots(&folder_matches);
        for group in &collected_groups {
            if group_fully_covered_by(group, &claimed_folders) {
                continue;
            }
            had_errors |= handle_group(
                group,
                action_kind.clone(),
                cli.keep_rule.as_core_rule(),
                &cli.reference_paths,
                cli.apply,
                cli.format,
                &mut total_bytes_reclaimed,
                &mut total_files_acted_on,
                history_actions.as_mut(),
            );
        }
    }

    // Printed after the folder-dedup pass above (not right after the scan
    // loop) so the total reflects both file-level group actions and any
    // folder-level ones -- one true grand total, not an undercount.
    if let Some(kind) = &action_kind {
        report_action_summary(
            cli.format,
            kind,
            cli.apply,
            total_bytes_reclaimed,
            total_files_acted_on,
        );
    }

    if let Some(history_path) = &cli.history {
        let record = history::ScanRecord {
            root: root_display,
            started_at,
            files_scanned: final_summary.files_scanned,
            bytes_scanned: final_summary.bytes_scanned,
            duplicate_groups: final_summary.duplicate_groups,
            duplicate_files: final_summary.duplicate_files,
            action_kind: action_kind.as_ref().map(action_word),
            action_applied: action_kind.as_ref().map(|_| cli.apply),
            bytes_reclaimed: action_kind.as_ref().map(|_| total_bytes_reclaimed),
            files_acted_on: action_kind.as_ref().map(|_| total_files_acted_on),
            actions: history_actions.unwrap_or_default(),
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
fn confirm_apply(root: &Path, kind: &ActionKind, find_duplicate_folders: bool) -> bool {
    let folders_note = if find_duplicate_folders {
        ", including whole duplicate folders,"
    } else {
        ""
    };
    eprint!(
        "This will scan {} and {} redundant files{folders_note} as duplicates are found. Proceed? [y/N] ",
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
    kind: &ActionKind,
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
#[allow(clippy::too_many_arguments)]
fn handle_group(
    group: &DuplicateGroup,
    action_kind: Option<ActionKind>,
    keep_rule: SelectRule,
    reference_paths: &[PathBuf],
    apply: bool,
    format: Format,
    total_bytes_reclaimed: &mut u64,
    total_files_acted_on: &mut u64,
    history_actions: Option<&mut Vec<history::ActionRecord>>,
) -> bool {
    let Some(kind) = action_kind else {
        print_group(format, group, None);
        return false;
    };

    let (keep, keep_reason) = select::choose_keep(group, keep_rule, reference_paths);
    let plan = action::plan_with_keep(group, &keep, kind.clone(), reference_paths);
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

    if let (Some(report), Some(history_actions)) = (&report, history_actions) {
        record_action_outcomes(
            history_actions,
            plan.actions.iter().map(|fa| (fa.path.as_path(), plan.size)),
            action_word(&kind),
            &report.succeeded,
            &report.failed,
        );
    }

    print_group(
        format,
        group,
        Some((kind, &plan, &keep_reason, apply, report.as_ref())),
    );

    if let Some(report) = &report {
        for err in &report.failed {
            eprintln!("warning: {err}");
        }
    }

    had_errors
}

/// Pushes one [`history::ActionRecord`] per `(path, bytes)` entry into
/// `history_actions`, correlating each planned action against its real
/// outcome in `succeeded`/`failed` (`CLI-HISTORY-AUDIT`). Shared by
/// [`handle_group`] (every `FileAction` in an [`ActionPlan`]) and
/// [`report_folder_matches`] (every `FolderFilePair` in a
/// `FolderActionPlan`) — `ApplyReport` and `FolderApplyReport` both
/// have `succeeded`/`failed` fields of these same two types, so one
/// function covers both without needing to know which report it's
/// reading from.
fn record_action_outcomes<'a>(
    history_actions: &mut Vec<history::ActionRecord>,
    entries: impl IntoIterator<Item = (&'a Path, u64)>,
    kind_word: &'static str,
    succeeded: &[PathBuf],
    failed: &[FileError],
) {
    for (path, bytes) in entries {
        let ok = succeeded.iter().any(|p| p.as_path() == path);
        let error = failed
            .iter()
            .find(|e| e.path.as_ref() == path)
            .map(|e| e.source.to_string());
        history_actions.push(history::ActionRecord {
            path: path.display().to_string(),
            kind: kind_word,
            bytes,
            succeeded: ok,
            error,
        });
    }
}

fn print_group(
    format: Format,
    group: &DuplicateGroup,
    action: Option<(ActionKind, &ActionPlan, &str, bool, Option<&ApplyReport>)>,
) {
    match format {
        Format::Text => print_group_text(group, action),
        Format::Json => print_group_json(group, action),
    }
}

fn print_group_text(
    group: &DuplicateGroup,
    action: Option<(ActionKind, &ActionPlan, &str, bool, Option<&ApplyReport>)>,
) {
    println!("--- {} bytes, {} copies ---", group.size, group.paths.len());
    for path in &group.paths {
        println!("{}", path.display());
    }
    let Some((kind, plan, keep_reason, ..)) = action else {
        return;
    };
    println!("  keep: {} ({keep_reason})", plan.kept.display());
    for file_action in &plan.actions {
        println!("  {}: {}", action_word(&kind), file_action.path.display());
    }
}

fn print_group_json(
    group: &DuplicateGroup,
    action: Option<(ActionKind, &ActionPlan, &str, bool, Option<&ApplyReport>)>,
) {
    let action = action.map(|(kind, plan, keep_reason, applied, report)| {
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
            kind: action_word(&kind),
            kept: plan.kept.display().to_string(),
            keep_reason: keep_reason.to_string(),
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

/// Every folder path involved in any folder match -- both sides of a
/// `Contained` match, every folder in an `Exact` cluster. Used to decide
/// whether a `DuplicateGroup` is already fully represented by the
/// folder-level output, so it isn't also printed (and acted on) a second
/// time as an individual group.
fn folder_match_roots(matches: &[FolderMatch]) -> Vec<&Path> {
    let mut roots = Vec::new();
    for m in matches {
        match m {
            FolderMatch::Exact { folders, .. } => roots.extend(folders.iter().map(|p| p.as_path())),
            FolderMatch::Contained {
                subset, superset, ..
            } => {
                roots.push(subset.as_path());
                roots.push(superset.as_path());
            }
        }
    }
    roots
}

/// `true` when every path in `group` sits under one of `roots` -- i.e.
/// this group's entire content is already covered by some folder match's
/// own output above, so reporting (and acting on) it again individually
/// would be redundant.
fn group_fully_covered_by(group: &DuplicateGroup, roots: &[&Path]) -> bool {
    group
        .paths
        .iter()
        .all(|p| roots.iter().any(|r| p.starts_with(r)))
}

/// One `(removed, kept)` folder pair to plan/apply `folder_action` for.
/// A `Contained` match is exactly one pair (`subset` removed against
/// `superset`); an `Exact` cluster of 2+ folders keeps the
/// alphabetically-first one (matching `action::plan`'s existing
/// "first path is kept" convention for files, ADR-0023) and removes
/// every other folder against it.
fn folder_match_pairs(m: &FolderMatch) -> Vec<(&Path, &Path)> {
    match m {
        FolderMatch::Contained {
            subset, superset, ..
        } => vec![(subset.as_path(), superset.as_path())],
        FolderMatch::Exact { folders, .. } => {
            let kept = folders
                .iter()
                .min()
                .expect("an Exact match always has at least 2 folders");
            folders
                .iter()
                .filter(|f| *f != kept)
                .map(|f| (f.as_path(), kept.as_path()))
                .collect()
        }
    }
}

/// The outcome of planning (and maybe applying) `kind` for one folder pair.
struct FolderPairOutcome {
    kind: ActionKind,
    removed: PathBuf,
    kept: PathBuf,
    file_count: u64,
    bytes: u64,
    applied: bool,
    directory_removed: bool,
    failed: usize,
}

/// Reports every folder match, planning (and, with `--apply`, applying)
/// `action_kind` for each `folder_match_pairs` pair when one was
/// requested. Returns whether any per-file or per-pair error occurred.
#[allow(clippy::too_many_arguments)]
fn report_folder_matches(
    format: Format,
    matches: &[FolderMatch],
    groups: &[DuplicateGroup],
    options: &ScanOptions,
    action_kind: Option<&ActionKind>,
    reference_paths: &[PathBuf],
    apply: bool,
    total_bytes_reclaimed: &mut u64,
    total_files_acted_on: &mut u64,
    mut history_actions: Option<&mut Vec<history::ActionRecord>>,
) -> bool {
    let mut had_errors = false;
    for m in matches {
        let mut outcomes = Vec::new();
        if let Some(kind) = action_kind {
            for (removed, kept) in folder_match_pairs(m) {
                match folder_action::plan_folder(
                    removed,
                    kept,
                    groups,
                    options,
                    kind.clone(),
                    reference_paths,
                ) {
                    Ok(plan) => {
                        let report = apply.then(|| folder_action::apply_folder(&plan));
                        let (bytes, files, failed, directory_removed) = match &report {
                            Some(r) => (
                                r.bytes_reclaimed,
                                r.succeeded.len() as u64,
                                r.failed.len(),
                                r.directory_removed,
                            ),
                            None => (plan.bytes_reclaimed, plan.pairs.len() as u64, 0, false),
                        };
                        *total_bytes_reclaimed += bytes;
                        *total_files_acted_on += files;
                        had_errors |= failed > 0;
                        if let Some(r) = &report {
                            for err in &r.failed {
                                eprintln!("warning: {err}");
                            }
                            if let Some(history_actions) = history_actions.as_mut() {
                                record_action_outcomes(
                                    history_actions,
                                    plan.pairs.iter().map(|p| (p.remove.as_path(), p.size)),
                                    action_word(kind),
                                    &r.succeeded,
                                    &r.failed,
                                );
                            }
                        }
                        outcomes.push(FolderPairOutcome {
                            kind: kind.clone(),
                            removed: removed.to_path_buf(),
                            kept: kept.to_path_buf(),
                            file_count: plan.pairs.len() as u64,
                            bytes: plan.bytes_reclaimed,
                            applied: report.is_some(),
                            directory_removed,
                            failed,
                        });
                    }
                    Err(err) => {
                        had_errors = true;
                        eprintln!("warning: {}: {err}", removed.display());
                    }
                }
            }
        }
        match format {
            Format::Text => print_folder_match_text(m, &outcomes),
            Format::Json => print_folder_match_json(m, &outcomes),
        }
    }
    had_errors
}

fn print_folder_match_text(m: &FolderMatch, outcomes: &[FolderPairOutcome]) {
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
    for o in outcomes {
        println!("  keep folder: {}", o.kept.display());
        let mut line = format!(
            "  {} folder: {} ({} files, {} bytes)",
            action_word(&o.kind),
            o.removed.display(),
            o.file_count,
            o.bytes
        );
        if o.applied {
            if o.directory_removed {
                line.push_str(" -- folder removed");
            }
            if o.failed > 0 {
                line.push_str(&format!(" -- {} file(s) failed", o.failed));
            }
        } else {
            line.push_str(" (preview -- pass --apply to actually do this)");
        }
        println!("{line}");
    }
}

fn print_folder_match_json(m: &FolderMatch, outcomes: &[FolderPairOutcome]) {
    let action: Vec<FolderPairActionJson> = outcomes
        .iter()
        .map(|o| FolderPairActionJson {
            kind: action_word(&o.kind),
            kept: o.kept.display().to_string(),
            removed: o.removed.display().to_string(),
            applied: o.applied,
            file_count: o.file_count,
            bytes: o.bytes,
            directory_removed: o.directory_removed,
            failed: o.failed as u64,
        })
        .collect();
    match m {
        FolderMatch::Exact {
            folders,
            file_count,
            bytes,
        } => print_json(&JsonEvent::FolderExact {
            folders: paths_to_strings(folders),
            file_count: *file_count,
            bytes: *bytes,
            action,
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
            action,
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
        action: Vec<FolderPairActionJson>,
    },
    FolderContained {
        subset: String,
        superset: String,
        file_count: u64,
        bytes: u64,
        action: Vec<FolderPairActionJson>,
    },
}

#[derive(Serialize)]
struct JsonAction {
    kind: &'static str,
    kept: String,
    keep_reason: String,
    applied: bool,
    planned: Vec<String>,
    succeeded: Vec<String>,
    failed: Vec<String>,
    bytes_reclaimed: u64,
}

/// One `folder_action` outcome for `--format json` (ADR-0023) — an empty
/// `action` array on the enclosing `FolderExact`/`FolderContained` event
/// means no `--action` was requested for this run. Field names are
/// already snake_case (matching `JsonAction` and this CLI's convention,
/// distinct from the GUI's camelCase wire format), so no `rename_all` is
/// needed.
#[derive(Serialize)]
struct FolderPairActionJson {
    kind: &'static str,
    kept: String,
    removed: String,
    applied: bool,
    file_count: u64,
    bytes: u64,
    directory_removed: bool,
    failed: u64,
}

fn action_word(kind: &ActionKind) -> &'static str {
    match kind {
        ActionKind::Delete => "delete",
        ActionKind::Trash => "trash",
        ActionKind::Hardlink => "hardlink",
        ActionKind::Reflink => "reflink",
        ActionKind::Move(_) => "move",
        ActionKind::Copy(_) => "copy",
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
            min_size: ScanOptions::default().min_size,
            max_size: ScanOptions::default().max_size,
            include_extensions: Vec::new(),
            exclude_extensions: Vec::new(),
            exclude_paths: ScanOptions::default().exclude_paths,
            action: Action::Report,
            keep_rule: KeepRule::Alphabetical,
            reference_paths: Vec::new(),
            archive_dir: None,
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
    fn action_with_apply_actually_trashes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Trash;
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(a.exists(), "the kept file must survive");
        assert!(
            !b.exists(),
            "the redundant copy must be gone from its original path"
        );
    }

    #[test]
    fn action_with_apply_actually_moves_into_the_archive_directory() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive");
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Move;
        cli.archive_dir = Some(archive.clone());
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(a.exists(), "the kept file must survive");
        assert!(!b.exists(), "the redundant copy is gone from its path");
        // The archived layout mirrors b's original absolute path
        // underneath `archive` (collision-safe across different original
        // directories) -- not a flat `archive/b.txt`.
        assert!(
            archive.join(b.strip_prefix("/").unwrap()).exists(),
            "the redundant copy must survive at its archived path"
        );
    }

    #[test]
    fn action_with_apply_actually_copies_into_the_archive_directory_and_keeps_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive");
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Copy;
        cli.archive_dir = Some(archive.clone());
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(a.exists());
        assert!(b.exists(), "Copy must not touch the original");
        assert!(
            archive.join(b.strip_prefix("/").unwrap()).exists(),
            "an archived copy must exist alongside the untouched original"
        );
    }

    #[test]
    fn action_move_without_archive_dir_fails_before_touching_anything() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Move;
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);

        assert_eq!(
            exit,
            ExitCode::FAILURE,
            "--action move without --archive-dir must be rejected"
        );
        assert!(a.exists());
        assert!(b.exists(), "a rejected run must not touch the filesystem");
    }

    #[test]
    fn keep_rule_newest_keeps_the_most_recently_modified_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(&b, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Delete;
        cli.keep_rule = KeepRule::Newest;
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(b.exists(), "the newest file (b) must be kept");
        assert!(!a.exists(), "the older file (a) must be removed");
    }

    #[test]
    fn reference_path_overrides_keep_rule_and_is_never_acted_on() {
        let dir = tempfile::tempdir().unwrap();
        let reference = dir.path().join("reference");
        fs::create_dir_all(&reference).unwrap();
        // z_protected.txt would lose to a.txt under --keep-rule alphabetical
        // (the default) on filename alone -- the reference guardrail must
        // still win.
        let protected = reference.join("z_protected.txt");
        let a = dir.path().join("a.txt");
        fs::write(&protected, b"dup").unwrap();
        fs::write(&a, b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.action = Action::Trash;
        cli.reference_paths = vec![reference];
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(protected.exists(), "the protected file must survive");
        assert!(!a.exists(), "its unprotected duplicate is still removed");
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
                ActionKind::Delete,
                &[]
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

    /// ADR-0023, `FCLONE-ACTION-001-FR-009`/`FR-010`/`FR-011`:
    /// `--find-duplicate-folders --action delete --apply` actually
    /// deletes a `Contained` match's subset folder (against its confirmed
    /// partner in the superset) and prunes the emptied folder, without
    /// touching the superset.
    #[test]
    fn find_duplicate_folders_with_action_delete_apply_removes_the_subset_folder() {
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
        cli.action = Action::Delete;
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(!small.exists(), "the emptied subset folder must be pruned");
        assert!(big.join("1.txt").exists(), "the superset must be untouched");
        assert!(big.join("extra.txt").exists());
    }

    /// ADR-0025: a protected file inside a folder-match's subset side
    /// must survive `--find-duplicate-folders --action delete --apply`,
    /// and the guardrail must also block the directory-prune step --
    /// pruning `small` here would silently delete the protected file it
    /// still contains.
    #[test]
    fn find_duplicate_folders_with_reference_protects_a_file_and_blocks_the_prune() {
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
        cli.action = Action::Delete;
        cli.reference_paths = vec![small.clone()];
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(
            small.join("1.txt").exists(),
            "the protected file must survive"
        );
        assert!(
            small.exists(),
            "the directory must not be pruned while it still holds a protected file"
        );
        assert!(big.join("1.txt").exists(), "the superset must be untouched");
        assert!(big.join("extra.txt").exists());
    }

    /// The other half: without `--apply`, `--find-duplicate-folders
    /// --action delete` must not touch the filesystem, only preview.
    #[test]
    fn find_duplicate_folders_with_action_delete_without_apply_is_a_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.find_duplicate_folders = true;
        cli.action = Action::Delete;
        cli.apply = false;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(
            small.join("1.txt").exists(),
            "dry run must not delete anything"
        );
        assert!(big.join("1.txt").exists());
    }

    /// A duplicate group entirely outside the matched folders must still
    /// get the normal per-file action, deferred to after the folder-dedup
    /// pass rather than skipped -- `group_fully_covered_by` must only
    /// suppress groups the folder-level output already represents.
    #[test]
    fn find_duplicate_folders_with_action_still_acts_on_an_unrelated_duplicate_pair() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();
        // Unrelated to either folder -- just two duplicate files sitting
        // directly in the scan root.
        let x = dir.path().join("x.txt");
        let y = dir.path().join("y.txt");
        fs::write(&x, b"unrelated dup").unwrap();
        fs::write(&y, b"unrelated dup").unwrap();

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.find_duplicate_folders = true;
        cli.action = Action::Delete;
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);

        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(!small.exists(), "the folder match must still be pruned");
        assert!(
            x.exists(),
            "the alphabetically-first unrelated file is kept"
        );
        assert!(
            !y.exists(),
            "the unrelated duplicate pair must still be acted on"
        );
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

    /// `CLI-HISTORY-AUDIT`: an applied action records one `actions` row
    /// per file, correlated with its real per-file outcome.
    #[test]
    fn history_flag_records_one_action_row_per_file_acted_on() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();
        let history_dir = tempfile::tempdir().unwrap();
        let history_path = history_dir.path().join("history.sqlite");

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.history = Some(history_path.clone());
        cli.action = Action::Trash;
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);
        assert_eq!(exit, ExitCode::SUCCESS);

        let conn = rusqlite::Connection::open(&history_path).unwrap();
        let (path, kind, bytes, succeeded, error): (String, String, i64, bool, Option<String>) =
            conn.query_row(
                "SELECT path, kind, bytes, succeeded, error FROM actions",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(path, b.display().to_string());
        assert_eq!(kind, "trash");
        assert_eq!(bytes, 3);
        assert!(succeeded);
        assert!(error.is_none());
    }

    /// A dry run (`--apply` not passed) records the scan summary as
    /// before, but no `actions` rows -- nothing real happened yet to
    /// audit.
    #[test]
    fn history_flag_records_no_action_rows_for_a_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"dup").unwrap();
        fs::write(dir.path().join("b.txt"), b"dup").unwrap();
        let history_dir = tempfile::tempdir().unwrap();
        let history_path = history_dir.path().join("history.sqlite");

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.history = Some(history_path.clone());
        cli.action = Action::Delete;
        cli.apply = false;
        let exit = run(cli);
        assert_eq!(exit, ExitCode::SUCCESS);

        let conn = rusqlite::Connection::open(&history_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    /// `--find-duplicate-folders` combined with `--action`/`--apply`
    /// records per-pair `actions` rows too, not just per-file ones.
    #[test]
    fn history_flag_records_action_rows_for_folder_level_actions() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();
        let history_dir = tempfile::tempdir().unwrap();
        let history_path = history_dir.path().join("history.sqlite");

        let mut cli = base_cli(dir.path().to_path_buf());
        cli.find_duplicate_folders = true;
        cli.history = Some(history_path.clone());
        cli.action = Action::Delete;
        cli.apply = true;
        cli.yes = true;
        let exit = run(cli);
        assert_eq!(exit, ExitCode::SUCCESS);

        let conn = rusqlite::Connection::open(&history_path).unwrap();
        let (path, kind): (String, String) = conn
            .query_row("SELECT path, kind FROM actions", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(path, small.join("1.txt").display().to_string());
        assert_eq!(kind, "delete");
    }

    #[test]
    fn history_list_reports_recent_scans_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        for (root, started_at) in [("/a", 1), ("/b", 2)] {
            let mut record = crate::history::ScanRecord {
                root: root.to_string(),
                started_at,
                files_scanned: 1,
                bytes_scanned: 1,
                duplicate_groups: 0,
                duplicate_files: 0,
                action_kind: None,
                action_applied: None,
                bytes_reclaimed: None,
                files_acted_on: None,
                actions: Vec::new(),
            };
            record.started_at = started_at;
            crate::history::record_scan(&db_path, &record).unwrap();
        }

        let cli = HistoryCli {
            db: db_path,
            format: Format::Text,
            command: HistoryCommand::List { limit: 10 },
        };
        let exit = run_history(cli);
        assert_eq!(exit, ExitCode::SUCCESS);
    }

    #[test]
    fn history_stats_aggregates_across_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        let record = crate::history::ScanRecord {
            root: "/a".to_string(),
            started_at: 1,
            files_scanned: 5,
            bytes_scanned: 500,
            duplicate_groups: 1,
            duplicate_files: 2,
            action_kind: Some("delete"),
            action_applied: Some(true),
            bytes_reclaimed: Some(500),
            files_acted_on: Some(1),
            actions: Vec::new(),
        };
        crate::history::record_scan(&db_path, &record).unwrap();

        let cli = HistoryCli {
            db: db_path,
            format: Format::Json,
            command: HistoryCommand::Stats {
                since: None,
                until: None,
            },
        };
        let exit = run_history(cli);
        assert_eq!(exit, ExitCode::SUCCESS);
    }

    #[test]
    fn history_subcommand_reports_failure_for_an_unreadable_database() {
        let cli = HistoryCli {
            db: PathBuf::from("/does/not/exist/history.sqlite"),
            format: Format::Text,
            command: HistoryCommand::List { limit: 10 },
        };
        let exit = run_history(cli);
        assert_eq!(exit, ExitCode::FAILURE);
    }
}
