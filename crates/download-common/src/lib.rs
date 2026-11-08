//! Domain types shared across `GrabBit`'s Rust crates and IPC boundary.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub type DownloadId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Queued,
    Inspecting,
    Downloading,
    Merging,
    Converting,
    Verifying,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid download state transition from {from:?} to {to:?}")]
pub struct InvalidStateTransition {
    pub from: DownloadState,
    pub to: DownloadState,
}

impl DownloadState {
    /// Validates the explicit lifecycle transitions agreed in ADR-0001.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidStateTransition`] when `next` is not reachable from the
    /// current state.
    pub fn transition_to(self, next: Self) -> Result<Self, InvalidStateTransition> {
        let valid = matches!(
            (self, next),
            (Self::Queued, Self::Inspecting | Self::Cancelled)
                | (
                    Self::Inspecting,
                    Self::Downloading | Self::Failed | Self::Cancelled
                )
                | (
                    Self::Downloading,
                    Self::Paused
                        | Self::Merging
                        | Self::Converting
                        | Self::Verifying
                        | Self::Completed
                        | Self::Failed
                        | Self::Cancelled
                )
                | (
                    Self::Merging | Self::Converting,
                    Self::Verifying | Self::Failed | Self::Cancelled
                )
                | (
                    Self::Verifying,
                    Self::Completed | Self::Failed | Self::Cancelled
                )
                | (Self::Paused | Self::Failed, Self::Queued | Self::Cancelled)
        );

        valid.then_some(next).ok_or(InvalidStateTransition {
            from: self,
            to: next,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub app_name: String,
    pub version: String,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMetadata {
    pub final_url: String,
    pub file_name: String,
    pub content_length: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
}

impl RemoteMetadata {
    #[must_use]
    pub fn validator(&self) -> Option<&str> {
        self.etag.as_deref().or(self.last_modified.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    pub id: DownloadId,
    pub url: String,
    pub destination: String,
    pub file_name: String,
    pub kind: DownloadKind,
    pub state: DownloadState,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub speed_bytes_per_second: u64,
    pub eta_seconds: Option<u64>,
    pub segments: u8,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub sha256: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadKind {
    File,
    Media,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub max_concurrent_downloads: u8,
    pub default_segments: u8,
    pub default_download_directory: String,
    pub theme: Theme,
    pub browser_capture_mode: BrowserCaptureMode,
    pub browser_excluded_sites: Vec<String>,
    pub yt_dlp_path: String,
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserCaptureMode {
    Off,
    Ask,
    Automatic,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 3,
            default_segments: 4,
            default_download_directory: String::new(),
            theme: Theme::System,
            browser_capture_mode: BrowserCaptureMode::Automatic,
            browser_excluded_sites: Vec::new(),
            yt_dlp_path: String::new(),
            ffmpeg_path: String::new(),
            ffprobe_path: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressSnapshot {
    pub id: DownloadId,
    pub state: DownloadState,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed_bytes_per_second: u64,
    pub eta_seconds: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_is_terminal() {
        assert_eq!(
            DownloadState::Completed.transition_to(DownloadState::Queued),
            Err(InvalidStateTransition {
                from: DownloadState::Completed,
                to: DownloadState::Queued,
            })
        );
    }

    #[test]
    fn failed_download_can_be_retried_through_queue() {
        assert_eq!(
            DownloadState::Failed.transition_to(DownloadState::Queued),
            Ok(DownloadState::Queued)
        );
    }
}
