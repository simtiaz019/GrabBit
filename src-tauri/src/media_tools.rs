use std::path::{Path, PathBuf};

use download_common::AppSettings;
use media_engine::{ToolCapabilities, ToolConfig, tool_path_is_valid};
use serde::Serialize;

#[derive(Debug)]
pub struct MediaToolRuntime {
    pub automatic: ToolConfig,
    bundled_directory: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaToolDiagnostic {
    pub name: &'static str,
    pub version: String,
    pub status: &'static str,
    pub source: &'static str,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaToolsDiagnostics {
    pub yt_dlp: MediaToolDiagnostic,
    pub ffmpeg: MediaToolDiagnostic,
    pub ffprobe: MediaToolDiagnostic,
}

impl MediaToolRuntime {
    #[must_use]
    pub fn discover(resources: &Path) -> Self {
        let bundled_directory = resources.join("media-tools");
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let automatic = ToolConfig::from_paths(
            automatic_path(&bundled_directory.join(format!("yt-dlp{suffix}")), "yt-dlp"),
            automatic_path(&bundled_directory.join(format!("ffmpeg{suffix}")), "ffmpeg"),
            automatic_path(
                &bundled_directory.join(format!("ffprobe{suffix}")),
                "ffprobe",
            ),
        );
        Self {
            automatic,
            bundled_directory,
        }
    }

    #[must_use]
    pub fn diagnostics(
        &self,
        settings: &AppSettings,
        resolved: &ToolConfig,
        versions: &ToolCapabilities,
    ) -> MediaToolsDiagnostics {
        MediaToolsDiagnostics {
            yt_dlp: self.diagnostic(
                "yt-dlp",
                &settings.yt_dlp_path,
                &resolved.yt_dlp,
                &versions.yt_dlp,
            ),
            ffmpeg: self.diagnostic(
                "FFmpeg",
                &settings.ffmpeg_path,
                &resolved.ffmpeg,
                &versions.ffmpeg,
            ),
            ffprobe: self.diagnostic(
                "FFprobe",
                &settings.ffprobe_path,
                &resolved.ffprobe,
                &versions.ffprobe,
            ),
        }
    }

    fn diagnostic(
        &self,
        name: &'static str,
        configured: &str,
        resolved: &Path,
        version: &str,
    ) -> MediaToolDiagnostic {
        let configured = Path::new(configured);
        let source = if tool_path_is_valid(configured) && configured == resolved {
            "custom"
        } else if resolved.starts_with(&self.bundled_directory) {
            "bundled"
        } else {
            "system"
        };
        MediaToolDiagnostic {
            name,
            version: version.lines().next().unwrap_or("Unknown").to_owned(),
            status: "Ready",
            source,
            path: resolved.to_string_lossy().into_owned(),
        }
    }
}

fn automatic_path(bundled: &Path, executable: &str) -> PathBuf {
    if tool_path_is_valid(bundled) {
        return bundled.to_path_buf();
    }
    discover_system_tool(executable).unwrap_or_default()
}

fn discover_system_tool(name: &str) -> Option<PathBuf> {
    let executable = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let from_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(&executable))
            .find(|candidate| tool_path_is_valid(candidate))
    });
    if from_path.is_some() {
        return from_path;
    }
    #[cfg(target_os = "macos")]
    for directory in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        let candidate = Path::new(directory).join(&executable);
        if tool_path_is_valid(&candidate) {
            return Some(candidate);
        }
    }
    None
}
