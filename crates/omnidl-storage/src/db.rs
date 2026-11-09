//! The SQLite database: connection, migrations and repositories.

use std::{path::Path, str::FromStr, time::Duration};

use sqlx::{
    Row, SqlitePool,
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous,
    },
};

use crate::Result;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// One row of `tasks`. Enumerations are their snake_case names, sizes are bytes, times are Unix
/// milliseconds (SQLite integers are signed; sizes fit comfortably).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub final_url: Option<String>,
    pub save_dir: String,
    pub file_name: String,
    pub status: String,
    pub prev_status: Option<String>,
    pub total_bytes: Option<i64>,
    pub downloaded: i64,
    pub uploaded: i64,
    pub category: String,
    pub priority: i64,
    pub sort_order: i64,
    pub options_json: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub resumable: bool,
    pub error_json: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

/// One segment of a download: bytes `start..=end` (`end` is `None` while the length is unknown),
/// of which the first `downloaded` are on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentRecord {
    pub start: i64,
    pub end: Option<i64>,
    pub downloaded: i64,
}

/// The database. Cheap to clone (a connection pool).
#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

/// The columns of `tasks`, in [`TaskRecord`] order. A macro so statements stay static strings
/// (sqlx refuses SQL built at run time).
macro_rules! task_columns {
    () => {
        "id, kind, source, final_url, save_dir, file_name, status, prev_status, \
     total_bytes, downloaded, uploaded, category, priority, sort_order, options_json, etag, \
     last_modified, resumable, error_json, created_at, started_at, completed_at"
    };
}

impl Db {
    /// Opens (creating it if needed) the database at `path` and brings its schema up to date.
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    /// A private in-memory database (tests). One connection: each would get its own database.
    pub async fn memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    /// Closes the pool, waiting for queries in flight. The WAL is checkpointed on close.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn insert_task(&self, t: &TaskRecord) -> Result<()> {
        sqlx::query(concat!(
            "INSERT INTO tasks (",
            task_columns!(),
            ") VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ))
        .bind(&t.id)
        .bind(&t.kind)
        .bind(&t.source)
        .bind(&t.final_url)
        .bind(&t.save_dir)
        .bind(&t.file_name)
        .bind(&t.status)
        .bind(&t.prev_status)
        .bind(t.total_bytes)
        .bind(t.downloaded)
        .bind(t.uploaded)
        .bind(&t.category)
        .bind(t.priority)
        .bind(t.sort_order)
        .bind(&t.options_json)
        .bind(&t.etag)
        .bind(&t.last_modified)
        .bind(t.resumable)
        .bind(&t.error_json)
        .bind(t.created_at)
        .bind(t.started_at)
        .bind(t.completed_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Writes every column but `id`, `kind` and `created_at`, which never change.
    pub async fn update_task(&self, t: &TaskRecord) -> Result<()> {
        sqlx::query(
            "UPDATE tasks SET source = ?, final_url = ?, save_dir = ?, file_name = ?, status = ?, \
             prev_status = ?, total_bytes = ?, downloaded = ?, uploaded = ?, category = ?, \
             priority = ?, sort_order = ?, options_json = ?, etag = ?, last_modified = ?, \
             resumable = ?, error_json = ?, started_at = ?, completed_at = ? WHERE id = ?",
        )
        .bind(&t.source)
        .bind(&t.final_url)
        .bind(&t.save_dir)
        .bind(&t.file_name)
        .bind(&t.status)
        .bind(&t.prev_status)
        .bind(t.total_bytes)
        .bind(t.downloaded)
        .bind(t.uploaded)
        .bind(&t.category)
        .bind(t.priority)
        .bind(t.sort_order)
        .bind(&t.options_json)
        .bind(&t.etag)
        .bind(&t.last_modified)
        .bind(t.resumable)
        .bind(&t.error_json)
        .bind(t.started_at)
        .bind(t.completed_at)
        .bind(&t.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_task(&self, id: &str) -> Result<Option<TaskRecord>> {
        let row = sqlx::query(concat!(
            "SELECT ",
            task_columns!(),
            " FROM tasks WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| task_from_row(&r)).transpose()
    }

    /// Every task, in list order.
    pub async fn list_tasks(&self) -> Result<Vec<TaskRecord>> {
        let rows = sqlx::query(concat!(
            "SELECT ",
            task_columns!(),
            " FROM tasks ORDER BY sort_order, created_at"
        ))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(task_from_row).collect()
    }

    /// Deletes tasks and (by cascade) their segments.
    pub async fn delete_tasks(&self, ids: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for id in ids {
            sqlx::query("DELETE FROM tasks WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The `sort_order` that puts a new task at the end of the list.
    pub async fn next_sort_order(&self) -> Result<i64> {
        let n: Option<i64> = sqlx::query_scalar("SELECT MAX(sort_order) FROM tasks")
            .fetch_one(&self.pool)
            .await?;
        Ok(n.map_or(0, |n| n + 1))
    }

    /// Sets `sort_order` to each task's position in `ids`.
    pub async fn reorder(&self, ids: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for (i, id) in ids.iter().enumerate() {
            sqlx::query("UPDATE tasks SET sort_order = ? WHERE id = ?")
                .bind(i as i64)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Saves a download's progress in one transaction: the byte counts on the task and its
    /// segment table, which replaces the previous one (segments split while downloading).
    pub async fn save_progress(
        &self,
        id: &str,
        total_bytes: Option<i64>,
        downloaded: i64,
        segments: &[SegmentRecord],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE tasks SET total_bytes = ?, downloaded = ? WHERE id = ?")
            .bind(total_bytes)
            .bind(downloaded)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM segments WHERE task_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for (i, s) in segments.iter().enumerate() {
            sqlx::query(
                "INSERT INTO segments (task_id, idx, start_pos, end_pos, downloaded) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(i as i64)
            .bind(s.start)
            .bind(s.end)
            .bind(s.downloaded)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn segments(&self, id: &str) -> Result<Vec<SegmentRecord>> {
        let rows = sqlx::query(
            "SELECT start_pos, end_pos, downloaded FROM segments WHERE task_id = ? ORDER BY idx",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(SegmentRecord {
                    start: r.try_get(0)?,
                    end: r.try_get(1)?,
                    downloaded: r.try_get(2)?,
                })
            })
            .collect()
    }

    pub async fn clear_segments(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM segments WHERE task_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Bytes a torrent task uploaded in total (saved while it seeds).
    pub async fn save_uploaded(&self, id: &str, uploaded: i64) -> Result<()> {
        sqlx::query("UPDATE tasks SET uploaded = ? WHERE id = ?")
            .bind(uploaded)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn task_from_row(r: &SqliteRow) -> Result<TaskRecord> {
    Ok(TaskRecord {
        id: r.try_get("id")?,
        kind: r.try_get("kind")?,
        source: r.try_get("source")?,
        final_url: r.try_get("final_url")?,
        save_dir: r.try_get("save_dir")?,
        file_name: r.try_get("file_name")?,
        status: r.try_get("status")?,
        prev_status: r.try_get("prev_status")?,
        total_bytes: r.try_get("total_bytes")?,
        downloaded: r.try_get("downloaded")?,
        uploaded: r.try_get("uploaded")?,
        category: r.try_get("category")?,
        priority: r.try_get("priority")?,
        sort_order: r.try_get("sort_order")?,
        options_json: r.try_get("options_json")?,
        etag: r.try_get("etag")?,
        last_modified: r.try_get("last_modified")?,
        resumable: r.try_get("resumable")?,
        error_json: r.try_get("error_json")?,
        created_at: r.try_get("created_at")?,
        started_at: r.try_get("started_at")?,
        completed_at: r.try_get("completed_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, sort_order: i64) -> TaskRecord {
        TaskRecord {
            id: id.into(),
            kind: "http".into(),
            source: format!("https://example.com/{id}.bin"),
            final_url: None,
            save_dir: "/tmp".into(),
            file_name: format!("{id}.bin"),
            status: "queued".into(),
            prev_status: None,
            total_bytes: None,
            downloaded: 0,
            uploaded: 0,
            category: "other".into(),
            priority: 1,
            sort_order,
            options_json: "{}".into(),
            etag: None,
            last_modified: None,
            resumable: false,
            error_json: None,
            created_at: 1,
            started_at: None,
            completed_at: None,
        }
    }

    #[tokio::test]
    async fn round_trips_tasks_and_segments() {
        let db = Db::memory().await.unwrap();
        assert_eq!(db.next_sort_order().await.unwrap(), 0);
        db.insert_task(&record("a", 0)).await.unwrap();
        db.insert_task(&record("b", 1)).await.unwrap();
        assert_eq!(db.next_sort_order().await.unwrap(), 2);

        let mut a = db.get_task("a").await.unwrap().unwrap();
        assert_eq!(a, record("a", 0));
        a.status = "downloading".into();
        a.etag = Some("\"x\"".into());
        a.resumable = true;
        db.update_task(&a).await.unwrap();
        assert_eq!(db.get_task("a").await.unwrap().unwrap(), a);

        let segs = [
            SegmentRecord {
                start: 0,
                end: Some(99),
                downloaded: 50,
            },
            SegmentRecord {
                start: 100,
                end: Some(199),
                downloaded: 0,
            },
        ];
        db.save_progress("a", Some(200), 50, &segs).await.unwrap();
        assert_eq!(db.segments("a").await.unwrap(), segs);
        let a = db.get_task("a").await.unwrap().unwrap();
        assert_eq!((a.total_bytes, a.downloaded), (Some(200), 50));

        // A later checkpoint replaces the table (a segment was split meanwhile).
        let segs = [SegmentRecord {
            start: 0,
            end: None,
            downloaded: 7,
        }];
        db.save_progress("a", None, 7, &segs).await.unwrap();
        assert_eq!(db.segments("a").await.unwrap(), segs);

        db.reorder(&["b".into(), "a".into()]).await.unwrap();
        let ids: Vec<_> = db
            .list_tasks()
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["b", "a"]);

        db.delete_tasks(&["a".into()]).await.unwrap();
        assert!(db.get_task("a").await.unwrap().is_none());
        assert!(
            db.segments("a").await.unwrap().is_empty(),
            "segments cascade"
        );
    }

    #[tokio::test]
    async fn reopens_a_file_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("omni.db");
        let db = Db::open(&path).await.unwrap();
        db.insert_task(&record("a", 0)).await.unwrap();
        db.close().await;
        let db = Db::open(&path).await.unwrap();
        assert_eq!(db.list_tasks().await.unwrap().len(), 1);
        db.close().await;
    }
}
