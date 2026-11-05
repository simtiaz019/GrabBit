#[cfg(unix)]
mod unix {
    use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};

    use browser_native_host::{MAX_MESSAGE_BYTES, RequestEnvelope, validate_request};
    use download_core::DownloadService;
    use tauri::AppHandle;
    use tokio::{
        fs,
        io::{AsyncReadExt, AsyncWriteExt},
        net::{UnixListener, UnixStream},
    };

    pub async fn serve(service: Arc<DownloadService>, data_directory: &Path, app: AppHandle) {
        let endpoint = data_directory.join("browser-ipc.sock");
        if fs::try_exists(&endpoint).await.unwrap_or(false) {
            if UnixStream::connect(&endpoint).await.is_ok() {
                tracing::warn!("browser IPC endpoint is already active");
                return;
            }
            if let Err(error) = fs::remove_file(&endpoint).await {
                tracing::error!(%error, "failed to remove stale browser IPC endpoint");
                return;
            }
        }
        let listener = match UnixListener::bind(&endpoint) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!(%error, "failed to bind browser IPC endpoint");
                return;
            }
        };
        if let Err(error) =
            fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o600)).await
        {
            tracing::error!(%error, "failed to secure browser IPC endpoint");
            return;
        }
        #[cfg(target_os = "macos")]
        tokio::spawn(super::serve_safari_app_group(
            Arc::clone(&service),
            app.clone(),
        ));
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let service = Arc::clone(&service);
                    let app = app.clone();
                    tokio::spawn(async move {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(8),
                            handle(stream, service, app),
                        )
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                super::record_error("browser IPC request rejected");
                                tracing::warn!(%error, "browser IPC request rejected");
                            }
                            Err(_) => {
                                super::record_error("browser IPC request timed out");
                                tracing::warn!("browser IPC request timed out");
                            }
                        }
                    });
                }
                Err(error) => tracing::warn!(%error, "browser IPC accept failed"),
            }
        }
    }

    async fn handle(
        mut stream: UnixStream,
        service: Arc<DownloadService>,
        app: AppHandle,
    ) -> Result<(), String> {
        let mut header = [0_u8; 4];
        stream
            .read_exact(&mut header)
            .await
            .map_err(|error| error.to_string())?;
        let length = u32::from_le_bytes(header) as usize;
        if length > MAX_MESSAGE_BYTES {
            return Err("message exceeds limit".into());
        }
        let mut bytes = vec![0; length];
        stream
            .read_exact(&mut bytes)
            .await
            .map_err(|error| error.to_string())?;
        let request: RequestEnvelope =
            serde_json::from_slice(&bytes).map_err(|_| "invalid request schema".to_owned())?;
        validate_request(&request).map_err(|error| error.to_string())?;
        let response = super::dispatch_common(request, service, &app).await;
        let bytes = serde_json::to_vec(&response).map_err(|error| error.to_string())?;
        let length = u32::try_from(bytes.len()).map_err(|_| "response exceeds limit".to_owned())?;
        stream
            .write_all(&length.to_le_bytes())
            .await
            .map_err(|error| error.to_string())?;
        stream
            .write_all(&bytes)
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use browser_native_host::{
    BrowserCaptureMode, BrowserRequest, CaptureDiagnosticsReport, RequestEnvelope, ResponseCode,
    ResponseEnvelope, SCHEMA_VERSION,
};
use download_core::{BrowserCaptureRequest, BrowserCaptureResult, DownloadService};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDiagnostics {
    pub platform: &'static str,
    pub ipc_available: bool,
    pub native_host_registered: bool,
    pub manifest_locations: Vec<String>,
    pub extension_connected: bool,
    pub last_connection_at: Option<u64>,
    pub last_successful_ping_at: Option<u64>,
    pub last_error: Option<String>,
    pub automatic_interception_enabled: Option<bool>,
    pub downloads_permission_granted: Option<bool>,
    pub last_download_event_at: Option<u64>,
    pub last_url_hostname: Option<String>,
    pub capture_decision: Option<String>,
    pub job_acceptance_status: Option<String>,
    pub chrome_cancellation_result: Option<String>,
    pub last_download_job_id: Option<String>,
    pub last_failure_category: Option<String>,
}

#[derive(Debug, Default)]
struct RuntimeStatus {
    last_connection_at: Option<u64>,
    last_successful_ping_at: Option<u64>,
    last_error: Option<String>,
    automatic_interception_enabled: Option<bool>,
    downloads_permission_granted: Option<bool>,
    last_download_event_at: Option<u64>,
    last_url_hostname: Option<String>,
    capture_decision: Option<String>,
    job_acceptance_status: Option<String>,
    chrome_cancellation_result: Option<String>,
    last_download_job_id: Option<String>,
    last_failure_category: Option<String>,
}

static RUNTIME_STATUS: LazyLock<Mutex<RuntimeStatus>> =
    LazyLock::new(|| Mutex::new(RuntimeStatus::default()));

fn record_connection() {
    if let Ok(mut status) = RUNTIME_STATUS.lock() {
        status.last_connection_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs());
        status.last_error = None;
    }
}

fn record_ping() {
    if let Ok(mut status) = RUNTIME_STATUS.lock() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs());
        status.last_connection_at = now;
        status.last_successful_ping_at = now;
        status.last_error = None;
    }
}

fn record_capture_result(job_id: Option<uuid::Uuid>, failure_category: Option<&str>) {
    if let Ok(mut status) = RUNTIME_STATUS.lock() {
        status.last_download_job_id = job_id.map(|id| id.to_string());
        status.last_failure_category = failure_category.map(ToOwned::to_owned);
    }
}

fn record_error(message: &str) {
    if let Ok(mut status) = RUNTIME_STATUS.lock() {
        status.last_error = Some(message.chars().take(240).collect());
    }
}

fn record_capture_diagnostics(report: CaptureDiagnosticsReport) {
    if let Ok(mut status) = RUNTIME_STATUS.lock() {
        status.last_connection_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs());
        status.automatic_interception_enabled = Some(report.automatic_enabled);
        status.downloads_permission_granted = Some(report.downloads_permission_granted);
        status.last_download_event_at = report.last_event_at;
        status.last_url_hostname = report.last_hostname;
        status.capture_decision = Some(report.decision);
        status.job_acceptance_status = Some(report.job_acceptance);
        status.chrome_cancellation_result = Some(report.cancellation_result);
        status.last_error = report.last_error;
    }
}

pub fn diagnostics(data_directory: &Path) -> BrowserDiagnostics {
    let locations = native_manifest_locations();
    let manifest_locations = locations
        .iter()
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    #[cfg(unix)]
    let ipc_available = data_directory.join("browser-ipc.sock").exists();
    #[cfg(windows)]
    let ipc_available = data_directory.join("browser-ipc.endpoint").exists();
    #[cfg(not(any(unix, windows)))]
    let ipc_available = false;
    let status = RUNTIME_STATUS.lock().ok();
    let last_connection_at = status.as_ref().and_then(|value| value.last_connection_at);
    let last_error = status.as_ref().map_or_else(
        || Some("diagnostics state unavailable".into()),
        |value| value.last_error.clone(),
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs());
    let extension_connected = last_connection_at
        .zip(now)
        .is_some_and(|(last, current)| current.saturating_sub(last) <= 120);
    let native_host_registered =
        crate::browser_integration::setup_report(&data_directory.join("browser-extensions"))
            .browsers
            .iter()
            .any(|browser| browser.native_host_registered);
    BrowserDiagnostics {
        platform: std::env::consts::OS,
        ipc_available,
        native_host_registered,
        manifest_locations,
        extension_connected,
        last_connection_at,
        last_successful_ping_at: status
            .as_ref()
            .and_then(|value| value.last_successful_ping_at),
        last_error,
        automatic_interception_enabled: status
            .as_ref()
            .and_then(|value| value.automatic_interception_enabled),
        downloads_permission_granted: status
            .as_ref()
            .and_then(|value| value.downloads_permission_granted),
        last_download_event_at: status
            .as_ref()
            .and_then(|value| value.last_download_event_at),
        last_url_hostname: status
            .as_ref()
            .and_then(|value| value.last_url_hostname.clone()),
        capture_decision: status
            .as_ref()
            .and_then(|value| value.capture_decision.clone()),
        job_acceptance_status: status
            .as_ref()
            .and_then(|value| value.job_acceptance_status.clone()),
        chrome_cancellation_result: status
            .as_ref()
            .and_then(|value| value.chrome_cancellation_result.clone()),
        last_download_job_id: status
            .as_ref()
            .and_then(|value| value.last_download_job_id.clone()),
        last_failure_category: status
            .as_ref()
            .and_then(|value| value.last_failure_category.clone()),
    }
}

fn native_manifest_locations() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        return [
            "Library/Application Support/Google/Chrome/NativeMessagingHosts/com.grabbit.native.json",
            "Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts/com.grabbit.native.json",
            "Library/Application Support/Microsoft Edge/NativeMessagingHosts/com.grabbit.native.json",
            "Library/Application Support/Mozilla/NativeMessagingHosts/com.grabbit.native.json",
        ]
        .iter()
        .map(|path| base.join(path))
        .collect();
    }
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        return [
            config.join("google-chrome/NativeMessagingHosts/com.grabbit.native.json"),
            config.join("BraveSoftware/Brave-Browser/NativeMessagingHosts/com.grabbit.native.json"),
            config.join("microsoft-edge/NativeMessagingHosts/com.grabbit.native.json"),
            home.join(".mozilla/native-messaging-hosts/com.grabbit.native.json"),
        ]
        .to_vec();
    }
    #[cfg(windows)]
    {
        return std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|root| {
                vec![
                    root.join("GrabBit/com.grabbit.native.chromium.json"),
                    root.join("GrabBit/com.grabbit.native.firefox.json"),
                ]
            })
            .unwrap_or_default();
    }
    #[allow(unreachable_code)]
    Vec::new()
}

#[cfg(target_os = "macos")]
async fn serve_safari_app_group(service: Arc<DownloadService>, app: AppHandle) {
    use tokio::time::{Duration, sleep};
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let bridge = home.join("Library/Group Containers/group.com.grabbit.desktop/GrabBitBridge");
    if tokio::fs::create_dir_all(&bridge).await.is_err() {
        tracing::info!("Safari app-group bridge unavailable in this unsigned build");
        return;
    }
    loop {
        let Ok(mut entries) = tokio::fs::read_dir(&bridge).await else {
            sleep(Duration::from_millis(250)).await;
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("request-")
                || entry.path().extension() != Some(std::ffi::OsStr::new("json"))
            {
                continue;
            }
            let processing = bridge.join(name.replace("request-", "processing-"));
            if tokio::fs::rename(entry.path(), &processing).await.is_err() {
                continue;
            }
            let response = match tokio::fs::read(&processing).await {
                Ok(bytes) if bytes.len() <= browser_native_host::MAX_MESSAGE_BYTES => {
                    match serde_json::from_slice::<RequestEnvelope>(&bytes) {
                        Ok(request) => match browser_native_host::validate_request(&request) {
                            Ok(()) => dispatch_common(request, Arc::clone(&service), &app).await,
                            Err(error) => ResponseEnvelope::error(
                                None,
                                ResponseCode::InvalidRequest,
                                error.to_string(),
                            ),
                        },
                        Err(_) => ResponseEnvelope::error(
                            None,
                            ResponseCode::InvalidRequest,
                            "invalid Safari request schema",
                        ),
                    }
                }
                _ => ResponseEnvelope::error(
                    None,
                    ResponseCode::InvalidRequest,
                    "Safari request exceeded the size limit",
                ),
            };
            let id = name
                .trim_start_matches("request-")
                .trim_end_matches(".json");
            let temporary_reply = bridge.join(format!("reply-{id}.tmp"));
            let final_reply = bridge.join(format!("reply-{id}.json"));
            if let Ok(bytes) = serde_json::to_vec(&response)
                && tokio::fs::write(&temporary_reply, bytes).await.is_ok()
            {
                let _ = tokio::fs::rename(&temporary_reply, &final_reply).await;
            }
            let _ = tokio::fs::remove_file(&processing).await;
        }
        sleep(Duration::from_millis(150)).await;
    }
}

#[allow(clippy::too_many_lines)]
async fn dispatch_common(
    request: RequestEnvelope,
    service: Arc<DownloadService>,
    app: &AppHandle,
) -> ResponseEnvelope {
    let request_id = request.request_id;
    match request.message {
        BrowserRequest::Ping => {
            record_ping();
            success(
                request_id,
                ResponseCode::Pong,
                "GrabBit is connected",
                None,
                None,
            )
        }
        BrowserRequest::GetCapabilities => success(
            request_id,
            ResponseCode::Capabilities,
            "capabilities",
            None,
            Some(json!({
                "schemaVersion": SCHEMA_VERSION,
                "captureDownload": true,
                "mediaInspection": true,
                "sendsCredentials": false,
                "drmCircumvention": false
            })),
        ),
        BrowserRequest::GetCaptureSettings => match service.settings().await {
            Ok(settings) => {
                record_connection();
                success(
                    request_id,
                    ResponseCode::Capabilities,
                    "browser capture settings",
                    None,
                    Some(json!({
                        "enabled": settings.browser_capture_mode != BrowserCaptureMode::Off,
                        "mode": if settings.browser_capture_mode == BrowserCaptureMode::Automatic { "automatic" } else { "ask" },
                        "captureMode": settings.browser_capture_mode,
                        "excludedSites": settings.browser_excluded_sites
                    })),
                )
            }
            Err(error) => ResponseEnvelope::error(
                Some(request_id),
                ResponseCode::InternalError,
                error.to_string(),
            ),
        },
        BrowserRequest::UpdateCaptureSettings(update) => match service.settings().await {
            Ok(mut settings) => {
                settings.browser_capture_mode = update.effective_mode();
                settings.browser_excluded_sites = update.excluded_sites;
                match service.save_settings(settings).await {
                    Ok(()) => success(
                        request_id,
                        ResponseCode::Accepted,
                        "browser capture settings saved",
                        None,
                        None,
                    ),
                    Err(error) => ResponseEnvelope::error(
                        Some(request_id),
                        ResponseCode::InternalError,
                        error.to_string(),
                    ),
                }
            }
            Err(error) => ResponseEnvelope::error(
                Some(request_id),
                ResponseCode::InternalError,
                error.to_string(),
            ),
        },
        BrowserRequest::ReportCaptureDiagnostics(report) => {
            record_capture_diagnostics(report);
            success(
                request_id,
                ResponseCode::Accepted,
                "browser diagnostics recorded",
                None,
                None,
            )
        }
        BrowserRequest::CaptureDownload(capture) => {
            let browser = format!("{:?}", capture.browser).to_ascii_lowercase();
            match service
                .capture_browser_download(BrowserCaptureRequest {
                    request_id,
                    idempotency_key: capture.idempotency_key,
                    url: capture.url,
                    browser,
                    source_origin: capture.source_page_origin,
                    automatic: capture.automatic,
                })
                .await
            {
                Ok(BrowserCaptureResult::Created(job_id)) => {
                    record_capture_result(Some(job_id), None);
                    success(
                        request_id,
                        ResponseCode::Accepted,
                        "download durably queued",
                        Some(job_id),
                        None,
                    )
                }
                Ok(BrowserCaptureResult::Duplicate(job_id)) => {
                    record_capture_result(Some(job_id), None);
                    success(
                        request_id,
                        ResponseCode::Duplicate,
                        "capture was already queued",
                        Some(job_id),
                        None,
                    )
                }
                Ok(BrowserCaptureResult::Pending) => {
                    record_capture_result(None, Some("duplicate_pending"));
                    ResponseEnvelope::error(
                        Some(request_id),
                        ResponseCode::Duplicate,
                        "capture is already being processed",
                    )
                }
                Err(error) => {
                    record_capture_result(None, Some("capture_rejected"));
                    ResponseEnvelope::error(
                        Some(request_id),
                        ResponseCode::Rejected,
                        error.to_string(),
                    )
                }
            }
        }
        BrowserRequest::InspectMedia(inspect) => {
            if inspect.authorization_confirmed {
                return ResponseEnvelope::error(
                    Some(request_id),
                    ResponseCode::Rejected,
                    "browser-asserted authorization is not accepted; confirm in GrabBit",
                );
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            match app.emit("browser-media-request", inspect.url) {
                Ok(()) => success(
                    request_id,
                    ResponseCode::Accepted,
                    "open GrabBit to confirm and inspect media",
                    None,
                    None,
                ),
                Err(error) => ResponseEnvelope::error(
                    Some(request_id),
                    ResponseCode::InternalError,
                    error.to_string(),
                ),
            }
        }
        BrowserRequest::OpenApplication => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            success(
                request_id,
                ResponseCode::Accepted,
                "GrabBit opened",
                None,
                None,
            )
        }
        BrowserRequest::CreateMediaDownload(_)
        | BrowserRequest::GetRequestStatus(_)
        | BrowserRequest::CancelPendingCapture(_) => ResponseEnvelope::error(
            Some(request_id),
            ResponseCode::Rejected,
            "request is not available in this build",
        ),
    }
}

fn success(
    request_id: uuid::Uuid,
    code: ResponseCode,
    message: &str,
    job_id: Option<uuid::Uuid>,
    data: Option<serde_json::Value>,
) -> ResponseEnvelope {
    ResponseEnvelope {
        schema_version: SCHEMA_VERSION,
        request_id: Some(request_id),
        ok: true,
        code,
        message: message.into(),
        job_id,
        data,
    }
}

#[cfg(windows)]
mod windows {
    use std::{io, path::Path, sync::Arc, time::Duration};

    use browser_native_host::{MAX_MESSAGE_BYTES, RequestEnvelope, validate_request};
    use download_core::DownloadService;
    use tauri::AppHandle;
    use tokio::{
        fs,
        io::{AsyncReadExt, AsyncWriteExt},
        net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions},
        time::timeout,
    };
    use uuid::Uuid;

    pub async fn serve(service: Arc<DownloadService>, data_directory: &Path, app: AppHandle) {
        let endpoint_file = data_directory.join("browser-ipc.endpoint");
        if let Ok(existing) = fs::read_to_string(&endpoint_file).await
            && valid_endpoint(existing.trim())
            && ClientOptions::new().open(existing.trim()).is_ok()
        {
            tracing::warn!("browser named pipe is already active");
            return;
        }
        if let Err(error) = fs::remove_file(&endpoint_file).await
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::error!(%error, "failed to remove stale IPC capability file");
            return;
        }
        let endpoint = format!(r"\\.\pipe\grabbit-{}", Uuid::new_v4());
        let first = match ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&endpoint)
        {
            Ok(server) => server,
            Err(error) => {
                tracing::error!(%error, "failed to create browser named pipe");
                return;
            }
        };
        let write_result = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&endpoint_file)
            .await;
        let mut endpoint_handle = match write_result {
            Ok(file) => file,
            Err(error) => {
                tracing::error!(%error, "another GrabBit instance owns browser IPC");
                return;
            }
        };
        if let Err(error) = endpoint_handle.write_all(endpoint.as_bytes()).await {
            tracing::error!(%error, "failed to write IPC capability file");
            return;
        }
        if let Err(error) = endpoint_handle.sync_all().await {
            tracing::error!(%error, "failed to persist IPC capability file");
            return;
        }
        drop(endpoint_handle);

        let mut server = first;
        loop {
            if let Err(error) = server.connect().await {
                tracing::warn!(%error, "browser named-pipe connection failed");
                continue;
            }
            let next = match ServerOptions::new()
                .reject_remote_clients(true)
                .create(&endpoint)
            {
                Ok(next) => next,
                Err(error) => {
                    tracing::error!(%error, "failed to create next named-pipe instance");
                    return;
                }
            };
            let service = Arc::clone(&service);
            let app = app.clone();
            tokio::spawn(async move {
                if let Err(error) = handle(server, service, app).await {
                    super::record_error("browser named-pipe request rejected");
                    tracing::warn!(%error, "browser named-pipe request rejected");
                }
            });
            server = next;
        }
    }

    async fn handle(
        mut stream: NamedPipeServer,
        service: Arc<DownloadService>,
        app: AppHandle,
    ) -> Result<(), String> {
        timeout(Duration::from_secs(8), async {
            let mut header = [0_u8; 4];
            stream.read_exact(&mut header).await?;
            let length = u32::from_le_bytes(header) as usize;
            if length > MAX_MESSAGE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "message exceeds limit",
                ));
            }
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).await?;
            let request: RequestEnvelope = serde_json::from_slice(&bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid schema"))?;
            validate_request(&request)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            let response = super::dispatch_common(request, service, &app).await;
            let bytes = serde_json::to_vec(&response).map_err(io::Error::other)?;
            let length = u32::try_from(bytes.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "response too large"))?;
            stream.write_all(&length.to_le_bytes()).await?;
            stream.write_all(&bytes).await?;
            stream.flush().await
        })
        .await
        .map_err(|_| "browser IPC timed out".to_owned())?
        .map_err(|error| error.to_string())
    }

    fn valid_endpoint(value: &str) -> bool {
        value
            .strip_prefix(r"\\.\pipe\grabbit-")
            .is_some_and(|id| Uuid::parse_str(id).is_ok())
    }
}

#[cfg(unix)]
pub use unix::serve;

#[cfg(windows)]
pub use windows::serve;

#[cfg(not(any(unix, windows)))]
pub async fn serve(
    _service: std::sync::Arc<download_core::DownloadService>,
    _data_directory: &std::path::Path,
    _app: tauri::AppHandle,
) {
    tracing::warn!("browser IPC is not yet available on this platform");
}
