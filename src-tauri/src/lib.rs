mod browser_ipc;
mod commands;

use download_core::DownloadService;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the native `GrabBit` application event loop.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the desktop application.
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_directory = std::env::var_os("GRABBIT_DATA_DIRECTORY")
                .map(std::path::PathBuf::from)
                .map_or_else(
                    || app.path().app_data_dir().map_err(|error| error.to_string()),
                    Ok,
                )?;
            let service = tauri::async_runtime::block_on(DownloadService::open(&data_directory))
                .map_err(|error| error.to_string())?;
            let ipc_service = service.clone();
            let ipc_directory = data_directory.clone();
            let ipc_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                browser_ipc::serve(ipc_service, &ipc_directory, ipc_handle).await;
            });
            let mut settings = tauri::async_runtime::block_on(service.settings())
                .map_err(|error| error.to_string())?;
            if let Some(directory) = std::env::var_os("GRABBIT_DEFAULT_DOWNLOAD_DIRECTORY") {
                settings.default_download_directory = std::path::PathBuf::from(directory)
                    .to_string_lossy()
                    .into_owned();
            } else if settings.default_download_directory.trim().is_empty()
                && let Ok(download_directory) = app.path().download_dir()
            {
                settings.default_download_directory =
                    download_directory.to_string_lossy().into_owned();
            }
            if std::env::var_os("GRABBIT_ENABLE_AUTOMATIC_CAPTURE").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                settings.browser_automatic_interception = true;
            }
            tauri::async_runtime::block_on(service.save_settings(settings))
                .map_err(|error| error.to_string())?;
            let mut progress = service.subscribe_progress();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(snapshot) = progress.recv().await {
                    if let Err(error) = handle.emit("download-progress", snapshot) {
                        tracing::warn!(%error, "failed to emit progress event");
                    }
                }
            });
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
            commands::probe_url,
            commands::add_download,
            commands::list_downloads,
            commands::pause_download,
            commands::resume_download,
            commands::retry_download,
            commands::cancel_download,
            commands::remove_download,
            commands::get_settings,
            commands::save_settings,
            commands::inspect_media,
            commands::media_capabilities,
            commands::create_media_download,
            commands::get_browser_diagnostics,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build GrabBit desktop application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let service = app
                    .state::<std::sync::Arc<DownloadService>>()
                    .inner()
                    .clone();
                tauri::async_runtime::block_on(service.shutdown());
            }
        });
}
