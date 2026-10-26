//! Reviewable plan files, the apply journal, and undo (`ACTION-PLAN-FILE`,
//! ADR-0032).
//!
//! `--plan-out` turns a scan's preview into a JSON file a human can read,
//! diff, and edit (delete groups they disagree with) before anything
//! happens. `rusty-fclone plan apply` re-checks every file against the
//! size/mtime recorded at plan time — skipping a whole group if anything in
//! it changed — then executes through the core action layer unchanged and
//! appends one JSONL line per completed action to a journal.
//! `rusty-fclone plan undo` reverses the journal's `move` entries.
//!
//! CLI-only by design: serialization stays out of `rusty_fclone-core`
//! (AGENTS.md). Execution is delegated to `action::plan_with_keep`/`apply`
//! so a plan file can never do anything a live `--apply` run couldn't.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use rusty_fclone_core::action::{self, ActionKind, ActionPlan, ApplyReport};
use rusty_fclone_core::DuplicateGroup;
use serde::{Deserialize, Serialize};

/// Value of a plan file's `format` field — rejects arbitrary JSON early.
pub const FORMAT: &str = "rusty-fclone-plan";
/// Plan file schema version; bump on any incompatible change.
pub const VERSION: u32 = 1;

/// Errors reading or writing plan and journal files.
#[derive(Debug)]
pub enum PlanFileError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        line: Option<usize>,
        msg: String,
    },
    Invalid {
        path: PathBuf,
        msg: String,
    },
}

impl PlanFileError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_owned(),
            source,
        }
    }
}

impl fmt::Display for PlanFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Parse {
                path,
                line: Some(line),
                msg,
            } => {
                write!(f, "{}:{line}: {msg}", path.display())
            }
            Self::Parse {
                path,
                line: None,
                msg,
            } => write!(f, "{}: {msg}", path.display()),
            Self::Invalid { path, msg } => write!(f, "{}: {msg}", path.display()),
        }
    }
}

impl std::error::Error for PlanFileError {}

/// The action a plan file applies. Mirrors [`ActionKind`] one-to-one, but
/// as a tagged serde enum so `move`/`copy` can't exist without their
/// archive directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanAction {
    Delete,
    Trash,
    Hardlink,
    Reflink,
    Move { archive_dir: PathBuf },
    Copy { archive_dir: PathBuf },
}

impl PlanAction {
    fn from_kind(kind: &ActionKind) -> io::Result<Self> {
        Ok(match kind {
            ActionKind::Delete => Self::Delete,
            ActionKind::Trash => Self::Trash,
            ActionKind::Hardlink => Self::Hardlink,
            ActionKind::Reflink => Self::Reflink,
            ActionKind::Move(dir) => Self::Move {
                archive_dir: std::path::absolute(dir)?,
            },
            ActionKind::Copy(dir) => Self::Copy {
                archive_dir: std::path::absolute(dir)?,
            },
        })
    }

    /// The core action this plan executes.
    pub fn to_kind(&self) -> ActionKind {
        match self {
            Self::Delete => ActionKind::Delete,
            Self::Trash => ActionKind::Trash,
            Self::Hardlink => ActionKind::Hardlink,
            Self::Reflink => ActionKind::Reflink,
            Self::Move { archive_dir } => ActionKind::Move(archive_dir.clone()),
            Self::Copy { archive_dir } => ActionKind::Copy(archive_dir.clone()),
        }
    }
}

/// A path plus the modification time it had when the plan was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileStamp {
    pub path: PathBuf,
    /// Nanoseconds since the Unix epoch.
    pub mtime_ns: u64,
}

/// One duplicate group's planned action: keep `kept`, act on `targets`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanGroup {
    /// Size in bytes shared by every file in the group.
    pub size: u64,
    pub kept: FileStamp,
    pub targets: Vec<FileStamp>,
}

/// A complete, reviewable plan as written by `--plan-out`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanFile {
    pub format: String,
    pub version: u32,
    /// The scan root, absolute.
    pub root: PathBuf,
    /// Unix seconds when the plan was written.
    pub created_at: i64,
    pub action: PlanAction,
    pub groups: Vec<PlanGroup>,
}

impl PlanFile {
    /// An empty plan for `kind` over `root`. Paths are made absolute so the
    /// plan applies identically from any working directory.
    pub fn new(root: &Path, created_at: i64, kind: &ActionKind) -> io::Result<Self> {
        Ok(Self {
            format: FORMAT.to_owned(),
            version: VERSION,
            root: std::path::absolute(root)?,
            created_at,
            action: PlanAction::from_kind(kind)?,
            groups: Vec::new(),
        })
    }

    /// Number of files the plan would act on.
    pub fn target_count(&self) -> usize {
        self.groups.iter().map(|g| g.targets.len()).sum()
    }

    /// Writes the plan as pretty JSON, replacing any existing file.
    pub fn write(&self, path: &Path) -> Result<(), PlanFileError> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| PlanFileError::Invalid {
            path: path.to_owned(),
            msg: format!("cannot serialize plan (non-UTF-8 path?): {e}"),
        })?;
        fs::write(path, json).map_err(|e| PlanFileError::io(path, e))
    }

    /// Reads and validates a plan file.
    pub fn read(path: &Path) -> Result<Self, PlanFileError> {
        let bytes = fs::read(path).map_err(|e| PlanFileError::io(path, e))?;
        let plan: Self = serde_json::from_slice(&bytes).map_err(|e| PlanFileError::Parse {
            path: path.to_owned(),
            line: Some(e.line()),
            msg: e.to_string(),
        })?;
        let invalid = |msg: String| PlanFileError::Invalid {
            path: path.to_owned(),
            msg,
        };
        if plan.format != FORMAT {
            return Err(invalid(format!(
                "not a plan file (format {:?})",
                plan.format
            )));
        }
        if plan.version != VERSION {
            return Err(invalid(format!(
                "plan version {} unsupported (expected {VERSION})",
                plan.version
            )));
        }
        Ok(plan)
    }
}

/// Records `plan`'s kept file and targets with their current mtimes, as
/// absolute paths.
pub fn stamp_plan(plan: &ActionPlan) -> io::Result<PlanGroup> {
    Ok(PlanGroup {
        size: plan.size,
        kept: stamp(&plan.kept)?,
        targets: plan
            .actions
            .iter()
            .map(|a| stamp(&a.path))
            .collect::<Result<_, _>>()?,
    })
}

/// Fails for non-UTF-8 paths up front: JSON can't carry them, and failing
/// here drops one group (with a warning) instead of the whole plan write.
fn stamp(path: &Path) -> io::Result<FileStamp> {
    if path.to_str().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "non-UTF-8 path can't be written to a plan file",
        ));
    }
    let meta = fs::symlink_metadata(path)?;
    Ok(FileStamp {
        path: std::path::absolute(path)?,
        mtime_ns: mtime_ns(&meta)?,
    })
}

fn mtime_ns(meta: &fs::Metadata) -> io::Result<u64> {
    let since_epoch = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    u64::try_from(since_epoch.as_nanos()).map_err(io::Error::other)
}

/// Why a group is unsafe to act on now, or `None` if every file in it still
/// has the size and mtime recorded at plan time. A stale group is skipped
/// whole: if the kept file changed, acting on its "duplicates" could
/// destroy the only remaining copy of the old content.
pub fn staleness(group: &PlanGroup) -> Option<String> {
    std::iter::once(&group.kept)
        .chain(&group.targets)
        .find_map(|s| stamp_mismatch(s, group.size))
}

fn stamp_mismatch(stamp: &FileStamp, size: u64) -> Option<String> {
    let path = stamp.path.display();
    let meta = match fs::symlink_metadata(&stamp.path) {
        Ok(meta) => meta,
        Err(e) => return Some(format!("{path}: {e}")),
    };
    if !meta.is_file() {
        return Some(format!("{path}: no longer a regular file"));
    }
    if meta.len() != size {
        return Some(format!("{path}: size changed ({} -> {})", size, meta.len()));
    }
    match mtime_ns(&meta) {
        Ok(ns) if ns == stamp.mtime_ns => None,
        Ok(_) => Some(format!("{path}: modified since the plan was written")),
        Err(e) => Some(format!("{path}: {e}")),
    }
}

/// Rebuilds the core [`ActionPlan`] for `group` through
/// `action::plan_with_keep`, so hardlink-alias filtering and byte
/// accounting stay identical to a live run.
pub fn to_action_plan(group: &PlanGroup, kind: &ActionKind) -> ActionPlan {
    let mut paths: Vec<Arc<Path>> = std::iter::once(&group.kept)
        .chain(&group.targets)
        .map(|s| Arc::from(s.path.as_path()))
        .collect();
    paths.sort();
    let dup = DuplicateGroup {
        size: group.size,
        paths,
    };
    action::plan_with_keep(&dup, &group.kept.path, kind.clone(), &[])
}

/// One completed action, as appended to the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum JournalEntry {
    Delete { path: PathBuf },
    Trash { path: PathBuf },
    Hardlink { path: PathBuf, kept: PathBuf },
    Reflink { path: PathBuf, kept: PathBuf },
    Move { from: PathBuf, to: PathBuf },
    Copy { from: PathBuf, to: PathBuf },
}

impl JournalEntry {
    /// The entry for `path` having been acted on with `kind`.
    pub fn record(kind: &ActionKind, path: &Path, kept: &Path) -> Self {
        let (path, kept) = (path.to_owned(), kept.to_owned());
        match kind {
            ActionKind::Delete => Self::Delete { path },
            ActionKind::Trash => Self::Trash { path },
            ActionKind::Hardlink => Self::Hardlink { path, kept },
            ActionKind::Reflink => Self::Reflink { path, kept },
            ActionKind::Move(dir) => Self::Move {
                to: action::archived_path(dir, &path),
                from: path,
            },
            ActionKind::Copy(dir) => Self::Copy {
                to: action::archived_path(dir, &path),
                from: path,
            },
        }
    }
}

/// Append-only JSONL journal. Each [`Journal::append`] ends with an fsync,
/// so a crash loses at most the group in flight.
pub struct Journal {
    file: File,
    path: PathBuf,
}

impl Journal {
    /// Opens (creating if needed) a journal for appending.
    pub fn open(path: &Path) -> Result<Self, PlanFileError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| PlanFileError::io(path, e))?;
        Ok(Self {
            file,
            path: path.to_owned(),
        })
    }

    /// Appends `entries` and syncs them to disk.
    pub fn append(&mut self, entries: &[JournalEntry]) -> Result<(), PlanFileError> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut buf = Vec::new();
        for entry in entries {
            serde_json::to_writer(&mut buf, entry).map_err(|e| PlanFileError::Invalid {
                path: self.path.clone(),
                msg: format!("cannot serialize journal entry: {e}"),
            })?;
            buf.push(b'\n');
        }
        self.file
            .write_all(&buf)
            .and_then(|()| self.file.sync_data())
            .map_err(|e| PlanFileError::io(&self.path, e))
    }
}

/// Default journal location: `<plan>.journal.jsonl`.
pub fn default_journal_path(plan: &Path) -> PathBuf {
    let mut name = OsString::from(plan.as_os_str());
    name.push(".journal.jsonl");
    PathBuf::from(name)
}

/// Reads every journal entry. A malformed line is an error, never skipped.
pub fn read_journal(path: &Path) -> Result<Vec<JournalEntry>, PlanFileError> {
    let file = File::open(path).map_err(|e| PlanFileError::io(path, e))?;
    let mut entries = Vec::new();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| PlanFileError::io(path, e))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry = serde_json::from_str(&line).map_err(|e| PlanFileError::Parse {
            path: path.to_owned(),
            line: Some(i + 1),
            msg: e.to_string(),
        })?;
        entries.push(entry);
    }
    Ok(entries)
}

/// Moves an archived file back to its original path. Refuses to overwrite
/// anything already at `from`.
pub fn restore(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(from).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("original path is occupied: {}", from.display()),
        ));
    }
    if let Some(parent) = from.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(to, from)
}

/// Totals from `plan apply`, for the summary line and exit code.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ApplyTotals {
    pub groups: usize,
    pub stale: usize,
    pub files: usize,
    pub failed: usize,
    pub bytes: u64,
}

impl ApplyTotals {
    fn add(&mut self, plan: &ActionPlan, report: Option<&ApplyReport>) {
        self.groups += 1;
        match report {
            Some(r) => {
                self.files += r.succeeded.len();
                self.failed += r.failed.len();
                self.bytes += r.bytes_reclaimed;
            }
            None => {
                self.files += plan.actions.len();
                self.bytes += plan.bytes_reclaimed;
            }
        }
    }

    /// Whether anything was skipped or failed.
    pub fn had_problems(&self) -> bool {
        self.stale > 0 || self.failed > 0
    }
}

/// Executes (or, without `journal`, previews) every group in `plan`,
/// writing one line per file to `out`. Preview is the default: passing a
/// journal is what makes this mutate the filesystem (ADR-0009).
pub fn apply_plan(
    plan: &PlanFile,
    mut journal: Option<&mut Journal>,
    out: &mut impl Write,
) -> Result<ApplyTotals, PlanFileError> {
    let kind = plan.action.to_kind();
    let verb = crate::action_word(&kind);
    let mut totals = ApplyTotals::default();
    for group in &plan.groups {
        if let Some(reason) = staleness(group) {
            totals.stale += 1;
            let _ = writeln!(out, "skip (stale) {}: {reason}", group.kept.path.display());
            continue;
        }
        let action_plan = to_action_plan(group, &kind);
        let Some(journal) = journal.as_deref_mut() else {
            for a in &action_plan.actions {
                let _ = writeln!(out, "would {verb} {}", a.path.display());
            }
            totals.add(&action_plan, None);
            continue;
        };
        let report = action::apply(&action_plan);
        let entries: Vec<_> = report
            .succeeded
            .iter()
            .map(|p| JournalEntry::record(&kind, p, &action_plan.kept))
            .collect();
        journal.append(&entries)?;
        for p in &report.succeeded {
            let _ = writeln!(out, "{verb} {}", p.display());
        }
        for e in &report.failed {
            let _ = writeln!(out, "failed {e}");
        }
        totals.add(&action_plan, Some(&report));
    }
    Ok(totals)
}

/// Totals from `plan undo`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UndoTotals {
    pub restored: usize,
    pub failed: usize,
    pub not_undoable: usize,
}

/// Reverses `entries` newest-first. Only `move` is undoable; everything
/// else is reported, never guessed at. Previews unless `apply`.
pub fn undo(entries: &[JournalEntry], apply: bool, out: &mut impl Write) -> UndoTotals {
    let mut totals = UndoTotals::default();
    for entry in entries.iter().rev() {
        let JournalEntry::Move { from, to } = entry else {
            totals.not_undoable += 1;
            let _ = writeln!(out, "not undoable: {entry:?}");
            continue;
        };
        if !apply {
            totals.restored += 1;
            let _ = writeln!(out, "would restore {} -> {}", to.display(), from.display());
            continue;
        }
        match restore(from, to) {
            Ok(()) => {
                totals.restored += 1;
                let _ = writeln!(out, "restored {}", from.display());
            }
            Err(e) => {
                totals.failed += 1;
                let _ = writeln!(out, "failed {}: {e}", from.display());
            }
        }
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, body: &str) -> PathBuf {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().expect("has parent")).expect("mkdir");
        fs::write(&p, body).expect("write");
        p
    }

    fn group_of(kept: &Path, targets: &[&Path]) -> PlanGroup {
        let size = fs::metadata(kept).expect("kept exists").len();
        PlanGroup {
            size,
            kept: stamp(kept).expect("stamp"),
            targets: targets.iter().map(|t| stamp(t).expect("stamp")).collect(),
        }
    }

    fn plan_with(dir: &Path, kind: ActionKind, groups: Vec<PlanGroup>) -> PlanFile {
        let mut plan = PlanFile::new(dir, 0, &kind).expect("plan");
        plan.groups = groups;
        plan
    }

    #[test]
    fn plan_file_round_trips() {
        let t = TempDir::new().expect("tempdir");
        let a = write(t.path(), "a", "x");
        let b = write(t.path(), "b", "x");
        let plan = plan_with(
            t.path(),
            ActionKind::Move(t.path().join("q")),
            vec![group_of(&a, &[&b])],
        );
        let file = t.path().join("plan.json");
        plan.write(&file).expect("write");
        assert_eq!(PlanFile::read(&file).expect("read"), plan);
        assert_eq!(plan.target_count(), 1);
    }

    #[test]
    fn read_rejects_foreign_or_future_files() {
        let t = TempDir::new().expect("tempdir");
        let file = t.path().join("p.json");
        let mut plan = plan_with(t.path(), ActionKind::Delete, vec![]);
        plan.version = VERSION + 1;
        plan.write(&file).expect("write");
        assert!(matches!(
            PlanFile::read(&file),
            Err(PlanFileError::Invalid { .. })
        ));

        plan.version = VERSION;
        plan.format = "something-else".into();
        plan.write(&file).expect("write");
        assert!(matches!(
            PlanFile::read(&file),
            Err(PlanFileError::Invalid { .. })
        ));

        fs::write(&file, "{ not json").expect("write");
        assert!(matches!(
            PlanFile::read(&file),
            Err(PlanFileError::Parse { .. })
        ));
    }

    #[test]
    fn move_without_archive_dir_is_unrepresentable() {
        let json = r#"{"kind":"move"}"#;
        assert!(serde_json::from_str::<PlanAction>(json).is_err());
        let ok = r#"{"kind":"move","archive_dir":"/q"}"#;
        assert!(serde_json::from_str::<PlanAction>(ok).is_ok());
    }

    #[test]
    fn staleness_detects_size_mtime_and_missing_changes() {
        let t = TempDir::new().expect("tempdir");
        let a = write(t.path(), "a", "xx");
        let b = write(t.path(), "b", "xx");
        let group = group_of(&a, &[&b]);
        assert_eq!(staleness(&group), None);

        let mut touched = group.clone();
        touched.targets[0].mtime_ns += 1;
        assert!(staleness(&touched).expect("stale").contains("modified"));

        fs::write(&a, "xxx").expect("grow kept");
        assert!(staleness(&group).expect("stale").contains("size changed"));

        fs::remove_file(&b).expect("rm");
        fs::write(&a, "xx").expect("restore size");
        let fresh_kept = PlanGroup {
            kept: stamp(&a).expect("stamp"),
            ..group
        };
        assert!(staleness(&fresh_kept).is_some(), "missing target is stale");
    }

    #[test]
    fn preview_touches_nothing() {
        let t = TempDir::new().expect("tempdir");
        let a = write(t.path(), "a", "x");
        let b = write(t.path(), "b", "x");
        let plan = plan_with(t.path(), ActionKind::Delete, vec![group_of(&a, &[&b])]);
        let mut out = Vec::new();
        let totals = apply_plan(&plan, None, &mut out).expect("preview");
        assert!(b.exists());
        assert_eq!((totals.groups, totals.files, totals.bytes), (1, 1, 1));
        assert!(String::from_utf8_lossy(&out).contains("would delete"));
    }

    #[test]
    fn apply_moves_journals_and_undo_restores() {
        let t = TempDir::new().expect("tempdir");
        let a = write(t.path(), "src/a", "dup");
        let b = write(t.path(), "src/sub/b", "dup");
        let q = t.path().join("quarantine");
        let plan = plan_with(
            t.path(),
            ActionKind::Move(q.clone()),
            vec![group_of(&a, &[&b])],
        );
        let jpath = t.path().join("j.jsonl");

        let mut journal = Journal::open(&jpath).expect("journal");
        let totals = apply_plan(&plan, Some(&mut journal), &mut io::sink()).expect("apply");
        assert_eq!((totals.files, totals.failed, totals.stale), (1, 0, 0));
        assert!(!b.exists() && a.exists());
        let archived = action::archived_path(&q, &std::path::absolute(&b).expect("abs"));
        assert!(archived.exists());

        let entries = read_journal(&jpath).expect("read journal");
        assert_eq!(
            entries,
            vec![JournalEntry::Move {
                from: std::path::absolute(&b).expect("abs"),
                to: archived.clone()
            }]
        );

        let preview = undo(&entries, false, &mut io::sink());
        assert_eq!(preview.restored, 1);
        assert!(!b.exists(), "undo preview touches nothing");

        let done = undo(&entries, true, &mut io::sink());
        assert_eq!((done.restored, done.failed), (1, 0));
        assert_eq!(fs::read_to_string(&b).expect("restored"), "dup");
        assert!(!archived.exists());

        let again = undo(&entries, true, &mut io::sink());
        assert_eq!(
            again.failed, 1,
            "second undo refuses: archived copy is gone"
        );
    }

    #[test]
    fn stale_group_is_skipped_whole() {
        let t = TempDir::new().expect("tempdir");
        let a = write(t.path(), "a", "dup");
        let b = write(t.path(), "b", "dup");
        let c = write(t.path(), "c", "dup");
        let plan = plan_with(t.path(), ActionKind::Delete, vec![group_of(&a, &[&b, &c])]);
        fs::write(&a, "new").expect("kept edited, same size");
        let mut stale = plan.clone();
        stale.groups[0].kept.mtime_ns -= 1;

        let jpath = t.path().join("j.jsonl");
        let mut journal = Journal::open(&jpath).expect("journal");
        let totals = apply_plan(&stale, Some(&mut journal), &mut io::sink()).expect("apply");
        assert_eq!((totals.stale, totals.files), (1, 0));
        assert!(b.exists() && c.exists(), "no target touched");
        assert!(totals.had_problems());
        assert!(read_journal(&jpath).expect("journal").is_empty());
    }

    #[test]
    fn restore_never_overwrites() {
        let t = TempDir::new().expect("tempdir");
        let from = write(t.path(), "orig", "new content");
        let to = write(t.path(), "archived", "old");
        let err = restore(&from, &to).expect_err("occupied");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&from).expect("read"), "new content");
    }

    #[test]
    fn non_move_entries_are_reported_not_undone() {
        let entries = vec![
            JournalEntry::Delete { path: "/x".into() },
            JournalEntry::Copy {
                from: "/a".into(),
                to: "/q/a".into(),
            },
        ];
        let mut out = Vec::new();
        let totals = undo(&entries, true, &mut out);
        assert_eq!((totals.not_undoable, totals.restored), (2, 0));
        assert!(String::from_utf8_lossy(&out).contains("not undoable"));
    }

    #[test]
    fn journal_rejects_malformed_lines_with_line_numbers() {
        let t = TempDir::new().expect("tempdir");
        let p = t.path().join("j.jsonl");
        fs::write(&p, "{\"op\":\"delete\",\"path\":\"/x\"}\n\ngarbage\n").expect("write");
        assert!(matches!(
            read_journal(&p),
            Err(PlanFileError::Parse { line: Some(3), .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_fail_at_stamp_time() {
        use std::os::unix::ffi::OsStrExt;
        let t = TempDir::new().expect("tempdir");
        let bad = t.path().join(std::ffi::OsStr::from_bytes(b"bad\xff"));
        fs::write(&bad, "x").expect("write");
        let err = stamp(&bad).expect_err("non-UTF-8 path");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn default_journal_path_appends_suffix() {
        assert_eq!(
            default_journal_path(Path::new("/p/plan.json")),
            PathBuf::from("/p/plan.json.journal.jsonl")
        );
    }
}
