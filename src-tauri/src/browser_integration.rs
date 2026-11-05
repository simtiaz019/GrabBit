use std::path::{Path, PathBuf};

use browser_native_host::SCHEMA_VERSION;
use serde::Serialize;

const CHROMIUM_EXTENSION_ID: &str = "hdfipadppabbjkbbodcdhjelmiifamjj";
const FIREFOX_EXTENSION_ID: &str = "grabbit@grabbit.download";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInstallation {
    pub id: &'static str,
    pub name: &'static str,
    pub detected: bool,
    pub extension_installed: Option<bool>,
    pub extension_enabled: Option<bool>,
    pub permissions_granted: Option<bool>,
    pub native_host_registered: bool,
    pub host_executable_valid: bool,
    pub connected: Option<bool>,
    pub last_test_result: String,
    pub setup_location: Option<String>,
}

fn selected_browser_families(browser_ids: &[String]) -> Result<Vec<(&str, &'static str)>, String> {
    if browser_ids.is_empty() {
        return Err("select at least one detected browser to configure".into());
    }
    let mut selected = Vec::new();
    for id in browser_ids {
        if selected.iter().any(|(selected_id, _)| *selected_id == id) {
            continue;
        }
        let family =
            browser_family(id).ok_or_else(|| format!("unsupported browser integration: {id}"))?;
        selected.push((id.as_str(), family));
    }
    Ok(selected)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSetupReport {
    pub app_version: &'static str,
    pub packaged_extension_version: Option<String>,
    pub native_host_version: &'static str,
    pub protocol_version: u16,
    pub host_executable: String,
    pub browsers: Vec<BrowserInstallation>,
}

pub fn native_host_install_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        return home_directory()
            .map(|home| home.join("Library/Application Support/GrabBit/grabbit-browser-host"));
    }
    #[cfg(target_os = "linux")]
    {
        let root = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home_directory().ok().map(|home| home.join(".local/share")))
            .ok_or_else(|| "could not resolve the user data directory".to_owned())?;
        return Ok(root.join("grabbit/grabbit-browser-host"));
    }
    #[cfg(windows)]
    {
        return std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("GrabBit/grabbit-browser-host.exe"))
            .ok_or_else(|| "LOCALAPPDATA is unavailable".to_owned());
    }
    #[allow(unreachable_code)]
    Err("native messaging is unsupported on this platform".into())
}

pub fn repair_native_messaging(browser_ids: &[String]) -> Result<(), String> {
    let selected = selected_browser_families(browser_ids)?;
    let host = native_host_install_path()?;
    if !host.is_file() {
        return Err(format!("native host is missing at {}", host.display()));
    }
    let chromium = serde_json::json!({
        "name": "com.grabbit.native",
        "description": "GrabBit secure browser bridge",
        "path": host,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{CHROMIUM_EXTENSION_ID}/")]
    });
    let firefox = serde_json::json!({
        "name": "com.grabbit.native",
        "description": "GrabBit secure browser bridge",
        "path": host,
        "type": "stdio",
        "allowed_extensions": [FIREFOX_EXTENSION_ID]
    });

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        write_selected_manifests(&selected, &chromium, &firefox, manifest_location)?;
    }
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};

        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "LOCALAPPDATA is unavailable".to_owned())?
            .join("GrabBit");
        let chromium_path = local.join("com.grabbit.native.chromium.json");
        let firefox_path = local.join("com.grabbit.native.firefox.json");
        if selected.iter().any(|(_, family)| *family == "chromium") {
            write_manifest(&chromium_path, &chromium)?;
        }
        if selected.iter().any(|(_, family)| *family == "firefox") {
            write_manifest(&firefox_path, &firefox)?;
        }
        let current_user = RegKey::predef(HKEY_CURRENT_USER);
        for (id, family) in &selected {
            if *family != "chromium" {
                continue;
            }
            let key =
                windows_registry_key(id).expect("supported Chromium browser has a registry key");
            current_user
                .create_subkey(key)
                .and_then(|(key, _)| key.set_value("", &chromium_path.to_string_lossy().as_ref()))
                .map_err(|error| format!("could not register {key}: {error}"))?;
        }
        if selected.iter().any(|(id, _)| *id == "firefox") {
            let key = windows_registry_key("firefox").expect("Firefox has a registry key");
            current_user
                .create_subkey(key)
                .and_then(|(key, _)| key.set_value("", &firefox_path.to_string_lossy().as_ref()))
                .map_err(|error| format!("could not register {key}: {error}"))?;
        }
    }
    Ok(())
}

pub fn remove_native_messaging() -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    for path in chromium_manifest_locations()
        .into_iter()
        .chain(firefox_manifest_locations())
    {
        remove_owned_file(&path)?;
    }
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};

        let current_user = RegKey::predef(HKEY_CURRENT_USER);
        for key in [
            r"Software\Google\Chrome\NativeMessagingHosts\com.grabbit.native",
            r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.grabbit.native",
            r"Software\Microsoft\Edge\NativeMessagingHosts\com.grabbit.native",
            r"Software\Chromium\NativeMessagingHosts\com.grabbit.native",
            r"Software\Mozilla\NativeMessagingHosts\com.grabbit.native",
        ] {
            match current_user.delete_subkey_all(key) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("could not remove {key}: {error}")),
            }
        }
        if let Some(root) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            for name in [
                "com.grabbit.native.chromium.json",
                "com.grabbit.native.firefox.json",
                "grabbit-browser-host.exe",
            ] {
                remove_owned_file(&root.join("GrabBit").join(name))?;
            }
        }
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    remove_owned_file(&native_host_install_path()?)?;
    Ok(())
}

fn browser_family(id: &str) -> Option<&'static str> {
    browser_definitions()
        .into_iter()
        .find(|(browser_id, _, _, _)| *browser_id == id)
        .and_then(|(_, _, _, family)| family)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn manifest_location(browser: &str) -> Result<PathBuf, String> {
    match browser {
        "chrome" => chromium_manifest_locations().into_iter().next(),
        "brave" => chromium_manifest_locations().into_iter().nth(1),
        "edge" => chromium_manifest_locations().into_iter().nth(2),
        "chromium" => chromium_manifest_locations().into_iter().nth(3),
        "firefox" => firefox_manifest_locations().into_iter().next(),
        _ => None,
    }
    .ok_or_else(|| format!("could not resolve Native Messaging manifest for {browser}"))
}

#[cfg(windows)]
fn windows_registry_key(browser: &str) -> Option<&'static str> {
    match browser {
        "chrome" => Some(r"Software\Google\Chrome\NativeMessagingHosts\com.grabbit.native"),
        "brave" => {
            Some(r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.grabbit.native")
        }
        "edge" => Some(r"Software\Microsoft\Edge\NativeMessagingHosts\com.grabbit.native"),
        "chromium" => Some(r"Software\Chromium\NativeMessagingHosts\com.grabbit.native"),
        "firefox" => Some(r"Software\Mozilla\NativeMessagingHosts\com.grabbit.native"),
        _ => None,
    }
}

pub fn setup_report(extension_directory: &Path) -> BrowserSetupReport {
    let host = native_host_install_path().unwrap_or_default();
    let packaged_extension_version =
        std::fs::read(extension_directory.join("chromium/manifest.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|manifest| manifest.get("version")?.as_str().map(ToOwned::to_owned));
    BrowserSetupReport {
        app_version: env!("CARGO_PKG_VERSION"),
        packaged_extension_version,
        native_host_version: env!("CARGO_PKG_VERSION"),
        protocol_version: SCHEMA_VERSION,
        host_executable: host.to_string_lossy().into_owned(),
        browsers: browser_definitions()
            .into_iter()
            .map(|(id, name, detected, family)| {
                let registered = family.is_some_and(|_| native_host_registered(id));
                BrowserInstallation {
                    id,
                    name,
                    detected,
                    extension_installed: None,
                    extension_enabled: None,
                    permissions_granted: None,
                    native_host_registered: registered,
                    host_executable_valid: host.is_file(),
                    connected: None,
                    last_test_result: "Not tested from this browser".into(),
                    setup_location: family.map(|family| {
                        extension_directory
                            .join(if family == "firefox" {
                                "firefox"
                            } else {
                                "chromium"
                            })
                            .to_string_lossy()
                            .into_owned()
                    }),
                }
            })
            .collect(),
    }
}

fn write_manifest(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid manifest path: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    std::fs::write(path, bytes)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

fn write_selected_manifests<F>(
    selected: &[(&str, &str)],
    chromium: &serde_json::Value,
    firefox: &serde_json::Value,
    mut manifest_location: F,
) -> Result<(), String>
where
    F: FnMut(&str) -> Result<PathBuf, String>,
{
    for (id, family) in selected {
        let manifest = match *family {
            "chromium" => chromium,
            "firefox" => firefox,
            _ => return Err(format!("unsupported browser family for {id}")),
        };
        write_manifest(&manifest_location(id)?, manifest)?;
    }
    Ok(())
}

fn remove_owned_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not remove {}: {error}", path.display())),
    }
}

fn native_host_registered(browser: &str) -> bool {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let locations = match browser {
            "chrome" => chromium_manifest_locations()
                .into_iter()
                .next()
                .into_iter()
                .collect(),
            "brave" => chromium_manifest_locations()
                .into_iter()
                .nth(1)
                .into_iter()
                .collect(),
            "edge" => chromium_manifest_locations()
                .into_iter()
                .nth(2)
                .into_iter()
                .collect(),
            "chromium" => chromium_manifest_locations()
                .into_iter()
                .nth(3)
                .into_iter()
                .collect(),
            "firefox" => firefox_manifest_locations(),
            _ => Vec::new(),
        };
        return locations
            .into_iter()
            .any(|path| manifest_points_to_host(&path));
    }
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};

        let Some(key) = windows_registry_key(browser) else {
            return false;
        };
        let current_user = RegKey::predef(HKEY_CURRENT_USER);
        return current_user
            .open_subkey(key)
            .and_then(|key| key.get_value::<String, _>(""))
            .map(PathBuf::from)
            .is_ok_and(|path| manifest_points_to_host(&path));
    }
    #[allow(unreachable_code)]
    false
}

fn manifest_points_to_host(path: &Path) -> bool {
    let expected = native_host_install_path().ok();
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|manifest| manifest.get("path")?.as_str().map(PathBuf::from))
        .zip(expected)
        .is_some_and(|(actual, expected)| actual == expected && expected.is_file())
}

#[cfg(target_os = "macos")]
fn chromium_manifest_locations() -> Vec<PathBuf> {
    let Ok(home) = home_directory() else {
        return Vec::new();
    };
    [
        "Library/Application Support/Google/Chrome/NativeMessagingHosts/com.grabbit.native.json",
        "Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts/com.grabbit.native.json",
        "Library/Application Support/Microsoft Edge/NativeMessagingHosts/com.grabbit.native.json",
        "Library/Application Support/Chromium/NativeMessagingHosts/com.grabbit.native.json",
    ]
    .iter()
    .map(|path| home.join(path))
    .collect()
}

#[cfg(target_os = "macos")]
fn firefox_manifest_locations() -> Vec<PathBuf> {
    home_directory()
        .map(|home| {
            vec![home.join(
                "Library/Application Support/Mozilla/NativeMessagingHosts/com.grabbit.native.json",
            )]
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn chromium_manifest_locations() -> Vec<PathBuf> {
    let Ok(home) = home_directory() else {
        return Vec::new();
    };
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    [
        "google-chrome",
        "BraveSoftware/Brave-Browser",
        "microsoft-edge",
        "chromium",
    ]
    .iter()
    .map(|root| {
        config
            .join(root)
            .join("NativeMessagingHosts/com.grabbit.native.json")
    })
    .collect()
}

#[cfg(target_os = "linux")]
fn firefox_manifest_locations() -> Vec<PathBuf> {
    home_directory()
        .map(|home| vec![home.join(".mozilla/native-messaging-hosts/com.grabbit.native.json")])
        .unwrap_or_default()
}

fn home_directory() -> Result<PathBuf, String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| "the user home directory is unavailable".to_owned())
}

#[cfg(target_os = "macos")]
fn browser_definitions() -> Vec<(&'static str, &'static str, bool, Option<&'static str>)> {
    let home = home_directory().unwrap_or_default();
    vec![
        (
            "chrome",
            "Google Chrome",
            app_exists(&home, "Google Chrome.app"),
            Some("chromium"),
        ),
        (
            "brave",
            "Brave",
            app_exists(&home, "Brave Browser.app"),
            Some("chromium"),
        ),
        (
            "edge",
            "Microsoft Edge",
            app_exists(&home, "Microsoft Edge.app"),
            Some("chromium"),
        ),
        (
            "chromium",
            "Chromium",
            app_exists(&home, "Chromium.app"),
            Some("chromium"),
        ),
        (
            "firefox",
            "Mozilla Firefox",
            app_exists(&home, "Firefox.app"),
            Some("firefox"),
        ),
        (
            "safari",
            "Safari",
            Path::new("/Applications/Safari.app").is_dir(),
            None,
        ),
    ]
}

#[cfg(target_os = "macos")]
fn app_exists(home: &Path, name: &str) -> bool {
    Path::new("/Applications").join(name).is_dir() || home.join("Applications").join(name).is_dir()
}

#[cfg(windows)]
fn browser_definitions() -> Vec<(&'static str, &'static str, bool, Option<&'static str>)> {
    let roots = ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|name| std::env::var_os(name).map(PathBuf::from))
        .collect::<Vec<_>>();
    let present = |relative: &str| roots.iter().any(|root| root.join(relative).is_file());
    vec![
        (
            "chrome",
            "Google Chrome",
            present("Google/Chrome/Application/chrome.exe"),
            Some("chromium"),
        ),
        (
            "brave",
            "Brave",
            present("BraveSoftware/Brave-Browser/Application/brave.exe"),
            Some("chromium"),
        ),
        (
            "edge",
            "Microsoft Edge",
            present("Microsoft/Edge/Application/msedge.exe"),
            Some("chromium"),
        ),
        (
            "chromium",
            "Chromium",
            present("Chromium/Application/chrome.exe"),
            Some("chromium"),
        ),
        (
            "firefox",
            "Mozilla Firefox",
            present("Mozilla Firefox/firefox.exe"),
            Some("firefox"),
        ),
    ]
}

#[cfg(target_os = "linux")]
fn browser_definitions() -> Vec<(&'static str, &'static str, bool, Option<&'static str>)> {
    vec![
        (
            "chrome",
            "Google Chrome",
            executable_in_path("google-chrome"),
            Some("chromium"),
        ),
        (
            "brave",
            "Brave",
            executable_in_path("brave-browser"),
            Some("chromium"),
        ),
        (
            "edge",
            "Microsoft Edge",
            executable_in_path("microsoft-edge"),
            Some("chromium"),
        ),
        (
            "chromium",
            "Chromium",
            executable_in_path("chromium"),
            Some("chromium"),
        ),
        (
            "firefox",
            "Mozilla Firefox",
            executable_in_path("firefox"),
            Some("firefox"),
        ),
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn browser_definitions() -> Vec<(&'static str, &'static str, bool, Option<&'static str>)> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn executable_in_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|value| {
        std::env::split_paths(&value).any(|directory| directory.join(name).is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_selection_is_nonempty_supported_and_deduplicated() {
        assert!(selected_browser_families(&[]).is_err());
        assert!(selected_browser_families(&["safari".into()]).is_err());

        let browser_ids = ["brave".into(), "brave".into(), "firefox".into()];
        let selected = selected_browser_families(&browser_ids).expect("supported browsers");
        assert_eq!(selected, [("brave", "chromium"), ("firefox", "firefox")]);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn first_run_registration_writes_only_selected_browser_manifests() {
        let root = tempfile::tempdir().expect("temporary directory");
        let browser_ids = ["chrome".into(), "firefox".into()];
        let selected = selected_browser_families(&browser_ids).expect("supported browsers");
        let chromium = serde_json::json!({"name":"com.grabbit.native","path":"/host/chromium"});
        let firefox = serde_json::json!({"name":"com.grabbit.native","path":"/host/firefox"});

        write_selected_manifests(&selected, &chromium, &firefox, |browser| {
            Ok(root.path().join(format!("{browser}.json")))
        })
        .expect("selected manifests");

        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(root.path().join("chrome.json")).expect("Chrome manifest")
            )
            .expect("valid Chrome manifest"),
            chromium
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(root.path().join("firefox.json")).expect("Firefox manifest")
            )
            .expect("Firefox manifest"),
            firefox
        );
        assert!(!root.path().join("brave.json").exists());
    }

    #[cfg(windows)]
    #[test]
    fn windows_registry_mapping_covers_detected_browsers() {
        for browser in ["chrome", "brave", "edge", "chromium", "firefox"] {
            assert!(windows_registry_key(browser).is_some(), "{browser}");
        }
        assert!(windows_registry_key("safari").is_none());
    }

    #[test]
    fn packaged_extension_version_is_reported_without_claiming_installation() {
        let root = tempfile::tempdir().expect("temporary directory");
        let chromium = root.path().join("chromium");
        std::fs::create_dir_all(&chromium).expect("extension directory");
        std::fs::write(chromium.join("manifest.json"), br#"{"version":"1.2.3"}"#)
            .expect("manifest");
        let report = setup_report(root.path());
        assert_eq!(report.packaged_extension_version.as_deref(), Some("1.2.3"));
        assert!(
            report
                .browsers
                .iter()
                .all(|browser| browser.extension_installed.is_none())
        );
    }
}
