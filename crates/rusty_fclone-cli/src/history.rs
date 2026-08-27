//! Scan-history persistence via SQLite, for longer-term analytics
//! (ADR-0017, `CLI-UX-001` revision).
//!
//! One row per completed scan (`scans`), plus, when the scan actually ran
//! an applied action, one row per individual file/pair outcome
//! (`actions`) -- what actually happened, not what was merely previewed
//! (`CLI-HISTORY-AUDIT`, ADR-0027). `record_scan` is the only place that
//! writes; `list_scans`/`stats` are read-only queries for the `history`
//! subcommand.

use std::path::Path;

use rusqlite::{params, Connection};

/// One completed scan's summary, ready to persist.
pub(crate) struct ScanRecord {
    pub root: String,
    /// Unix timestamp (seconds) when the scan started.
    pub started_at: i64,
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub duplicate_groups: u64,
    pub duplicate_files: u64,
    /// `None` when `--action` was `report` (no action requested).
    pub action_kind: Option<&'static str>,
    pub action_applied: Option<bool>,
    pub bytes_reclaimed: Option<u64>,
    pub files_acted_on: Option<u64>,
    /// One row per individual file/pair this scan actually acted on.
    /// Empty whenever `--apply` wasn't passed (a preview plans actions
    /// but never runs them, so there's nothing real to audit yet) or
    /// `--action` was `report`.
    pub actions: Vec<ActionRecord>,
}

/// One file/pair a scan actually acted on -- the per-action counterpart
/// of `ScanRecord`'s aggregate totals.
pub(crate) struct ActionRecord {
    pub path: String,
    pub kind: &'static str,
    pub bytes: u64,
    pub succeeded: bool,
    /// The error's `Display` text when `succeeded` is `false`, `None`
    /// otherwise.
    pub error: Option<String>,
}

/// Opens (creating if necessary) a history database at `path`, ensures its
/// schema exists, and inserts one row for `record` plus one row per entry
/// in `record.actions`.
pub(crate) fn record_scan(path: &Path, record: &ScanRecord) -> rusqlite::Result<()> {
    let mut conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS scans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            root TEXT NOT NULL,
            started_at INTEGER NOT NULL,
            files_scanned INTEGER NOT NULL,
            bytes_scanned INTEGER NOT NULL,
            duplicate_groups INTEGER NOT NULL,
            duplicate_files INTEGER NOT NULL,
            action_kind TEXT,
            action_applied INTEGER,
            bytes_reclaimed INTEGER,
            files_acted_on INTEGER
        );
        CREATE TABLE IF NOT EXISTS actions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scan_id INTEGER NOT NULL REFERENCES scans(id),
            path TEXT NOT NULL,
            kind TEXT NOT NULL,
            bytes INTEGER NOT NULL,
            succeeded INTEGER NOT NULL,
            error TEXT
        )",
    )?;
    let tx = conn.transaction()?;
    // SQLite's native integer type is a signed 64-bit int; rusqlite has no
    // `ToSql` for `u64` since it can't losslessly represent every u64
    // value. A plain `as i64` cast is safe here -- file/byte counts never
    // approach i64::MAX in practice.
    tx.execute(
        "INSERT INTO scans (
            root, started_at, files_scanned, bytes_scanned,
            duplicate_groups, duplicate_files,
            action_kind, action_applied, bytes_reclaimed, files_acted_on
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            record.root,
            record.started_at,
            record.files_scanned as i64,
            record.bytes_scanned as i64,
            record.duplicate_groups as i64,
            record.duplicate_files as i64,
            record.action_kind,
            record.action_applied,
            record.bytes_reclaimed.map(|v| v as i64),
            record.files_acted_on.map(|v| v as i64),
        ],
    )?;
    let scan_id = tx.last_insert_rowid();
    for action in &record.actions {
        tx.execute(
            "INSERT INTO actions (scan_id, path, kind, bytes, succeeded, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                scan_id,
                action.path,
                action.kind,
                action.bytes as i64,
                action.succeeded,
                action.error,
            ],
        )?;
    }
    tx.commit()
}

/// One `scans` row as returned by [`list_scans`].
pub(crate) struct ScanRow {
    pub id: i64,
    pub root: String,
    pub started_at: i64,
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub duplicate_groups: u64,
    pub duplicate_files: u64,
    pub action_kind: Option<String>,
    pub action_applied: Option<bool>,
    pub bytes_reclaimed: Option<u64>,
    pub files_acted_on: Option<u64>,
}

/// The most recent `limit` scans, newest first. Creates the schema (empty)
/// if the database doesn't exist yet, the same "never error on an unused
/// database" posture `record_scan` already has.
pub(crate) fn list_scans(path: &Path, limit: u32) -> rusqlite::Result<Vec<ScanRow>> {
    let conn = open_with_schema(path)?;
    let mut stmt = conn.prepare(
        "SELECT id, root, started_at, files_scanned, bytes_scanned,
                duplicate_groups, duplicate_files,
                action_kind, action_applied, bytes_reclaimed, files_acted_on
         FROM scans ORDER BY started_at DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(ScanRow {
                id: row.get(0)?,
                root: row.get(1)?,
                started_at: row.get(2)?,
                files_scanned: row.get::<_, i64>(3)? as u64,
                bytes_scanned: row.get::<_, i64>(4)? as u64,
                duplicate_groups: row.get::<_, i64>(5)? as u64,
                duplicate_files: row.get::<_, i64>(6)? as u64,
                action_kind: row.get(7)?,
                action_applied: row.get(8)?,
                bytes_reclaimed: row.get::<_, Option<i64>>(9)?.map(|v| v as u64),
                files_acted_on: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Aggregate totals across every scan with `started_at` in
/// `[since, until]` (either bound optional -- `None` means unbounded on
/// that side).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct HistoryStats {
    pub scans: u64,
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub duplicate_groups: u64,
    pub duplicate_files: u64,
    pub bytes_reclaimed: u64,
    pub files_acted_on: u64,
}

pub(crate) fn stats(
    path: &Path,
    since: Option<i64>,
    until: Option<i64>,
) -> rusqlite::Result<HistoryStats> {
    let conn = open_with_schema(path)?;
    conn.query_row(
        "SELECT
            COUNT(*),
            COALESCE(SUM(files_scanned), 0),
            COALESCE(SUM(bytes_scanned), 0),
            COALESCE(SUM(duplicate_groups), 0),
            COALESCE(SUM(duplicate_files), 0),
            COALESCE(SUM(bytes_reclaimed), 0),
            COALESCE(SUM(files_acted_on), 0)
         FROM scans
         WHERE (?1 IS NULL OR started_at >= ?1)
           AND (?2 IS NULL OR started_at <= ?2)",
        params![since, until],
        |row| {
            Ok(HistoryStats {
                scans: row.get::<_, i64>(0)? as u64,
                files_scanned: row.get::<_, i64>(1)? as u64,
                bytes_scanned: row.get::<_, i64>(2)? as u64,
                duplicate_groups: row.get::<_, i64>(3)? as u64,
                duplicate_files: row.get::<_, i64>(4)? as u64,
                bytes_reclaimed: row.get::<_, i64>(5)? as u64,
                files_acted_on: row.get::<_, i64>(6)? as u64,
            })
        },
    )
}

/// Opens `path`, creating the `scans`/`actions` schema if the database is
/// new -- shared by every read-only query so `history list`/`history
/// stats` against a database no scan has ever written to returns an
/// empty result instead of a "no such table" error.
fn open_with_schema(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS scans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            root TEXT NOT NULL,
            started_at INTEGER NOT NULL,
            files_scanned INTEGER NOT NULL,
            bytes_scanned INTEGER NOT NULL,
            duplicate_groups INTEGER NOT NULL,
            duplicate_files INTEGER NOT NULL,
            action_kind TEXT,
            action_applied INTEGER,
            bytes_reclaimed INTEGER,
            files_acted_on INTEGER
        );
        CREATE TABLE IF NOT EXISTS actions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scan_id INTEGER NOT NULL REFERENCES scans(id),
            path TEXT NOT NULL,
            kind TEXT NOT NULL,
            bytes INTEGER NOT NULL,
            succeeded INTEGER NOT NULL,
            error TEXT
        )",
    )?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(root: &str) -> ScanRecord {
        ScanRecord {
            root: root.to_string(),
            started_at: 1_700_000_000,
            files_scanned: 10,
            bytes_scanned: 1024,
            duplicate_groups: 1,
            duplicate_files: 2,
            action_kind: None,
            action_applied: None,
            bytes_reclaimed: None,
            files_acted_on: None,
            actions: Vec::new(),
        }
    }

    #[test]
    fn creates_the_database_and_schema_on_first_use() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        record_scan(&db_path, &sample_record("/some/tree")).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM scans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn records_every_field_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        let record = ScanRecord {
            root: "/tree".to_string(),
            started_at: 42,
            files_scanned: 100,
            bytes_scanned: 5_000_000,
            duplicate_groups: 3,
            duplicate_files: 7,
            action_kind: Some("delete"),
            action_applied: Some(true),
            bytes_reclaimed: Some(123_456),
            files_acted_on: Some(4),
            actions: Vec::new(),
        };
        record_scan(&db_path, &record).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let (root, started_at, files_scanned, action_kind, applied, reclaimed): (
            String,
            i64,
            i64,
            Option<String>,
            Option<bool>,
            Option<i64>,
        ) = conn
            .query_row(
                "SELECT root, started_at, files_scanned, action_kind, action_applied, bytes_reclaimed FROM scans",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(root, "/tree");
        assert_eq!(started_at, 42);
        assert_eq!(files_scanned, 100);
        assert_eq!(action_kind.as_deref(), Some("delete"));
        assert_eq!(applied, Some(true));
        assert_eq!(reclaimed, Some(123_456));
    }

    #[test]
    fn multiple_scans_append_rather_than_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        record_scan(&db_path, &sample_record("/a")).unwrap();
        record_scan(&db_path, &sample_record("/b")).unwrap();
        record_scan(&db_path, &sample_record("/c")).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM scans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn reopening_an_existing_database_does_not_lose_prior_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        record_scan(&db_path, &sample_record("/first")).unwrap();
        // A second call re-runs "CREATE TABLE IF NOT EXISTS" against an
        // already-populated database -- must not clear it.
        record_scan(&db_path, &sample_record("/second")).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let roots: Vec<String> = {
            let mut stmt = conn.prepare("SELECT root FROM scans ORDER BY id").unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(roots, vec!["/first", "/second"]);
    }

    #[test]
    fn records_one_action_row_per_entry_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        let mut record = sample_record("/tree");
        record.actions = vec![
            ActionRecord {
                path: "/tree/a.txt".to_string(),
                kind: "trash",
                bytes: 100,
                succeeded: true,
                error: None,
            },
            ActionRecord {
                path: "/tree/b.txt".to_string(),
                kind: "trash",
                bytes: 200,
                succeeded: false,
                error: Some("permission denied".to_string()),
            },
        ];
        record_scan(&db_path, &record).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let mut stmt = conn
            .prepare("SELECT path, kind, bytes, succeeded, error FROM actions ORDER BY id")
            .unwrap();
        let rows: Vec<(String, String, i64, bool, Option<String>)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(
            rows,
            vec![
                (
                    "/tree/a.txt".to_string(),
                    "trash".to_string(),
                    100,
                    true,
                    None
                ),
                (
                    "/tree/b.txt".to_string(),
                    "trash".to_string(),
                    200,
                    false,
                    Some("permission denied".to_string())
                ),
            ]
        );
    }

    #[test]
    fn action_rows_reference_the_correct_scan_when_multiple_scans_exist() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        record_scan(&db_path, &sample_record("/first")).unwrap();
        let mut second = sample_record("/second");
        second.actions = vec![ActionRecord {
            path: "/second/x.txt".to_string(),
            kind: "delete",
            bytes: 1,
            succeeded: true,
            error: None,
        }];
        record_scan(&db_path, &second).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let (root, count): (String, i64) = conn
            .query_row(
                "SELECT scans.root, COUNT(actions.id) FROM scans
                 LEFT JOIN actions ON actions.scan_id = scans.id
                 GROUP BY scans.id HAVING COUNT(actions.id) > 0",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(root, "/second");
        assert_eq!(count, 1);
    }

    #[test]
    fn list_scans_returns_the_most_recent_first_up_to_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        for (root, started_at) in [("/a", 1), ("/b", 2), ("/c", 3)] {
            let mut record = sample_record(root);
            record.started_at = started_at;
            record_scan(&db_path, &record).unwrap();
        }

        let rows = list_scans(&db_path, 2).unwrap();
        let roots: Vec<&str> = rows.iter().map(|r| r.root.as_str()).collect();
        assert_eq!(roots, vec!["/c", "/b"], "newest first, limited to 2");
    }

    #[test]
    fn list_scans_against_a_database_no_scan_has_written_to_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        let rows = list_scans(&db_path, 10).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn stats_aggregates_every_scan_when_no_date_range_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        let mut a = sample_record("/a");
        a.started_at = 100;
        a.bytes_reclaimed = Some(50);
        a.files_acted_on = Some(2);
        let mut b = sample_record("/b");
        b.started_at = 200;
        b.bytes_reclaimed = Some(30);
        b.files_acted_on = Some(1);
        record_scan(&db_path, &a).unwrap();
        record_scan(&db_path, &b).unwrap();

        let totals = stats(&db_path, None, None).unwrap();
        assert_eq!(totals.scans, 2);
        assert_eq!(totals.bytes_reclaimed, 80);
        assert_eq!(totals.files_acted_on, 3);
    }

    #[test]
    fn stats_filters_by_the_given_date_range() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        let mut a = sample_record("/a");
        a.started_at = 100;
        a.bytes_reclaimed = Some(50);
        let mut b = sample_record("/b");
        b.started_at = 200;
        b.bytes_reclaimed = Some(30);
        let mut c = sample_record("/c");
        c.started_at = 300;
        c.bytes_reclaimed = Some(10);
        record_scan(&db_path, &a).unwrap();
        record_scan(&db_path, &b).unwrap();
        record_scan(&db_path, &c).unwrap();

        let totals = stats(&db_path, Some(150), Some(250)).unwrap();
        assert_eq!(totals.scans, 1, "only /b falls in [150, 250]");
        assert_eq!(totals.bytes_reclaimed, 30);
    }

    #[test]
    fn stats_against_a_database_no_scan_has_written_to_returns_zeroes() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.sqlite");
        let totals = stats(&db_path, None, None).unwrap();
        assert_eq!(totals, HistoryStats::default());
    }
}
