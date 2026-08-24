//! Scan-history persistence via SQLite, for longer-term analytics
//! (ADR-0017, `CLI-UX-001` revision).
//!
//! Deliberately scoped to per-scan summaries only -- one row per
//! completed scan, not one row per file or per duplicate group. That's
//! what makes trend queries ("bytes reclaimed over time") possible
//! without a table that grows unbounded with tree size. Querying/
//! reporting against this data is left to a future unit (or ad hoc SQL);
//! this module only ever writes.

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
}

/// Opens (creating if necessary) a history database at `path`, ensures its
/// schema exists, and inserts one row for `record`.
pub(crate) fn record_scan(path: &Path, record: &ScanRecord) -> rusqlite::Result<()> {
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
        )",
    )?;
    // SQLite's native integer type is a signed 64-bit int; rusqlite has no
    // `ToSql` for `u64` since it can't losslessly represent every u64
    // value. A plain `as i64` cast is safe here -- file/byte counts never
    // approach i64::MAX in practice.
    conn.execute(
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
    Ok(())
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
}
