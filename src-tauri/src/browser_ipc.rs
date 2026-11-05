#[cfg(unix)]
mod unix {
    use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};

    use browser_native_host::{
        BrowserRequest, MAX_MESSAGE_BYTES, RequestEnvelope, ResponseCode, ResponseEnvelope,
        SCHEMA_VERSION, validate_request,
    };
    use download_core::{BrowserCaptureRequest, BrowserCaptureResult, DownloadService};
    use serde_json::json;
    use tauri::{AppHandle, Emitter, Manager};
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
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let service = Arc::clone(&service);
                    let app = app.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle(stream, service, app).await {
                            tracing::warn!(%error, "browser IPC request rejected");
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
        let response = dispatch(request, service, &app).await;
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

    #[allow(clippy::too_many_lines)]
    async fn dispatch(
        request: RequestEnvelope,
        service: Arc<DownloadService>,
        app: &AppHandle,
    ) -> ResponseEnvelope {
        let request_id = request.request_id;
        match request.message {
            BrowserRequest::Ping => success(
                request_id,
                ResponseCode::Pong,
                "GrabBit is connected",
                None,
                None,
            ),
            BrowserRequest::GetCapabilities => success(
                request_id,
                ResponseCode::Capabilities,
                "capabilities",
                None,
                Some(json!({
                    "schemaVersion": SCHEMA_VERSION, "captureDownload": true, "mediaInspection": true,
                    "sendsCredentials": false, "drmCircumvention": false
                })),
            ),
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
                    Ok(BrowserCaptureResult::Created(job_id)) => success(
                        request_id,
                        ResponseCode::Accepted,
                        "download durably queued",
                        Some(job_id),
                        None,
                    ),
                    Ok(BrowserCaptureResult::Duplicate(job_id)) => success(
                        request_id,
                        ResponseCode::Duplicate,
                        "capture was already queued",
                        Some(job_id),
                        None,
                    ),
                    Ok(BrowserCaptureResult::Pending) => ResponseEnvelope::error(
                        Some(request_id),
                        ResponseCode::Duplicate,
                        "capture is already being processed",
                    ),
                    Err(error) => ResponseEnvelope::error(
                        Some(request_id),
                        ResponseCode::Rejected,
                        error.to_string(),
                    ),
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
}

#[cfg(unix)]
pub use unix::serve;

#[cfg(not(unix))]
pub async fn serve(
    _service: std::sync::Arc<download_core::DownloadService>,
    _data_directory: &std::path::Path,
    _app: tauri::AppHandle,
) {
    tracing::warn!("browser IPC is not yet available on this platform");
}
