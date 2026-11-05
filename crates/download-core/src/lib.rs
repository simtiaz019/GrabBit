//! Application service coordinating transport, durable state, and files.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use download_common::{
    AppSettings, DownloadId, DownloadJob, DownloadKind, DownloadState, ProgressSnapshot,
    RemoteMetadata,
};
use download_storage::{
    BrowserCaptureReservation, DownloadStorage, MediaRecord, SegmentCheckpoint, StorageError,
};
use http_engine::{DownloadRequest, EngineConfig, EngineError, EngineEvent, HttpEngine};
use media_engine::{
    Availability, InspectRequest, MediaDownloadRequest, MediaError, MediaInspector, MediaOutput,
    MediaProgress, MediaStage, ToolConfig, YtDlpAdapter,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    fs,
    io::AsyncReadExt,
    sync::{Mutex, Semaphore, broadcast, mpsc},
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AddDownloadRequest {
    pub url: String,
    pub destination_directory: PathBuf,
    pub segments: u8,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BrowserCaptureRequest {
    pub request_id: Uuid,
    pub idempotency_key: Uuid,
    pub url: String,
    pub browser: String,
    pub source_origin: Option<String>,
    pub automatic: bool,
}

#[derive(Debug, Clone)]
pub struct AddMediaRequest {
    pub url: String,
    pub destination_directory: PathBuf,
    pub format_ids: Vec<String>,
    pub output: MediaOutput,
    pub authorization_confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCaptureResult {
    Created(DownloadId),
    Duplicate(DownloadId),
    Pending,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("download job was not found")]
    NotFound,
    #[error("destination must be an existing directory")]
    InvalidDestination,
    #[error("destination or partial file already exists")]
    DestinationExists,
    #[error("the remote resource changed; partial bytes were not reused")]
    ResourceChanged,
    #[error("SHA-256 integrity check failed")]
    IntegrityMismatch,
    #[error("expected SHA-256 must contain 64 hexadecimal characters")]
    InvalidHash,
    #[error("download cannot transition from {0:?} to {1:?}")]
    InvalidTransition(DownloadState, DownloadState),
    #[error("system clock is before the Unix epoch")]
    Clock,
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error("browser capture is disabled")]
    BrowserCaptureDisabled,
    #[error("this website is excluded from browser capture")]
    BrowserSiteExcluded,
    #[error(
        "signed or credential-bearing browser URLs are not persisted; use the browser's original download"
    )]
    SensitiveBrowserUrl,
}

pub struct DownloadService {
    storage: DownloadStorage,
    engine: HttpEngine,
    active: Mutex<HashMap<DownloadId, CancellationToken>>,
    concurrency: Arc<Semaphore>,
    progress: broadcast::Sender<ProgressSnapshot>,
}

impl std::fmt::Debug for DownloadService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DownloadService")
            .finish_non_exhaustive()
    }
}

impl DownloadService {
    /// Initializes persistent state and reconciles interrupted transfers.
    ///
    /// # Errors
    ///
    /// Returns an error when directories, SQLite, or the HTTP client cannot be initialized.
    pub async fn open(data_directory: &Path) -> Result<Arc<Self>, ServiceError> {
        fs::create_dir_all(data_directory).await?;
        let storage = DownloadStorage::open(&data_directory.join("grabbit.sqlite3")).await?;
        storage.reconcile_interrupted(now()?).await?;
        let settings = storage.settings().await?;
        let engine = HttpEngine::new(EngineConfig::default())?;
        let (progress, _) = broadcast::channel(256);
        Ok(Arc::new(Self {
            storage,
            engine,
            active: Mutex::new(HashMap::new()),
            concurrency: Arc::new(Semaphore::new(usize::from(
                settings.max_concurrent_downloads.clamp(1, 32),
            ))),
            progress,
        }))
    }

    /// Inspects remote metadata without creating a job.
    ///
    /// # Errors
    ///
    /// Returns URL, HTTP, or metadata validation errors.
    pub async fn probe(&self, url: &str) -> Result<RemoteMetadata, ServiceError> {
        Ok(self.engine.probe(url).await?)
    }

    /// Creates, persists, and schedules a download.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe destinations, collisions, invalid hashes, or probe failures.
    pub async fn add(
        self: &Arc<Self>,
        request: AddDownloadRequest,
    ) -> Result<DownloadJob, ServiceError> {
        validate_hash(request.sha256.as_deref())?;
        let destination_directory = if request.destination_directory.as_os_str().is_empty() {
            PathBuf::from(self.storage.settings().await?.default_download_directory)
        } else {
            request.destination_directory.clone()
        };
        let directory_metadata = fs::metadata(&destination_directory)
            .await
            .map_err(|_| ServiceError::InvalidDestination)?;
        if !directory_metadata.is_dir() {
            return Err(ServiceError::InvalidDestination);
        }

        let metadata = self.engine.probe(&request.url).await?;
        let destination = destination_directory.join(&metadata.file_name);
        let partial = partial_path(&destination);
        if fs::try_exists(&destination).await? || fs::try_exists(&partial).await? {
            return Err(ServiceError::DestinationExists);
        }
        let timestamp = now()?;
        let supports_ranges = metadata.supports_ranges;
        let job = DownloadJob {
            id: Uuid::new_v4(),
            url: metadata.final_url.clone(),
            destination: destination.to_string_lossy().into_owned(),
            file_name: metadata.file_name,
            kind: DownloadKind::File,
            state: DownloadState::Queued,
            total_bytes: metadata.content_length,
            downloaded_bytes: 0,
            speed_bytes_per_second: 0,
            eta_seconds: None,
            segments: request.segments.clamp(1, 32),
            etag: metadata.etag,
            last_modified: metadata.last_modified,
            sha256: request.sha256.map(|value| value.to_ascii_lowercase()),
            error: None,
            created_at: timestamp,
            updated_at: timestamp,
        };
        self.storage.insert_job(&job).await?;
        if supports_ranges
            && job.segments > 1
            && let Some(total) = job.total_bytes
        {
            let checkpoints = (0..job.segments)
                .map(|index| SegmentCheckpoint {
                    index,
                    start_byte: total * u64::from(index) / u64::from(job.segments),
                    end_byte: total * (u64::from(index) + 1) / u64::from(job.segments) - 1,
                    downloaded_bytes: 0,
                })
                .collect::<Vec<_>>();
            self.storage.replace_segments(job.id, &checkpoints).await?;
        }
        self.schedule(job.id).await?;
        Ok(job)
    }

    /// Idempotently adds a browser-captured URL through the ordinary durable queue path.
    ///
    /// # Errors
    /// Returns policy, storage, destination, or HTTP validation errors.
    pub async fn capture_browser_download(
        self: &Arc<Self>,
        request: BrowserCaptureRequest,
    ) -> Result<BrowserCaptureResult, ServiceError> {
        let settings = self.storage.settings().await?;
        if !settings.browser_capture_enabled {
            return Err(ServiceError::BrowserCaptureDisabled);
        }
        if request.automatic && !settings.browser_automatic_interception {
            return Err(ServiceError::BrowserCaptureDisabled);
        }
        let parsed_url = url::Url::parse(&request.url).ok();
        if parsed_url.as_ref().is_some_and(|url| {
            url.query_pairs().any(|(name, _)| {
                let name = name.to_ascii_lowercase();
                [
                    "token",
                    "signature",
                    "sig",
                    "policy",
                    "credential",
                    "auth",
                    "authorization",
                    "x-amz-signature",
                    "x-goog-signature",
                ]
                .iter()
                .any(|candidate| name == *candidate || name.ends_with(&format!("_{candidate}")))
            })
        }) {
            return Err(ServiceError::SensitiveBrowserUrl);
        }
        let host = parsed_url.and_then(|url| url.host_str().map(str::to_ascii_lowercase));
        if host.as_ref().is_some_and(|host| {
            settings.browser_excluded_sites.iter().any(|site| {
                let site = site.trim().trim_start_matches("*.").to_ascii_lowercase();
                !site.is_empty() && (host == &site || host.ends_with(&format!(".{site}")))
            })
        }) {
            return Err(ServiceError::BrowserSiteExcluded);
        }
        match self
            .storage
            .reserve_browser_capture(
                request.idempotency_key,
                request.request_id,
                &request.browser,
                request.source_origin.as_deref(),
                now()?,
            )
            .await?
        {
            BrowserCaptureReservation::Existing(id) => {
                return Ok(BrowserCaptureResult::Duplicate(id));
            }
            BrowserCaptureReservation::Pending => return Ok(BrowserCaptureResult::Pending),
            BrowserCaptureReservation::Reserved => {}
        }
        let result = self
            .add(AddDownloadRequest {
                url: request.url,
                destination_directory: PathBuf::from(settings.default_download_directory),
                segments: settings.default_segments,
                sha256: None,
            })
            .await;
        match result {
            Ok(job) => {
                self.storage
                    .commit_browser_capture(request.idempotency_key, job.id)
                    .await?;
                Ok(BrowserCaptureResult::Created(job.id))
            }
            Err(error) => {
                self.storage
                    .release_browser_capture(request.idempotency_key)
                    .await?;
                Err(error)
            }
        }
    }

    /// Re-inspects, persists, and schedules an explicitly authorized media selection.
    ///
    /// # Errors
    /// Returns tool, authorization, format, destination, or storage errors.
    pub async fn add_media(
        self: &Arc<Self>,
        request: AddMediaRequest,
    ) -> Result<DownloadJob, ServiceError> {
        let settings = self.storage.settings().await?;
        let destination_directory = if request.destination_directory.as_os_str().is_empty() {
            PathBuf::from(&settings.default_download_directory)
        } else {
            request.destination_directory
        };
        if !fs::metadata(&destination_directory)
            .await
            .map_err(|_| ServiceError::InvalidDestination)?
            .is_dir()
        {
            return Err(ServiceError::InvalidDestination);
        }
        let adapter = media_adapter(&settings)?;
        let inspection = adapter
            .inspect(
                &InspectRequest {
                    url: request.url.clone(),
                    authorization_confirmed: request.authorization_confirmed,
                },
                CancellationToken::new(),
            )
            .await?;
        if matches!(
            inspection.availability,
            Availability::DrmProtected | Availability::LoginRequired | Availability::Unavailable
        ) {
            return Err(ServiceError::Media(MediaError::ToolFailed(
                "media is protected, restricted, or unavailable; use the website's official download/export option".into(),
            )));
        }
        let selected = request
            .format_ids
            .iter()
            .map(|id| {
                inspection
                    .formats
                    .iter()
                    .find(|format| &format.id == id)
                    .ok_or(MediaError::UnknownFormat)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if selected.is_empty() || selected.len() > 4 {
            return Err(ServiceError::Media(MediaError::UnknownFormat));
        }
        let extension = output_extension(&request.output, &selected)?;
        let file_name = format!("{}.{}", sanitize_media_name(&inspection.title), extension);
        let destination = destination_directory.join(&file_name);
        let temporary = media_partial_path(&destination);
        if fs::try_exists(&destination).await? || fs::try_exists(&temporary).await? {
            return Err(ServiceError::DestinationExists);
        }
        let timestamp = now()?;
        let total_bytes = selected.iter().try_fold(0_u64, |sum, format| {
            format
                .estimated_bytes
                .map(|bytes| sum.saturating_add(bytes))
        });
        let job = DownloadJob {
            id: Uuid::new_v4(),
            url: inspection.source_url.clone(),
            destination: destination.to_string_lossy().into_owned(),
            file_name,
            kind: DownloadKind::Media,
            state: DownloadState::Queued,
            total_bytes,
            downloaded_bytes: 0,
            speed_bytes_per_second: 0,
            eta_seconds: None,
            segments: 1,
            etag: None,
            last_modified: None,
            sha256: None,
            error: None,
            created_at: timestamp,
            updated_at: timestamp,
        };
        self.storage.insert_job(&job).await?;
        let record = MediaRecord {
            id: job.id,
            source_url: inspection.source_url,
            title: inspection.title,
            website: inspection.website,
            destination: job.destination.clone(),
            selected_format_ids: request.format_ids,
            output_format: output_name(&request.output).into(),
            stage: "queued".into(),
            temporary_path: temporary.to_string_lossy().into_owned(),
        };
        if let Err(error) = self.storage.insert_media_record(&record, timestamp).await {
            self.storage.delete_job(job.id).await?;
            return Err(ServiceError::Storage(error));
        }
        self.schedule_media(job.id).await?;
        Ok(job)
    }

    /// Returns persistent queue and history entries.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot be read.
    pub async fn list(&self) -> Result<Vec<DownloadJob>, ServiceError> {
        Ok(self.storage.list_jobs().await?)
    }

    /// Pauses an active or queued job.
    ///
    /// # Errors
    ///
    /// Returns an error when the job is missing or has an invalid state.
    pub async fn pause(&self, id: DownloadId) -> Result<(), ServiceError> {
        let job = self
            .storage
            .get_job(id)
            .await?
            .ok_or(ServiceError::NotFound)?;
        if !matches!(
            job.state,
            DownloadState::Downloading
                | DownloadState::Merging
                | DownloadState::Converting
                | DownloadState::Verifying
                | DownloadState::Queued
        ) {
            return Err(ServiceError::InvalidTransition(
                job.state,
                DownloadState::Paused,
            ));
        }
        let token = self.active.lock().await.get(&id).cloned();
        if let Some(token) = token {
            token.cancel();
            while self.active.lock().await.contains_key(&id) {
                sleep(Duration::from_millis(10)).await;
            }
        }
        let timestamp = now()?;
        if job.kind == DownloadKind::Media {
            self.storage
                .update_media_stage(id, DownloadState::Paused, "paused", None, timestamp)
                .await?;
        } else {
            self.storage
                .update_state(id, DownloadState::Paused, None, timestamp)
                .await?;
        }
        let paused = self
            .storage
            .get_job(id)
            .await?
            .ok_or(ServiceError::NotFound)?;
        let _unused = self.progress.send(ProgressSnapshot {
            id,
            state: DownloadState::Paused,
            downloaded_bytes: paused.downloaded_bytes,
            total_bytes: paused.total_bytes,
            speed_bytes_per_second: 0,
            eta_seconds: None,
        });
        Ok(())
    }

    /// Resumes a paused job after source revalidation.
    ///
    /// # Errors
    ///
    /// Returns an error when the job is missing or the resource changed.
    pub async fn resume(self: &Arc<Self>, id: DownloadId) -> Result<(), ServiceError> {
        let job = self
            .storage
            .get_job(id)
            .await?
            .ok_or(ServiceError::NotFound)?;
        if !matches!(job.state, DownloadState::Paused | DownloadState::Failed) {
            return Err(ServiceError::InvalidTransition(
                job.state,
                DownloadState::Queued,
            ));
        }
        if job.kind == DownloadKind::Media {
            let record = self
                .storage
                .media_record(id)
                .await?
                .ok_or(ServiceError::NotFound)?;
            if let Err(error) = fs::remove_file(&record.temporary_path).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                return Err(ServiceError::Io(error));
            }
            let timestamp = now()?;
            self.storage
                .update_progress(id, 0, 0, None, timestamp)
                .await?;
            self.storage
                .update_media_stage(id, DownloadState::Queued, "queued", None, timestamp)
                .await?;
            return self.schedule_media(id).await;
        }
        let metadata = self.engine.probe(&job.url).await?;
        ensure_same_resource(&job, &metadata)?;
        self.storage
            .update_state(id, DownloadState::Queued, None, now()?)
            .await?;
        self.schedule(id).await
    }

    /// Retries a failed job.
    ///
    /// # Errors
    ///
    /// Returns an error when resume validation fails.
    pub async fn retry(self: &Arc<Self>, id: DownloadId) -> Result<(), ServiceError> {
        self.resume(id).await
    }

    /// Cancels a job without deleting its history record.
    ///
    /// # Errors
    ///
    /// Returns an error when the job is missing or already terminal.
    pub async fn cancel(&self, id: DownloadId) -> Result<(), ServiceError> {
        let job = self
            .storage
            .get_job(id)
            .await?
            .ok_or(ServiceError::NotFound)?;
        if matches!(
            job.state,
            DownloadState::Completed | DownloadState::Cancelled
        ) {
            return Err(ServiceError::InvalidTransition(
                job.state,
                DownloadState::Cancelled,
            ));
        }
        let timestamp = now()?;
        if job.kind == DownloadKind::Media {
            self.storage
                .update_media_stage(id, DownloadState::Cancelled, "cancelled", None, timestamp)
                .await?;
        } else {
            self.storage
                .update_state(id, DownloadState::Cancelled, None, timestamp)
                .await?;
        }
        if let Some(token) = self.active.lock().await.get(&id) {
            token.cancel();
        }
        Ok(())
    }

    /// Removes a non-active history item and any partial file.
    ///
    /// # Errors
    ///
    /// Returns an error for active jobs, missing jobs, or filesystem/database failures.
    pub async fn remove(&self, id: DownloadId) -> Result<(), ServiceError> {
        let job = self
            .storage
            .get_job(id)
            .await?
            .ok_or(ServiceError::NotFound)?;
        if matches!(
            job.state,
            DownloadState::Downloading
                | DownloadState::Merging
                | DownloadState::Converting
                | DownloadState::Verifying
                | DownloadState::Queued
        ) {
            return Err(ServiceError::InvalidTransition(
                job.state,
                DownloadState::Cancelled,
            ));
        }
        let partial = if job.kind == DownloadKind::Media {
            self.storage.media_record(id).await?.map_or_else(
                || partial_path(Path::new(&job.destination)),
                |record| PathBuf::from(record.temporary_path),
            )
        } else {
            partial_path(Path::new(&job.destination))
        };
        if let Err(error) = fs::remove_file(partial).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(ServiceError::Io(error));
        }
        self.storage.delete_job(id).await?;
        Ok(())
    }

    /// Gets persisted settings.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn settings(&self) -> Result<AppSettings, ServiceError> {
        Ok(self.storage.settings().await?)
    }

    /// Saves settings for future scheduling and UI sessions.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn save_settings(&self, mut settings: AppSettings) -> Result<(), ServiceError> {
        settings.max_concurrent_downloads = settings.max_concurrent_downloads.clamp(1, 32);
        settings.default_segments = settings.default_segments.clamp(1, 32);
        self.storage.save_settings(&settings).await?;
        Ok(())
    }

    #[must_use]
    pub fn subscribe_progress(&self) -> broadcast::Receiver<ProgressSnapshot> {
        self.progress.subscribe()
    }

    /// Cancels active transfers and waits briefly for durable checkpoint and
    /// child-process cleanup during application shutdown.
    pub async fn shutdown(&self) {
        let tokens = self
            .active
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for token in tokens {
            token.cancel();
        }
        let _ = tokio::time::timeout(Duration::from_secs(8), async {
            while !self.active.lock().await.is_empty() {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
    }

    async fn schedule(self: &Arc<Self>, id: DownloadId) -> Result<(), ServiceError> {
        let token = CancellationToken::new();
        self.active.lock().await.insert(id, token.clone());
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = service.run_job(id, token).await {
                tracing::error!(job_id = %id, error = %error, "download failed");
                if let Ok(Some(job)) = service.storage.get_job(id).await
                    && !matches!(job.state, DownloadState::Paused | DownloadState::Cancelled)
                {
                    let _unused = service
                        .storage
                        .update_state(
                            id,
                            DownloadState::Failed,
                            Some(&error.to_string()),
                            now().unwrap_or(job.updated_at),
                        )
                        .await;
                }
            }
            service.active.lock().await.remove(&id);
        });
        Ok(())
    }

    async fn schedule_media(self: &Arc<Self>, id: DownloadId) -> Result<(), ServiceError> {
        let token = CancellationToken::new();
        self.active.lock().await.insert(id, token.clone());
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(error) = service.run_media_job(id, token).await {
                tracing::error!(job_id = %id, error = %error, "media download failed");
                if let Ok(Some(job)) = service.storage.get_job(id).await
                    && !matches!(job.state, DownloadState::Paused | DownloadState::Cancelled)
                {
                    let timestamp = now().unwrap_or(job.updated_at);
                    let _ = service
                        .storage
                        .update_media_stage(
                            id,
                            DownloadState::Failed,
                            "failed",
                            Some(&error.to_string()),
                            timestamp,
                        )
                        .await;
                }
            }
            service.active.lock().await.remove(&id);
        });
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn run_media_job(
        &self,
        id: DownloadId,
        token: CancellationToken,
    ) -> Result<(), ServiceError> {
        let _permit = tokio::select! {
            () = token.cancelled() => return Ok(()),
            permit = self.concurrency.acquire() => permit.map_err(|_| ServiceError::NotFound)?,
        };
        let job = self
            .storage
            .get_job(id)
            .await?
            .ok_or(ServiceError::NotFound)?;
        if job.state != DownloadState::Queued {
            return Ok(());
        }
        let record = self
            .storage
            .media_record(id)
            .await?
            .ok_or(ServiceError::NotFound)?;
        self.storage
            .update_media_stage(id, DownloadState::Downloading, "downloading", None, now()?)
            .await?;
        let _ = self.progress.send(ProgressSnapshot {
            id,
            state: DownloadState::Downloading,
            downloaded_bytes: job.downloaded_bytes,
            total_bytes: job.total_bytes,
            speed_bytes_per_second: 0,
            eta_seconds: None,
        });
        let settings = self.storage.settings().await?;
        let adapter = media_adapter(&settings)?;
        let processing_state = if record.output_format == "original" {
            DownloadState::Downloading
        } else if matches!(record.output_format.as_str(), "mp4" | "webm" | "mkv") {
            DownloadState::Merging
        } else {
            DownloadState::Converting
        };
        let (media_events, mut media_receiver) = mpsc::channel::<MediaProgress>(16);
        let progress_storage = self.storage.clone();
        let progress_sender = self.progress.clone();
        let initial_total = job.total_bytes;
        let progress_task = tokio::spawn(async move {
            let mut downloaded = 0_u64;
            while let Some(event) = media_receiver.recv().await {
                downloaded = event.downloaded_bytes.unwrap_or(downloaded);
                let total = event.total_bytes.or(initial_total);
                let state = if event.stage == MediaStage::PostProcessing {
                    processing_state
                } else {
                    DownloadState::Downloading
                };
                let progress_stage = state_stage(state);
                let speed = event.speed_bytes_per_second.unwrap_or(0);
                if let Ok(timestamp) = now()
                    && progress_storage
                        .update_media_progress(
                            id,
                            state,
                            progress_stage,
                            downloaded,
                            total,
                            speed,
                            event.eta_seconds,
                            timestamp,
                        )
                        .await
                        .is_ok()
                {
                    let _ = progress_sender.send(ProgressSnapshot {
                        id,
                        state,
                        downloaded_bytes: downloaded,
                        total_bytes: total,
                        speed_bytes_per_second: speed,
                        eta_seconds: event.eta_seconds,
                    });
                }
            }
        });
        let download_result = adapter
            .download(
                &MediaDownloadRequest {
                    url: record.source_url,
                    format_ids: record.selected_format_ids,
                    output: parse_output(&record.output_format)?,
                    output_path: PathBuf::from(&record.temporary_path),
                    authorization_confirmed: true,
                },
                token.clone(),
                Some(media_events),
            )
            .await;
        progress_task
            .await
            .map_err(|error| ServiceError::Io(std::io::Error::other(error)))?;
        download_result?;
        if token.is_cancelled() {
            return Ok(());
        }
        self.storage
            .update_media_stage(id, DownloadState::Verifying, "verifying", None, now()?)
            .await?;
        let _ = self.progress.send(ProgressSnapshot {
            id,
            state: DownloadState::Verifying,
            downloaded_bytes: job.downloaded_bytes,
            total_bytes: job.total_bytes,
            speed_bytes_per_second: 0,
            eta_seconds: None,
        });
        let temporary = PathBuf::from(record.temporary_path);
        let file = fs::OpenOptions::new().read(true).open(&temporary).await?;
        file.sync_all().await?;
        let bytes = file.metadata().await?.len();
        finalize_without_overwrite(&temporary, Path::new(&record.destination)).await?;
        self.storage
            .update_progress(id, bytes, 0, Some(0), now()?)
            .await?;
        self.storage
            .update_media_stage(id, DownloadState::Completed, "completed", None, now()?)
            .await?;
        let _ = self.progress.send(ProgressSnapshot {
            id,
            state: DownloadState::Completed,
            downloaded_bytes: bytes,
            total_bytes: Some(bytes),
            speed_bytes_per_second: 0,
            eta_seconds: Some(0),
        });
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn run_job(&self, id: DownloadId, token: CancellationToken) -> Result<(), ServiceError> {
        let _permit = tokio::select! {
            () = token.cancelled() => return Ok(()),
            permit = self.concurrency.acquire() => permit.map_err(|_| ServiceError::NotFound)?,
        };
        if token.is_cancelled() {
            return Ok(());
        }
        let job = self
            .storage
            .get_job(id)
            .await?
            .ok_or(ServiceError::NotFound)?;
        if job.state != DownloadState::Queued {
            return Ok(());
        }
        self.storage
            .update_state(id, DownloadState::Downloading, None, now()?)
            .await?;

        let metadata = self.engine.probe(&job.url).await?;
        ensure_same_resource(&job, &metadata)?;
        let destination = PathBuf::from(&job.destination);
        let partial = partial_path(&destination);
        let (sequential_resume, partial_exists) = match fs::metadata(&partial).await {
            Ok(value) if metadata.supports_ranges => (value.len().min(job.downloaded_bytes), true),
            Ok(_) => (0, true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (0, false),
            Err(error) => return Err(ServiceError::Io(error)),
        };
        let mut checkpoints = self.storage.segments(id).await?;
        if !partial_exists && !checkpoints.is_empty() {
            for checkpoint in &mut checkpoints {
                checkpoint.downloaded_bytes = 0;
            }
            self.storage.replace_segments(id, &checkpoints).await?;
        }
        let resume_from = if checkpoints.is_empty() {
            sequential_resume
        } else {
            0
        };
        let segment_offsets = checkpoints
            .iter()
            .map(|checkpoint| checkpoint.downloaded_bytes)
            .collect::<Vec<_>>();

        let (events, mut receiver) = mpsc::unbounded_channel();
        let progress_sender = self.progress.clone();
        let progress_storage = self.storage.clone();
        let total = job.total_bytes;
        let base = if checkpoints.is_empty() {
            resume_from
        } else {
            segment_offsets.iter().copied().sum()
        };
        let progress_task = tokio::spawn(async move {
            let started = Instant::now();
            let mut downloaded = base;
            while let Some(event) = receiver.recv().await {
                match event {
                    EngineEvent::BytesWritten(bytes) => {
                        downloaded = downloaded.saturating_add(bytes);
                        let elapsed_ms = u64::try_from(started.elapsed().as_millis())
                            .unwrap_or(u64::MAX)
                            .max(1);
                        let speed =
                            downloaded.saturating_sub(base).saturating_mul(1_000) / elapsed_ms;
                        let eta = total.and_then(|length| {
                            (speed > 0 && downloaded < length)
                                .then(|| (length - downloaded) / speed)
                        });
                        let _unused = progress_sender.send(ProgressSnapshot {
                            id,
                            state: DownloadState::Downloading,
                            downloaded_bytes: downloaded,
                            total_bytes: total,
                            speed_bytes_per_second: speed,
                            eta_seconds: eta,
                        });
                    }
                    EngineEvent::SegmentCheckpoint { index, downloaded } => {
                        if let Err(error) = progress_storage
                            .checkpoint_segment(id, index, downloaded)
                            .await
                        {
                            tracing::error!(job_id = %id, %error, "failed to persist segment checkpoint");
                        }
                    }
                }
            }
        });

        let result = self
            .engine
            .download(
                DownloadRequest {
                    url: &job.url,
                    path: &partial,
                    metadata: &metadata,
                    segments: job.segments,
                    resume_from,
                    segment_offsets: &segment_offsets,
                },
                token,
                Some(events),
            )
            .await;
        progress_task
            .await
            .map_err(|error| ServiceError::Io(std::io::Error::other(error)))?;

        match result {
            Ok(_) => {
                verify_integrity(&partial, job.sha256.as_deref()).await?;
                finalize_without_overwrite(&partial, &destination).await?;
                let completed = metadata.content_length.unwrap_or_else(|| {
                    std::fs::metadata(&destination).map_or(0, |value| value.len())
                });
                self.storage
                    .update_progress(id, completed, 0, None, now()?)
                    .await?;
                self.storage
                    .update_state(id, DownloadState::Completed, None, now()?)
                    .await?;
                let _unused = self.progress.send(ProgressSnapshot {
                    id,
                    state: DownloadState::Completed,
                    downloaded_bytes: completed,
                    total_bytes: metadata.content_length,
                    speed_bytes_per_second: 0,
                    eta_seconds: Some(0),
                });
                Ok(())
            }
            Err(EngineError::Cancelled) => {
                if job.segments > 1 && !checkpoints.is_empty() {
                    let durable = self.storage.checkpointed_bytes(id).await?;
                    self.storage
                        .update_progress(id, durable, 0, None, now()?)
                        .await?;
                } else if let Ok(metadata) = fs::metadata(&partial).await {
                    self.storage
                        .update_progress(id, metadata.len(), 0, None, now()?)
                        .await?;
                }
                Ok(())
            }
            Err(error) => Err(ServiceError::Engine(error)),
        }
    }
}

fn media_adapter(settings: &AppSettings) -> Result<YtDlpAdapter, ServiceError> {
    Ok(YtDlpAdapter::new(ToolConfig::from_paths(
        PathBuf::from(&settings.yt_dlp_path),
        PathBuf::from(&settings.ffmpeg_path),
        PathBuf::from(&settings.ffprobe_path),
    ))?)
}

fn output_name(output: &MediaOutput) -> &'static str {
    match output {
        MediaOutput::Original => "original",
        MediaOutput::Mp4 => "mp4",
        MediaOutput::WebM => "webm",
        MediaOutput::Mkv => "mkv",
        MediaOutput::Mp3 => "mp3",
        MediaOutput::M4a => "m4a",
        MediaOutput::Aac => "aac",
        MediaOutput::Opus => "opus",
        MediaOutput::Flac => "flac",
        MediaOutput::Wav => "wav",
    }
}

fn parse_output(value: &str) -> Result<MediaOutput, ServiceError> {
    Ok(match value {
        "original" => MediaOutput::Original,
        "mp4" => MediaOutput::Mp4,
        "webm" => MediaOutput::WebM,
        "mkv" => MediaOutput::Mkv,
        "mp3" => MediaOutput::Mp3,
        "m4a" => MediaOutput::M4a,
        "aac" => MediaOutput::Aac,
        "opus" => MediaOutput::Opus,
        "flac" => MediaOutput::Flac,
        "wav" => MediaOutput::Wav,
        _ => return Err(ServiceError::Media(MediaError::UnknownFormat)),
    })
}

fn output_extension(
    output: &MediaOutput,
    selected: &[&media_engine::MediaFormat],
) -> Result<String, ServiceError> {
    if *output == MediaOutput::Original {
        selected
            .first()
            .and_then(|format| format.extension.clone())
            .ok_or(ServiceError::Media(MediaError::UnknownFormat))
    } else {
        Ok(output_name(output).to_owned())
    }
}

fn sanitize_media_name(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let trimmed = cleaned.trim().trim_matches('.');
    let result = trimmed.chars().take(180).collect::<String>();
    if result.is_empty() {
        "media".into()
    } else {
        result
    }
}

fn media_partial_path(destination: &Path) -> PathBuf {
    let stem = destination
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("media");
    let extension = destination
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("bin");
    destination.with_file_name(format!("{stem}.grabbit-media-part.{extension}"))
}

fn state_stage(state: DownloadState) -> &'static str {
    match state {
        DownloadState::Downloading => "downloading",
        DownloadState::Merging => "merging",
        DownloadState::Converting => "converting",
        DownloadState::Verifying => "verifying",
        _ => "processing",
    }
}

fn ensure_same_resource(job: &DownloadJob, metadata: &RemoteMetadata) -> Result<(), ServiceError> {
    if let (Some(expected), Some(actual)) = (job.etag.as_deref(), metadata.etag.as_deref())
        && expected != actual
    {
        return Err(ServiceError::ResourceChanged);
    }
    if job.etag.is_none()
        && let (Some(expected), Some(actual)) = (
            job.last_modified.as_deref(),
            metadata.last_modified.as_deref(),
        )
        && expected != actual
    {
        return Err(ServiceError::ResourceChanged);
    }
    if let (Some(expected), Some(actual)) = (job.total_bytes, metadata.content_length)
        && expected != actual
    {
        return Err(ServiceError::ResourceChanged);
    }
    Ok(())
}

fn partial_path(destination: &Path) -> PathBuf {
    let mut name = destination
        .file_name()
        .map_or_else(|| "download".into(), std::ffi::OsStr::to_os_string);
    name.push(".grabbit.part");
    destination.with_file_name(name)
}

async fn verify_integrity(path: &Path, expected: Option<&str>) -> Result<(), ServiceError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(ServiceError::IntegrityMismatch)
    }
}

async fn finalize_without_overwrite(
    partial: &Path,
    destination: &Path,
) -> Result<(), ServiceError> {
    match fs::hard_link(partial, destination).await {
        Ok(()) => {
            fs::remove_file(partial).await?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(ServiceError::DestinationExists)
        }
        Err(error) => Err(ServiceError::Io(error)),
    }
}

fn validate_hash(value: Option<&str>) -> Result<(), ServiceError> {
    if value.is_some_and(|hash| {
        hash.len() != 64 || !hash.chars().all(|character| character.is_ascii_hexdigit())
    }) {
        Err(ServiceError::InvalidHash)
    } else {
        Ok(())
    }
}

fn now() -> Result<i64, ServiceError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ServiceError::Clock)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| ServiceError::Clock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn integrity_and_collision_safe_finalization() {
        let directory = tempdir().expect("temporary directory");
        let partial = directory.path().join("file.part");
        let destination = directory.path().join("file");
        fs::write(&partial, b"hello").await.expect("write");
        verify_integrity(
            &partial,
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"),
        )
        .await
        .expect("hash");
        finalize_without_overwrite(&partial, &destination)
            .await
            .expect("finalize");
        assert_eq!(fs::read(&destination).await.expect("read"), b"hello");

        fs::write(&partial, b"new").await.expect("write partial");
        assert!(matches!(
            finalize_without_overwrite(&partial, &destination).await,
            Err(ServiceError::DestinationExists)
        ));
        assert_eq!(fs::read(&destination).await.expect("read"), b"hello");
    }

    #[test]
    fn validates_expected_hashes() {
        assert!(validate_hash(None).is_ok());
        assert!(validate_hash(Some(&"a".repeat(64))).is_ok());
        assert!(validate_hash(Some("not-a-hash")).is_err());
    }
}
