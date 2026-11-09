use std::{path::PathBuf, sync::Arc};

use base64::Engine;
use download_common::{AppSettings, DownloadJob, HealthResponse, HealthStatus, RemoteMetadata};
use download_core::{AddDownloadRequest, AddMediaRequest, DownloadService};
use media_engine::{
    InspectRequest, MediaInspection, MediaInspector, MediaOutput, YtDlpAdapter,
    fetch_secure_thumbnail,
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
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

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects AppHandle command arguments by value.
pub fn get_launch_at_login(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects AppHandle command arguments by value.
pub fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(|error| error.to_string())
}

fn media_adapter(
    service: &DownloadService,
    settings: &AppSettings,
) -> Result<YtDlpAdapter, String> {
    YtDlpAdapter::new(service.resolved_media_tools(settings)).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn inspect_media(
    service: State<'_, Arc<DownloadService>>,
    url: String,
    authorization_confirmed: bool,
) -> Result<MediaInspection, String> {
    let adapter = media_adapter(
        &service,
        &service
            .settings()
            .await
            .map_err(|error| error.to_string())?,
    )?;
    let mut inspection = adapter
        .inspect(
            &InspectRequest {
                url,
                authorization_confirmed,
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|error| error.to_string())?;
    inspection.thumbnail_url = if let Some(url) = inspection.thumbnail_url.as_deref() {
        fetch_secure_thumbnail(url).await.ok().map(|thumbnail| {
            format!(
                "data:{};base64,{}",
                thumbnail.content_type,
                base64::engine::general_purpose::STANDARD.encode(thumbnail.bytes)
            )
        })
    } else {
        None
    };
    Ok(inspection)
}

#[tauri::command]
pub async fn media_capabilities(
    service: State<'_, Arc<DownloadService>>,
    runtime: State<'_, crate::media_tools::MediaToolRuntime>,
) -> Result<crate::media_tools::MediaToolsDiagnostics, String> {
    let settings = service
        .settings()
        .await
        .map_err(|error| error.to_string())?;
    let resolved = service.resolved_media_tools(&settings);
    let adapter = media_adapter(&service, &settings)?;
    let versions = adapter
        .capabilities()
        .await
        .map_err(|error| error.to_string())?;
    Ok(runtime.diagnostics(&settings, &resolved, &versions))
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

#[tauri::command]
pub async fn get_browser_diagnostics(
    app: AppHandle,
) -> Result<crate::browser_ipc::BrowserDiagnostics, String> {
    let data_directory = std::env::var_os("GRABBIT_DATA_DIRECTORY")
        .map(PathBuf::from)
        .map_or_else(
            || app.path().app_data_dir().map_err(|error| error.to_string()),
            Ok,
        )?;
    Ok(crate::browser_ipc::diagnostics(&data_directory))
}

fn app_data_directory(app: &AppHandle) -> Result<PathBuf, String> {
    std::env::var_os("GRABBIT_DATA_DIRECTORY")
        .map(PathBuf::from)
        .map_or_else(
            || app.path().app_data_dir().map_err(|error| error.to_string()),
            Ok,
        )
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects AppHandle command arguments by value.
pub fn get_browser_setup(
    app: AppHandle,
) -> Result<crate::browser_integration::BrowserSetupReport, String> {
    let data_directory = app_data_directory(&app)?;
    Ok(crate::browser_integration::setup_report(
        &data_directory.join("browser-extensions"),
    ))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri command arguments are deserialized by value.
pub fn repair_browser_integration(
    app: AppHandle,
    browser_ids: Vec<String>,
) -> Result<crate::browser_integration::BrowserSetupReport, String> {
    crate::browser_integration::repair_native_messaging(&browser_ids)?;
    get_browser_setup(app)
}

#[tauri::command]
pub fn remove_browser_integration() -> Result<(), String> {
    crate::browser_integration::remove_native_messaging()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects AppHandle command arguments by value.
pub fn get_onboarding_complete(app: AppHandle) -> Result<bool, String> {
    Ok(app_data_directory(&app)?
        .join("onboarding-v1.complete")
        .is_file())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri injects AppHandle command arguments by value.
pub fn set_onboarding_complete(app: AppHandle, complete: bool) -> Result<(), String> {
    let marker = app_data_directory(&app)?.join("onboarding-v1.complete");
    if complete {
        if let Some(parent) = marker.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(marker, b"completed\n").map_err(|error| error.to_string())
    } else {
        match std::fs::remove_file(marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
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
