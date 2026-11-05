use std::{path::PathBuf, sync::Arc};

use download_common::{AppSettings, DownloadJob, HealthResponse, HealthStatus, RemoteMetadata};
use download_core::{AddDownloadRequest, AddMediaRequest, DownloadService};
use media_engine::{
    InspectRequest, MediaInspection, MediaInspector, MediaOutput, ToolCapabilities, ToolConfig,
    YtDlpAdapter,
};
use tauri::State;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tauri::command]
pub fn health_check() -> HealthResponse {
    tracing::info!(command = "health_check", "frontend command invoked");
    HealthResponse {
        app_name: "GrabBit".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        status: HealthStatus::Ok,
    }
}

#[tauri::command]
pub async fn probe_url(
    service: State<'_, Arc<DownloadService>>,
    url: String,
) -> Result<RemoteMetadata, String> {
    service.probe(&url).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn add_download(
    service: State<'_, Arc<DownloadService>>,
    url: String,
    destination_directory: String,
    segments: u8,
    sha256: Option<String>,
) -> Result<DownloadJob, String> {
    service
        .add(AddDownloadRequest {
            url,
            destination_directory: PathBuf::from(destination_directory),
            segments,
            sha256: sha256.filter(|value| !value.trim().is_empty()),
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_downloads(
    service: State<'_, Arc<DownloadService>>,
) -> Result<Vec<DownloadJob>, String> {
    service.list().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn pause_download(
    service: State<'_, Arc<DownloadService>>,
    id: Uuid,
) -> Result<(), String> {
    service.pause(id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn resume_download(
    service: State<'_, Arc<DownloadService>>,
    id: Uuid,
) -> Result<(), String> {
    service.resume(id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn retry_download(
    service: State<'_, Arc<DownloadService>>,
    id: Uuid,
) -> Result<(), String> {
    service.retry(id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cancel_download(
    service: State<'_, Arc<DownloadService>>,
    id: Uuid,
) -> Result<(), String> {
    service.cancel(id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn remove_download(
    service: State<'_, Arc<DownloadService>>,
    id: Uuid,
) -> Result<(), String> {
    service.remove(id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_settings(service: State<'_, Arc<DownloadService>>) -> Result<AppSettings, String> {
    service.settings().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_settings(
    service: State<'_, Arc<DownloadService>>,
    settings: AppSettings,
) -> Result<(), String> {
    service
        .save_settings(settings)
        .await
        .map_err(|error| error.to_string())
}

fn media_adapter(settings: &AppSettings) -> Result<YtDlpAdapter, String> {
    YtDlpAdapter::new(ToolConfig::from_paths(
        PathBuf::from(&settings.yt_dlp_path),
        PathBuf::from(&settings.ffmpeg_path),
        PathBuf::from(&settings.ffprobe_path),
    ))
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn inspect_media(
    service: State<'_, Arc<DownloadService>>,
    url: String,
    authorization_confirmed: bool,
) -> Result<MediaInspection, String> {
    let adapter = media_adapter(
        &service
            .settings()
            .await
            .map_err(|error| error.to_string())?,
    )?;
    adapter
        .inspect(
            &InspectRequest {
                url,
                authorization_confirmed,
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn media_capabilities(
    service: State<'_, Arc<DownloadService>>,
) -> Result<ToolCapabilities, String> {
    let adapter = media_adapter(
        &service
            .settings()
            .await
            .map_err(|error| error.to_string())?,
    )?;
    adapter
        .capabilities()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_media_download(
    service: State<'_, Arc<DownloadService>>,
    url: String,
    destination_directory: String,
    format_ids: Vec<String>,
    output: MediaOutput,
    authorization_confirmed: bool,
) -> Result<DownloadJob, String> {
    service
        .add_media(AddMediaRequest {
            url,
            destination_directory: PathBuf::from(destination_directory),
            format_ids,
            output,
            authorization_confirmed,
        })
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_check_exposes_the_desktop_contract() {
        let response = health_check();
        assert_eq!(response.app_name, "GrabBit");
        assert_eq!(response.status, HealthStatus::Ok);
    }
}
