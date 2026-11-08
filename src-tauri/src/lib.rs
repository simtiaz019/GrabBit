mod browser_integration;
mod browser_ipc;
mod commands;
mod lifecycle;

use download_common::{BrowserCaptureMode, DownloadState};
use download_core::DownloadService;
use std::path::Path;
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

use crate::lifecycle::{ActivationDecision, ActivationSource, LifecycleState, activation_decision};

struct AppDataDirectory(std::path::PathBuf);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::too_many_lines)]
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
        .plugin(tauri_plugin_single_instance::init(
            |app, _arguments, _directory| {
                restore_main_window(app, None, ActivationSource::SecondInstance);
            },
        ))
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(LifecycleState::default())
        .setup(|app| {
            let data_directory = std::env::var_os("GRABBIT_DATA_DIRECTORY")
                .map(std::path::PathBuf::from)
                .map_or_else(
                    || app.path().app_data_dir().map_err(|error| error.to_string()),
                    Ok,
                )?;
            let resource_directory = app
                .path()
                .resource_dir()
                .map_err(|error| error.to_string())?;
            let extension_directory = data_directory.join("browser-extensions");
            sync_browser_extensions(&resource_directory, &extension_directory)
                .map_err(|error| error.to_string())?;
            sync_browser_native_host(&resource_directory).map_err(|error| error.to_string())?;
            let service = tauri::async_runtime::block_on(DownloadService::open(&data_directory))
                .map_err(|error| error.to_string())?;
            app.manage(AppDataDirectory(data_directory.clone()));
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
            if settings.yt_dlp_path.trim().is_empty()
                && let Some(path) = discover_tool("yt-dlp")
            {
                settings.yt_dlp_path = path.to_string_lossy().into_owned();
            }
            if settings.ffmpeg_path.trim().is_empty()
                && let Some(path) = discover_tool("ffmpeg")
            {
                settings.ffmpeg_path = path.to_string_lossy().into_owned();
            }
            if settings.ffprobe_path.trim().is_empty()
                && let Some(path) = discover_tool("ffprobe")
            {
                settings.ffprobe_path = path.to_string_lossy().into_owned();
            }
            if std::env::var_os("GRABBIT_ENABLE_AUTOMATIC_CAPTURE").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                settings.browser_capture_mode = BrowserCaptureMode::Automatic;
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
            setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let lifecycle = window.state::<LifecycleState>();
                if lifecycle
                    .close_decision()
                    .keeps_background_services_running()
                {
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        tracing::warn!(%error, "failed to hide main window");
                    }
                }
            }
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
            commands::get_browser_setup,
            commands::repair_browser_integration,
            commands::remove_browser_integration,
            commands::get_onboarding_complete,
            commands::set_onboarding_complete,
            commands::get_launch_at_login,
            commands::set_launch_at_login,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build GrabBit desktop application")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => shutdown(app),
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                restore_main_window(app, None, ActivationSource::Tray);
            }
            _ => {}
        });
}

fn shutdown(app: &tauri::AppHandle) {
    if !app.state::<LifecycleState>().begin_shutdown() {
        return;
    }
    let service = app
        .state::<std::sync::Arc<DownloadService>>()
        .inner()
        .clone();
    tauri::async_runtime::block_on(service.shutdown());
    let data_directory = app.state::<AppDataDirectory>();
    browser_ipc::cleanup_endpoint(&data_directory.0);
}

fn restore_main_window(
    app: &tauri::AppHandle,
    destination: Option<&str>,
    source: ActivationSource,
) {
    let ActivationDecision::RestoreAndFocus = activation_decision(source);
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = window.unminimize() {
        tracing::warn!(%error, "failed to unminimize main window");
    }
    if let Err(error) = window.show() {
        tracing::warn!(%error, "failed to show main window");
    }
    if let Err(error) = window.set_focus() {
        tracing::warn!(%error, "failed to focus main window");
    }
    if let Some(destination) = destination
        && let Err(error) = app.emit("tray-navigate", destination)
    {
        tracing::warn!(%error, "failed to navigate from tray");
    }
}

fn set_all_downloads_paused(app: &tauri::AppHandle, paused: bool) {
    let service = app
        .state::<std::sync::Arc<DownloadService>>()
        .inner()
        .clone();
    tauri::async_runtime::spawn(async move {
        let Ok(jobs) = service.list().await else {
            tracing::warn!("failed to list downloads for tray action");
            return;
        };
        for job in jobs {
            let result = if paused
                && matches!(
                    job.state,
                    DownloadState::Queued
                        | DownloadState::Downloading
                        | DownloadState::Merging
                        | DownloadState::Converting
                        | DownloadState::Verifying
                ) {
                service.pause(job.id).await
            } else if !paused && job.state == DownloadState::Paused {
                service.resume(job.id).await
            } else {
                continue;
            };
            if let Err(error) = result {
                tracing::warn!(%error, download_id = %job.id, "tray bulk action failed");
            }
        }
    });
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open GrabBit", true, None::<&str>)?;
    let active = MenuItem::with_id(
        app,
        "active-downloads",
        "Show active downloads",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let pause = MenuItem::with_id(app, "pause-all", "Pause all", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume-all", "Resume all", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit GrabBit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open, &active, &separator, &pause, &resume, &settings, &quit,
        ],
    )?;

    let mut tray = TrayIconBuilder::with_id("grabbit-tray")
        .tooltip("GrabBit Download Manager")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => restore_main_window(app, None, ActivationSource::Tray),
            "active-downloads" => {
                restore_main_window(app, Some("active"), ActivationSource::Tray);
            }
            "pause-all" => set_all_downloads_paused(app, true),
            "resume-all" => set_all_downloads_paused(app, false),
            "settings" => restore_main_window(app, Some("settings"), ActivationSource::Tray),
            "quit" => {
                app.state::<LifecycleState>().request_quit();
                app.exit(0);
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn discover_tool(name: &str) -> Option<std::path::PathBuf> {
    let executable = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let path_candidate = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(&executable))
            .find(|candidate| candidate.is_file())
    });
    if path_candidate.is_some() {
        return path_candidate;
    }
    #[cfg(target_os = "macos")]
    for directory in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        let candidate = std::path::Path::new(directory).join(&executable);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn sync_browser_extensions(resources: &Path, destination: &Path) -> std::io::Result<()> {
    for browser in ["chromium", "firefox"] {
        let source = resources.join("browser-extensions").join(browser);
        if !source.is_dir() {
            tracing::warn!(path = %source.display(), "packaged browser extension resources are unavailable; skipping update");
            continue;
        }
        copy_extension_directory(&source, &destination.join(browser))?;
    }
    Ok(())
}

fn sync_browser_native_host(resources: &Path) -> std::io::Result<()> {
    let executable = if cfg!(windows) {
        "grabbit-browser-host.exe"
    } else {
        "grabbit-browser-host"
    };
    let source = resources.join("browser-native-host").join(executable);
    if !source.is_file() {
        tracing::warn!(path = %source.display(), "packaged browser native host is unavailable; skipping update");
        return Ok(());
    }

    let destination =
        browser_integration::native_host_install_path().map_err(std::io::Error::other)?;
    let install_directory = destination.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid native host path")
    })?;
    std::fs::create_dir_all(install_directory)?;
    std::fs::copy(source, &destination)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn copy_extension_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_extension_directory(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            std::fs::copy(source_path, destination_path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "browser extension resources may contain only files and directories",
            ));
        }
    }
    Ok(())
}
