//! SQLite-backed jobs, settings, and durable segment checkpoints.

use std::path::Path;

use download_common::{
    AppSettings, BrowserCaptureMode, DownloadJob, DownloadKind, DownloadState, Theme,
};
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use thiserror::Error;
use uuid::Uuid;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_initial.sql")),
    (2, include_str!("../migrations/0002_browser_media.sql")),
    (
        3,
        include_str!("../migrations/0003_browser_capture_mode.sql"),
    ),
];

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRecord {
    pub id: Uuid,
    pub source_url: String,
    pub title: String,
    pub website: Option<String>,
    pub destination: String,
    pub selected_format_ids: Vec<String>,
    pub output_format: String,
    pub stage: String,
    pub temporary_path: String,
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
                id, url, destination, file_name, job_kind, state, total_bytes, downloaded_bytes,
                speed_bytes_per_second, eta_seconds, segments, etag, last_modified,
                sha256, error, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(job.id.to_string())
        .bind(&job.url)
        .bind(&job.destination)
        .bind(&job.file_name)
        .bind(kind_name(job.kind))
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
             WHERE state IN ('inspecting', 'downloading', 'merging', 'converting', 'verifying')",
        )
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE media_jobs SET state = 'paused', stage = 'paused',
             error = 'Interrupted; media transfer will restart', updated_at = ?
             WHERE state IN ('inspecting', 'downloading', 'merging', 'converting', 'verifying')",
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
        let preferences = self.browser_preferences().await?;
        Ok(AppSettings {
            max_concurrent_downloads: to_u8(row.try_get::<i64, _>("max_concurrent_downloads")?)?,
            default_segments: to_u8(row.try_get::<i64, _>("default_segments")?)?,
            default_download_directory: row.try_get("default_download_directory")?,
            theme: parse_theme(row.try_get("theme")?)?,
            browser_capture_mode: preferences.browser_capture_mode,
            browser_excluded_sites: preferences.browser_excluded_sites,
            yt_dlp_path: preferences.yt_dlp_path,
            ffmpeg_path: preferences.ffmpeg_path,
            ffprobe_path: preferences.ffprobe_path,
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
        let excluded = serde_json::to_string(&settings.browser_excluded_sites)
            .map_err(|error| StorageError::InvalidState(error.to_string()))?;
        let capture_enabled = settings.browser_capture_mode != BrowserCaptureMode::Off;
        let automatic_interception = settings.browser_capture_mode == BrowserCaptureMode::Automatic;
        sqlx::query(
            "INSERT INTO browser_preferences
             (singleton, capture_enabled, automatic_interception, capture_mode, excluded_sites_json,
              yt_dlp_path, ffmpeg_path, ffprobe_path)
             VALUES (1, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(singleton) DO UPDATE SET
             capture_enabled=excluded.capture_enabled,
             automatic_interception=excluded.automatic_interception,
             capture_mode=excluded.capture_mode,
             excluded_sites_json=excluded.excluded_sites_json,
             yt_dlp_path=excluded.yt_dlp_path,
             ffmpeg_path=excluded.ffmpeg_path,
             ffprobe_path=excluded.ffprobe_path",
        )
        .bind(capture_enabled)
        .bind(automatic_interception)
        .bind(capture_mode_name(settings.browser_capture_mode))
        .bind(excluded)
        .bind(&settings.yt_dlp_path)
        .bind(&settings.ffmpeg_path)
        .bind(&settings.ffprobe_path)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn browser_preferences(&self) -> Result<AppSettings, StorageError> {
        let Some(row) = sqlx::query("SELECT * FROM browser_preferences WHERE singleton = 1")
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(AppSettings::default());
        };
        let sites: String = row.try_get("excluded_sites_json")?;
        Ok(AppSettings {
            browser_capture_mode: parse_capture_mode(row.try_get("capture_mode")?)?,
            browser_excluded_sites: serde_json::from_str(&sites)
                .map_err(|error| StorageError::InvalidState(error.to_string()))?,
            yt_dlp_path: row.try_get("yt_dlp_path")?,
            ffmpeg_path: row.try_get("ffmpeg_path")?,
            ffprobe_path: row.try_get("ffprobe_path")?,
            ..AppSettings::default()
        })
    }

    /// Atomically reserves a browser idempotency key.
    ///
    /// # Errors
    /// Returns a database error or an invalid persisted UUID error.
    pub async fn reserve_browser_capture(
        &self,
        idempotency_key: Uuid,
        request_id: Uuid,
        browser: &str,
        source_origin: Option<&str>,
        created_at: i64,
    ) -> Result<BrowserCaptureReservation, StorageError> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO browser_captures
             (idempotency_key, request_id, browser, source_origin, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(idempotency_key.to_string())
        .bind(request_id.to_string())
        .bind(browser)
        .bind(source_origin)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 1 {
            return Ok(BrowserCaptureReservation::Reserved);
        }
        let job_id: Option<String> =
            sqlx::query_scalar("SELECT job_id FROM browser_captures WHERE idempotency_key = ?")
                .bind(idempotency_key.to_string())
                .fetch_one(&self.pool)
                .await?;
        Ok(match job_id {
            Some(id) => BrowserCaptureReservation::Existing(Uuid::parse_str(&id)?),
            None => BrowserCaptureReservation::Pending,
        })
    }

    /// Links a reserved capture to its durably inserted queue job.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn commit_browser_capture(
        &self,
        idempotency_key: Uuid,
        job_id: Uuid,
    ) -> Result<(), StorageError> {
        sqlx::query("UPDATE browser_captures SET job_id = ? WHERE idempotency_key = ?")
            .bind(job_id.to_string())
            .bind(idempotency_key.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Removes a failed reservation so a safe browser retry is possible.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn release_browser_capture(&self, idempotency_key: Uuid) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM browser_captures WHERE idempotency_key = ? AND job_id IS NULL")
            .bind(idempotency_key.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Inserts media execution metadata linked by the shared queue identifier.
    ///
    /// # Errors
    /// Returns a serialization or database error.
    pub async fn insert_media_record(
        &self,
        record: &MediaRecord,
        timestamp: i64,
    ) -> Result<(), StorageError> {
        let formats = serde_json::to_string(&record.selected_format_ids)
            .map_err(|error| StorageError::InvalidState(error.to_string()))?;
        sqlx::query(
            "INSERT INTO media_jobs
             (id, source_url, title, website, destination, selected_format_ids_json,
              output_format, state, stage, temporary_path, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?, ?, ?)",
        )
        .bind(record.id.to_string())
        .bind(&record.source_url)
        .bind(&record.title)
        .bind(&record.website)
        .bind(&record.destination)
        .bind(formats)
        .bind(&record.output_format)
        .bind(&record.stage)
        .bind(&record.temporary_path)
        .bind(timestamp)
        .bind(timestamp)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Loads media execution metadata for a shared queue identifier.
    ///
    /// # Errors
    /// Returns an error for database failures or corrupt persisted metadata.
    pub async fn media_record(&self, id: Uuid) -> Result<Option<MediaRecord>, StorageError> {
        let row = sqlx::query("SELECT * FROM media_jobs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| {
            let formats: String = row.try_get("selected_format_ids_json")?;
            Ok(MediaRecord {
                id: Uuid::parse_str(row.try_get("id")?)?,
                source_url: row.try_get("source_url")?,
                title: row.try_get("title")?,
                website: row.try_get("website")?,
                destination: row.try_get("destination")?,
                selected_format_ids: serde_json::from_str(&formats)
                    .map_err(|error| StorageError::InvalidState(error.to_string()))?,
                output_format: row.try_get("output_format")?,
                stage: row.try_get("stage")?,
                temporary_path: row.try_get("temporary_path")?,
            })
        })
        .transpose()
    }

    /// Updates the media-specific stage and mirrors its lifecycle state.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn update_media_stage(
        &self,
        id: Uuid,
        lifecycle_state: DownloadState,
        media_stage: &str,
        error: Option<&str>,
        updated_at: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE media_jobs SET state = ?, stage = ?, error = ?, updated_at = ? WHERE id = ?",
        )
        .bind(state_name(lifecycle_state))
        .bind(media_stage)
        .bind(error)
        .bind(updated_at)
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        self.update_state(id, lifecycle_state, error, updated_at)
            .await
    }

    /// Persists a throttled media progress update in both queue and media metadata.
    ///
    /// # Errors
    /// Returns a database or numeric conversion error.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_media_progress(
        &self,
        id: Uuid,
        lifecycle_state: DownloadState,
        media_stage: &str,
        downloaded: u64,
        total: Option<u64>,
        speed: u64,
        eta: Option<u64>,
        updated_at: i64,
    ) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE download_jobs SET state = ?, downloaded_bytes = ?, total_bytes = ?,
             speed_bytes_per_second = ?, eta_seconds = ?, updated_at = ? WHERE id = ?",
        )
        .bind(state_name(lifecycle_state))
        .bind(to_i64(downloaded)?)
        .bind(optional_i64(total)?)
        .bind(to_i64(speed)?)
        .bind(optional_i64(eta)?)
        .bind(updated_at)
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE media_jobs SET state = ?, stage = ?, downloaded_bytes = ?, total_bytes = ?,
             speed_bytes_per_second = ?, eta_seconds = ?, updated_at = ? WHERE id = ?",
        )
        .bind(state_name(lifecycle_state))
        .bind(media_stage)
        .bind(to_i64(downloaded)?)
        .bind(optional_i64(total)?)
        .bind(to_i64(speed)?)
        .bind(optional_i64(eta)?)
        .bind(updated_at)
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCaptureReservation {
    Reserved,
    Pending,
    Existing(Uuid),
}

fn row_to_job(row: &sqlx::sqlite::SqliteRow) -> Result<DownloadJob, StorageError> {
    Ok(DownloadJob {
        id: Uuid::parse_str(row.try_get("id")?)?,
        url: row.try_get("url")?,
        destination: row.try_get("destination")?,
        file_name: row.try_get("file_name")?,
        kind: parse_kind(row.try_get("job_kind")?)?,
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
        DownloadState::Merging => "merging",
        DownloadState::Converting => "converting",
        DownloadState::Verifying => "verifying",
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
        "merging" => Ok(DownloadState::Merging),
        "converting" => Ok(DownloadState::Converting),
        "verifying" => Ok(DownloadState::Verifying),
        "paused" => Ok(DownloadState::Paused),
        "completed" => Ok(DownloadState::Completed),
        "failed" => Ok(DownloadState::Failed),
        "cancelled" => Ok(DownloadState::Cancelled),
        _ => Err(StorageError::InvalidState(value.to_owned())),
    }
}

fn kind_name(kind: DownloadKind) -> &'static str {
    match kind {
        DownloadKind::File => "file",
        DownloadKind::Media => "media",
    }
}

fn parse_kind(value: &str) -> Result<DownloadKind, StorageError> {
    match value {
        "file" => Ok(DownloadKind::File),
        "media" => Ok(DownloadKind::Media),
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

fn capture_mode_name(mode: BrowserCaptureMode) -> &'static str {
    match mode {
        BrowserCaptureMode::Off => "off",
        BrowserCaptureMode::Ask => "ask",
        BrowserCaptureMode::Automatic => "automatic",
    }
}

fn parse_capture_mode(value: &str) -> Result<BrowserCaptureMode, StorageError> {
    match value {
        "off" => Ok(BrowserCaptureMode::Off),
        "ask" => Ok(BrowserCaptureMode::Ask),
        "automatic" => Ok(BrowserCaptureMode::Automatic),
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
            kind: DownloadKind::File,
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
            browser_capture_mode: BrowserCaptureMode::Off,
            browser_excluded_sites: vec!["example.com".into()],
            yt_dlp_path: "/tools/yt-dlp".into(),
            ffmpeg_path: "/tools/ffmpeg".into(),
            ffprobe_path: "/tools/ffprobe".into(),
        };
        storage
            .save_settings(&settings)
            .await
            .expect("save settings");
        assert_eq!(storage.settings().await.expect("settings"), settings);
    }

    #[tokio::test]
    async fn migrates_legacy_browser_capture_settings_to_one_mode() {
        let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
        sqlx::query("CREATE TABLE _grabbit_migrations (version INTEGER PRIMARY KEY NOT NULL)")
            .execute(&pool)
            .await
            .expect("migration table");
        for (version, sql) in &MIGRATIONS[..2] {
            for statement in sql
                .split(';')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                sqlx::query(statement)
                    .execute(&pool)
                    .await
                    .expect("legacy migration");
            }
            sqlx::query("INSERT INTO _grabbit_migrations(version) VALUES (?)")
                .bind(version)
                .execute(&pool)
                .await
                .expect("migration version");
        }
        sqlx::query(
            "INSERT INTO app_settings
             (singleton, max_concurrent_downloads, default_segments, default_download_directory, theme)
             VALUES (1, 3, 4, '', 'system')",
        )
        .execute(&pool)
        .await
        .expect("application settings");
        sqlx::query(
            "INSERT INTO browser_preferences
             (singleton, capture_enabled, automatic_interception, excluded_sites_json,
              yt_dlp_path, ffmpeg_path, ffprobe_path)
             VALUES (1, 1, 1, '[]', '', '', '')",
        )
        .execute(&pool)
        .await
        .expect("legacy browser settings");

        let storage = DownloadStorage { pool };
        storage.migrate().await.expect("new migration");
        assert_eq!(
            storage
                .settings()
                .await
                .expect("settings")
                .browser_capture_mode,
            BrowserCaptureMode::Automatic
        );
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

    #[tokio::test]
    async fn browser_capture_reservations_are_idempotent() {
        let storage = DownloadStorage::in_memory().await.expect("storage");
        let key = Uuid::new_v4();
        let request = Uuid::new_v4();
        assert_eq!(
            storage
                .reserve_browser_capture(key, request, "firefox", Some("https://example.com"), 1)
                .await
                .expect("reserve"),
            BrowserCaptureReservation::Reserved
        );
        assert_eq!(
            storage
                .reserve_browser_capture(key, request, "firefox", None, 1)
                .await
                .expect("duplicate pending"),
            BrowserCaptureReservation::Pending
        );
        let job = fixture_job();
        storage.insert_job(&job).await.expect("insert");
        storage
            .commit_browser_capture(key, job.id)
            .await
            .expect("commit");
        assert_eq!(
            storage
                .reserve_browser_capture(key, request, "firefox", None, 1)
                .await
                .expect("duplicate committed"),
            BrowserCaptureReservation::Existing(job.id)
        );
    }
}
