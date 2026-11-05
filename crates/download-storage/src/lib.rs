//! SQLite-backed jobs, settings, and durable segment checkpoints.

use std::path::Path;

use download_common::{AppSettings, DownloadJob, DownloadState, Theme};
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use thiserror::Error;
use uuid::Uuid;

const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_initial.sql"))];

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid persisted download state: {0}")]
    InvalidState(String),
    #[error("invalid persisted UUID: {0}")]
    InvalidUuid(#[from] uuid::Error),
    #[error("numeric value cannot be represented safely")]
    NumericRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentCheckpoint {
    pub index: u8,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct DownloadStorage {
    pool: SqlitePool,
}

impl DownloadStorage {
    /// Opens or creates a SQLite database and applies embedded migrations.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or migrated.
    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
        let pool = SqlitePool::connect_with(options).await?;
        let storage = Self { pool };
        storage.migrate().await?;
        Ok(storage)
    }

    /// Creates an isolated in-memory database, primarily for tests.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be initialized.
    pub async fn in_memory() -> Result<Self, StorageError> {
        let pool = SqlitePool::connect("sqlite::memory:").await?;
        let storage = Self { pool };
        storage.migrate().await?;
        Ok(storage)
    }

    async fn migrate(&self) -> Result<(), StorageError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _grabbit_migrations (version INTEGER PRIMARY KEY NOT NULL)",
        )
        .execute(&self.pool)
        .await?;
        for (version, sql) in MIGRATIONS {
            let applied: Option<i64> =
                sqlx::query_scalar("SELECT version FROM _grabbit_migrations WHERE version = ?")
                    .bind(version)
                    .fetch_optional(&self.pool)
                    .await?;
            if applied.is_some() {
                continue;
            }
            let mut transaction = self.pool.begin().await?;
            for statement in sql
                .split(';')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                sqlx::query(statement).execute(&mut *transaction).await?;
            }
            sqlx::query("INSERT INTO _grabbit_migrations(version) VALUES (?)")
                .bind(version)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
        }
        Ok(())
    }

    /// Inserts a new job without overwriting an existing identifier.
    ///
    /// # Errors
    ///
    /// Returns a database or numeric conversion error.
    pub async fn insert_job(&self, job: &DownloadJob) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO download_jobs (
                id, url, destination, file_name, state, total_bytes, downloaded_bytes,
                speed_bytes_per_second, eta_seconds, segments, etag, last_modified,
                sha256, error, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(job.id.to_string())
        .bind(&job.url)
        .bind(&job.destination)
        .bind(&job.file_name)
        .bind(state_name(job.state))
        .bind(optional_i64(job.total_bytes)?)
        .bind(to_i64(job.downloaded_bytes)?)
        .bind(to_i64(job.speed_bytes_per_second)?)
        .bind(optional_i64(job.eta_seconds)?)
        .bind(i64::from(job.segments))
        .bind(&job.etag)
        .bind(&job.last_modified)
        .bind(&job.sha256)
        .bind(&job.error)
        .bind(job.created_at)
        .bind(job.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lists jobs newest first.
    ///
    /// # Errors
    ///
    /// Returns an error for database failures or corrupt persisted values.
    pub async fn list_jobs(&self) -> Result<Vec<DownloadJob>, StorageError> {
        let rows = sqlx::query("SELECT * FROM download_jobs ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(row_to_job).collect()
    }

    /// Fetches one job.
    ///
    /// # Errors
    ///
    /// Returns an error for database failures or corrupt persisted values.
    pub async fn get_job(&self, id: Uuid) -> Result<Option<DownloadJob>, StorageError> {
        let row = sqlx::query("SELECT * FROM download_jobs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(row_to_job).transpose()
    }

    /// Updates state and optional user-facing error.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn update_state(
        &self,
        id: Uuid,
        state: DownloadState,
        error: Option<&str>,
        updated_at: i64,
    ) -> Result<(), StorageError> {
        sqlx::query("UPDATE download_jobs SET state = ?, error = ?, updated_at = ? WHERE id = ?")
            .bind(state_name(state))
            .bind(error)
            .bind(updated_at)
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Persists aggregate progress after the caller has durably written bytes.
    ///
    /// # Errors
    ///
    /// Returns a database or numeric conversion error.
    pub async fn update_progress(
        &self,
        id: Uuid,
        downloaded: u64,
        speed: u64,
        eta: Option<u64>,
        updated_at: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE download_jobs SET downloaded_bytes = ?, speed_bytes_per_second = ?,
             eta_seconds = ?, updated_at = ? WHERE id = ?",
        )
        .bind(to_i64(downloaded)?)
        .bind(to_i64(speed)?)
        .bind(optional_i64(eta)?)
        .bind(updated_at)
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Replaces segment layout for a job.
    ///
    /// # Errors
    ///
    /// Returns a database or numeric conversion error.
    pub async fn replace_segments(
        &self,
        id: Uuid,
        segments: &[SegmentCheckpoint],
    ) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM segment_checkpoints WHERE job_id = ?")
            .bind(id.to_string())
            .execute(&mut *transaction)
            .await?;
        for segment in segments {
            sqlx::query(
                "INSERT INTO segment_checkpoints
                 (job_id, segment_index, start_byte, end_byte, downloaded_bytes)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id.to_string())
            .bind(i64::from(segment.index))
            .bind(to_i64(segment.start_byte)?)
            .bind(to_i64(segment.end_byte)?)
            .bind(to_i64(segment.downloaded_bytes)?)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Updates one segment checkpoint after its file writes are durable.
    ///
    /// # Errors
    ///
    /// Returns a database or numeric conversion error.
    pub async fn checkpoint_segment(
        &self,
        id: Uuid,
        index: u8,
        downloaded: u64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE segment_checkpoints SET downloaded_bytes = ?
             WHERE job_id = ? AND segment_index = ?",
        )
        .bind(to_i64(downloaded)?)
        .bind(id.to_string())
        .bind(i64::from(index))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Loads segment checkpoints in index order.
    ///
    /// # Errors
    ///
    /// Returns a database or numeric conversion error.
    pub async fn segments(&self, id: Uuid) -> Result<Vec<SegmentCheckpoint>, StorageError> {
        let rows = sqlx::query(
            "SELECT segment_index, start_byte, end_byte, downloaded_bytes
             FROM segment_checkpoints WHERE job_id = ? ORDER BY segment_index",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(SegmentCheckpoint {
                    index: to_u8(row.try_get("segment_index")?)?,
                    start_byte: to_u64(row.try_get("start_byte")?)?,
                    end_byte: to_u64(row.try_get("end_byte")?)?,
                    downloaded_bytes: to_u64(row.try_get("downloaded_bytes")?)?,
                })
            })
            .collect()
    }

    /// Returns the sum of durable per-segment bytes.
    ///
    /// # Errors
    ///
    /// Returns a database or numeric conversion error.
    pub async fn checkpointed_bytes(&self, id: Uuid) -> Result<u64, StorageError> {
        let value: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(downloaded_bytes), 0) FROM segment_checkpoints WHERE job_id = ?",
        )
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await?;
        to_u64(value)
    }

    /// Marks jobs interrupted during active transfer as paused for recovery.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn reconcile_interrupted(&self, updated_at: i64) -> Result<u64, StorageError> {
        let result = sqlx::query(
            "UPDATE download_jobs SET state = 'paused',
             error = 'Interrupted; source will be revalidated before resume', updated_at = ?
             WHERE state IN ('inspecting', 'downloading')",
        )
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Deletes a history record and its cascading segment metadata.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn delete_job(&self, id: Uuid) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM download_jobs WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Loads application settings or returns safe defaults.
    ///
    /// # Errors
    ///
    /// Returns an error for database failures or invalid stored values.
    pub async fn settings(&self) -> Result<AppSettings, StorageError> {
        let row = sqlx::query("SELECT * FROM app_settings WHERE singleton = 1")
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(AppSettings::default());
        };
        Ok(AppSettings {
            max_concurrent_downloads: to_u8(row.try_get::<i64, _>("max_concurrent_downloads")?)?,
            default_segments: to_u8(row.try_get::<i64, _>("default_segments")?)?,
            default_download_directory: row.try_get("default_download_directory")?,
            theme: parse_theme(row.try_get("theme")?)?,
        })
    }

    /// Stores validated application settings.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn save_settings(&self, settings: &AppSettings) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO app_settings
             (singleton, max_concurrent_downloads, default_segments, default_download_directory, theme)
             VALUES (1, ?, ?, ?, ?)
             ON CONFLICT(singleton) DO UPDATE SET
             max_concurrent_downloads=excluded.max_concurrent_downloads,
             default_segments=excluded.default_segments,
             default_download_directory=excluded.default_download_directory,
             theme=excluded.theme",
        )
        .bind(i64::from(settings.max_concurrent_downloads.clamp(1, 32)))
        .bind(i64::from(settings.default_segments.clamp(1, 32)))
        .bind(&settings.default_download_directory)
        .bind(theme_name(settings.theme))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn row_to_job(row: &sqlx::sqlite::SqliteRow) -> Result<DownloadJob, StorageError> {
    Ok(DownloadJob {
        id: Uuid::parse_str(row.try_get("id")?)?,
        url: row.try_get("url")?,
        destination: row.try_get("destination")?,
        file_name: row.try_get("file_name")?,
        state: parse_state(row.try_get("state")?)?,
        total_bytes: optional_u64(row.try_get("total_bytes")?)?,
        downloaded_bytes: to_u64(row.try_get("downloaded_bytes")?)?,
        speed_bytes_per_second: to_u64(row.try_get("speed_bytes_per_second")?)?,
        eta_seconds: optional_u64(row.try_get("eta_seconds")?)?,
        segments: to_u8(row.try_get("segments")?)?,
        etag: row.try_get("etag")?,
        last_modified: row.try_get("last_modified")?,
        sha256: row.try_get("sha256")?,
        error: row.try_get("error")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn state_name(state: DownloadState) -> &'static str {
    match state {
        DownloadState::Queued => "queued",
        DownloadState::Inspecting => "inspecting",
        DownloadState::Downloading => "downloading",
        DownloadState::Paused => "paused",
        DownloadState::Completed => "completed",
        DownloadState::Failed => "failed",
        DownloadState::Cancelled => "cancelled",
    }
}

fn parse_state(value: &str) -> Result<DownloadState, StorageError> {
    match value {
        "queued" => Ok(DownloadState::Queued),
        "inspecting" => Ok(DownloadState::Inspecting),
        "downloading" => Ok(DownloadState::Downloading),
        "paused" => Ok(DownloadState::Paused),
        "completed" => Ok(DownloadState::Completed),
        "failed" => Ok(DownloadState::Failed),
        "cancelled" => Ok(DownloadState::Cancelled),
        _ => Err(StorageError::InvalidState(value.to_owned())),
    }
}

fn theme_name(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "system",
        Theme::Light => "light",
        Theme::Dark => "dark",
    }
}

fn parse_theme(value: &str) -> Result<Theme, StorageError> {
    match value {
        "system" => Ok(Theme::System),
        "light" => Ok(Theme::Light),
        "dark" => Ok(Theme::Dark),
        _ => Err(StorageError::InvalidState(value.to_owned())),
    }
}

fn to_i64(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::NumericRange)
}

fn optional_i64(value: Option<u64>) -> Result<Option<i64>, StorageError> {
    value.map(to_i64).transpose()
}

fn to_u64(value: i64) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::NumericRange)
}

fn optional_u64(value: Option<i64>) -> Result<Option<u64>, StorageError> {
    value.map(to_u64).transpose()
}

fn to_u8(value: i64) -> Result<u8, StorageError> {
    u8::try_from(value).map_err(|_| StorageError::NumericRange)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_job() -> DownloadJob {
        DownloadJob {
            id: Uuid::new_v4(),
            url: "https://example.com/file".into(),
            destination: "/tmp/file".into(),
            file_name: "file".into(),
            state: DownloadState::Downloading,
            total_bytes: Some(100),
            downloaded_bytes: 20,
            speed_bytes_per_second: 10,
            eta_seconds: Some(8),
            segments: 2,
            etag: Some("\"v1\"".into()),
            last_modified: None,
            sha256: None,
            error: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[tokio::test]
    async fn persists_jobs_settings_and_reconciles_crashes() {
        let storage = DownloadStorage::in_memory().await.expect("storage");
        let job = fixture_job();
        storage.insert_job(&job).await.expect("insert");
        assert_eq!(
            storage.get_job(job.id).await.expect("get"),
            Some(job.clone())
        );
        assert_eq!(
            storage.reconcile_interrupted(2).await.expect("reconcile"),
            1
        );
        let recovered = storage.get_job(job.id).await.expect("get").expect("job");
        assert_eq!(recovered.state, DownloadState::Paused);

        let settings = AppSettings {
            max_concurrent_downloads: 5,
            default_segments: 8,
            default_download_directory: "/tmp".into(),
            theme: Theme::Dark,
        };
        storage
            .save_settings(&settings)
            .await
            .expect("save settings");
        assert_eq!(storage.settings().await.expect("settings"), settings);
    }

    #[tokio::test]
    async fn stores_segment_checkpoints() {
        let storage = DownloadStorage::in_memory().await.expect("storage");
        let job = fixture_job();
        storage.insert_job(&job).await.expect("insert");
        storage
            .replace_segments(
                job.id,
                &[
                    SegmentCheckpoint {
                        index: 0,
                        start_byte: 0,
                        end_byte: 49,
                        downloaded_bytes: 0,
                    },
                    SegmentCheckpoint {
                        index: 1,
                        start_byte: 50,
                        end_byte: 99,
                        downloaded_bytes: 0,
                    },
                ],
            )
            .await
            .expect("segments");
        storage
            .checkpoint_segment(job.id, 0, 50)
            .await
            .expect("checkpoint");
    }
}
