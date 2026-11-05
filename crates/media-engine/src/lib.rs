//! Safe, Tauri-independent media inspection and execution adapters.

use std::{
    collections::BTreeSet,
    net::IpAddr,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use url::{Host, Url};

const MAX_TOOL_OUTPUT: usize = 8 * 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("only public HTTP and HTTPS media URLs are accepted")]
    UnsafeUrl,
    #[error("media inspection requires the user to confirm download authorization")]
    AuthorizationRequired,
    #[error("required media tool is missing or is not an executable file: {0}")]
    ToolUnavailable(String),
    #[error("media tool could not be started: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("media tool timed out")]
    Timeout,
    #[error("media operation was cancelled")]
    Cancelled,
    #[error("media tool output exceeded the safety limit")]
    OutputLimit,
    #[error("media tool failed: {0}")]
    ToolFailed(String),
    #[error("media metadata was invalid: {0}")]
    InvalidMetadata(String),
    #[error("the selected media format is not available")]
    UnknownFormat,
    #[error("the requested output path is unsafe")]
    UnsafeOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Public,
    LoginRequired,
    DrmProtected,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFormat {
    pub id: String,
    pub extension: Option<String>,
    pub container: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub audio_bitrate_kbps: Option<f64>,
    pub estimated_bytes: Option<u64>,
    pub has_video: bool,
    pub has_audio: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub language: String,
    pub name: Option<String>,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInspection {
    pub source_url: String,
    pub website: Option<String>,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_seconds: Option<f64>,
    pub availability: Availability,
    pub restriction_reason: Option<String>,
    pub formats: Vec<MediaFormat>,
    pub subtitles: Vec<SubtitleTrack>,
}

#[derive(Debug, Clone)]
pub struct InspectRequest {
    pub url: String,
    pub authorization_confirmed: bool,
}

#[derive(Debug, Clone)]
pub struct ToolConfig {
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub timeout: Duration,
}

impl ToolConfig {
    #[must_use]
    pub fn from_paths(yt_dlp: PathBuf, ffmpeg: PathBuf, ffprobe: PathBuf) -> Self {
        Self {
            yt_dlp,
            ffmpeg,
            ffprobe,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaOutput {
    Original,
    Mp4,
    #[serde(rename = "webm")]
    WebM,
    Mkv,
    Mp3,
    M4a,
    Aac,
    Opus,
    Flac,
    Wav,
}

#[derive(Debug, Clone)]
pub struct MediaDownloadRequest {
    pub url: String,
    pub format_ids: Vec<String>,
    pub output: MediaOutput,
    pub output_path: PathBuf,
    pub authorization_confirmed: bool,
}

#[async_trait]
pub trait MediaInspector: Send + Sync {
    async fn inspect(
        &self,
        request: &InspectRequest,
        cancellation: CancellationToken,
    ) -> Result<MediaInspection, MediaError>;
}

#[derive(Debug, Clone)]
pub struct YtDlpAdapter {
    tools: ToolConfig,
}

impl YtDlpAdapter {
    /// Creates an adapter after validating all configured executable paths.
    ///
    /// # Errors
    /// Returns an error if a path is absent, relative, or not a regular file.
    pub fn new(tools: ToolConfig) -> Result<Self, MediaError> {
        for path in [&tools.yt_dlp, &tools.ffmpeg, &tools.ffprobe] {
            validate_tool(path)?;
        }
        Ok(Self { tools })
    }

    /// Reports tool versions without executing a shell.
    ///
    /// # Errors
    /// Returns bounded process errors or invalid UTF-8 output.
    pub async fn capabilities(&self) -> Result<ToolCapabilities, MediaError> {
        let cancellation = CancellationToken::new();
        let yt_dlp = run_tool(
            &self.tools.yt_dlp,
            &["--version"],
            self.tools.timeout,
            cancellation.clone(),
        )
        .await?;
        let ffmpeg = run_tool(
            &self.tools.ffmpeg,
            &["-version"],
            self.tools.timeout,
            cancellation.clone(),
        )
        .await?;
        let ffprobe = run_tool(
            &self.tools.ffprobe,
            &["-version"],
            self.tools.timeout,
            cancellation,
        )
        .await?;
        Ok(ToolCapabilities {
            yt_dlp: first_line(&yt_dlp),
            ffmpeg: first_line(&ffmpeg),
            ffprobe: first_line(&ffprobe),
        })
    }

    /// Downloads and post-processes an explicitly selected, inspected format.
    ///
    /// # Errors
    /// Returns validation, tool, cancellation, or timeout errors.
    pub async fn download(
        &self,
        request: &MediaDownloadRequest,
        cancellation: CancellationToken,
    ) -> Result<(), MediaError> {
        ensure_authorized_url(&request.url, request.authorization_confirmed)?;
        validate_output_path(&request.output_path)?;
        if request.format_ids.is_empty() || request.format_ids.iter().any(|id| !valid_format_id(id))
        {
            return Err(MediaError::UnknownFormat);
        }
        let format = request.format_ids.join("+");
        let output = yt_output_template(&request.output_path)?;
        let ffmpeg = self.tools.ffmpeg.to_string_lossy().into_owned();
        let mut owned = vec![
            "--no-playlist".to_owned(),
            "--no-part".to_owned(),
            "--quiet".to_owned(),
            "--no-write-info-json".to_owned(),
            "--ffmpeg-location".to_owned(),
            ffmpeg,
            "--format".to_owned(),
            format,
            "--output".to_owned(),
            output,
        ];
        append_output_args(&mut owned, &request.output);
        owned.push("--".to_owned());
        owned.push(request.url.clone());
        let args = owned.iter().map(String::as_str).collect::<Vec<_>>();
        run_tool(
            &self.tools.yt_dlp,
            &args,
            Duration::from_hours(24),
            cancellation,
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl MediaInspector for YtDlpAdapter {
    async fn inspect(
        &self,
        request: &InspectRequest,
        cancellation: CancellationToken,
    ) -> Result<MediaInspection, MediaError> {
        ensure_authorized_url(&request.url, request.authorization_confirmed)?;
        let output = run_tool(
            &self.tools.yt_dlp,
            &[
                "--dump-single-json",
                "--skip-download",
                "--no-playlist",
                "--no-warnings",
                "--",
                &request.url,
            ],
            self.tools.timeout,
            cancellation,
        )
        .await?;
        parse_inspection(&output, &request.url)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCapabilities {
    pub yt_dlp: String,
    pub ffmpeg: String,
    pub ffprobe: String,
}

fn validate_tool(path: &Path) -> Result<(), MediaError> {
    if !path.is_absolute() || !path.is_file() {
        return Err(MediaError::ToolUnavailable(path.display().to_string()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path
            .metadata()
            .map_err(MediaError::Spawn)?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(MediaError::ToolUnavailable(path.display().to_string()));
        }
    }
    Ok(())
}

fn validate_output_path(path: &Path) -> Result<(), MediaError> {
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(MediaError::UnsafeOutput);
    }
    Ok(())
}

fn yt_output_template(path: &Path) -> Result<String, MediaError> {
    let parent = path.parent().ok_or(MediaError::UnsafeOutput)?;
    let stem = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or(MediaError::UnsafeOutput)?;
    Ok(parent
        .join(format!("{stem}.%(ext)s"))
        .to_string_lossy()
        .into_owned())
}

fn ensure_authorized_url(value: &str, authorized: bool) -> Result<(), MediaError> {
    if !authorized {
        return Err(MediaError::AuthorizationRequired);
    }
    let url = Url::parse(value).map_err(|_| MediaError::UnsafeUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(MediaError::UnsafeUrl);
    }
    if url
        .query_pairs()
        .any(|(name, _)| is_sensitive_query_name(&name))
    {
        return Err(MediaError::UnsafeUrl);
    }
    match url.host() {
        Some(Host::Ipv4(ip)) if !is_global_ipv4(ip) => Err(MediaError::UnsafeUrl),
        Some(Host::Ipv6(ip)) if !is_global_ipv6(ip) => Err(MediaError::UnsafeUrl),
        Some(Host::Domain(name))
            if name.eq_ignore_ascii_case("localhost") || name.ends_with(".localhost") =>
        {
            Err(MediaError::UnsafeUrl)
        }
        Some(_) => Ok(()),
        None => Err(MediaError::UnsafeUrl),
    }
}

fn is_sensitive_query_name(name: &str) -> bool {
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
}

fn is_global_ipv6(ip: std::net::Ipv6Addr) -> bool {
    let addr = IpAddr::V6(ip);
    !(addr.is_loopback()
        || addr.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast())
}

fn is_global_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let [a, b, c, d] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224
        || (a == 255 && b == 255 && c == 255 && d == 255))
}

fn valid_format_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn append_output_args(args: &mut Vec<String>, output: &MediaOutput) {
    match output {
        MediaOutput::Original => {}
        MediaOutput::Mp4 | MediaOutput::WebM | MediaOutput::Mkv => {
            args.push("--merge-output-format".into());
            args.push(
                match output {
                    MediaOutput::Mp4 => "mp4",
                    MediaOutput::WebM => "webm",
                    _ => "mkv",
                }
                .into(),
            );
        }
        _ => {
            args.push("--extract-audio".into());
            args.push("--audio-format".into());
            args.push(
                match output {
                    MediaOutput::Mp3 => "mp3",
                    MediaOutput::M4a => "m4a",
                    MediaOutput::Aac => "aac",
                    MediaOutput::Opus => "opus",
                    MediaOutput::Flac => "flac",
                    _ => "wav",
                }
                .into(),
            );
        }
    }
}

async fn run_tool(
    path: &Path,
    args: &[&str],
    limit: Duration,
    cancellation: CancellationToken,
) -> Result<Vec<u8>, MediaError> {
    let mut child = Command::new(path)
        .args(args)
        .env_clear()
        .env("PATH", path.parent().unwrap_or_else(|| Path::new("")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| MediaError::InvalidMetadata("stdout unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| MediaError::InvalidMetadata("stderr unavailable".into()))?;
    let stdout_task = tokio::spawn(read_bounded(stdout));
    let stderr_task = tokio::spawn(read_bounded(stderr));
    let status = wait_child(&mut child, limit, cancellation).await?;
    let stdout = stdout_task
        .await
        .map_err(|error| MediaError::InvalidMetadata(error.to_string()))??;
    let stderr = stderr_task
        .await
        .map_err(|error| MediaError::InvalidMetadata(error.to_string()))??;
    if !status.success() {
        drop(stderr);
        return Err(MediaError::ToolFailed(format!(
            "process exited with status {} (details withheld to protect source credentials)",
            status
                .code()
                .map_or_else(|| "signal".into(), |code| code.to_string())
        )));
    }
    Ok(stdout)
}

async fn read_bounded<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
) -> Result<Vec<u8>, MediaError> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    let mut exceeded = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        if !exceeded && bytes.len().saturating_add(read) <= MAX_TOOL_OUTPUT {
            bytes.extend_from_slice(&buffer[..read]);
        } else {
            exceeded = true;
        }
    }
    if exceeded {
        Err(MediaError::OutputLimit)
    } else {
        Ok(bytes)
    }
}

async fn wait_child(
    child: &mut Child,
    limit: Duration,
    cancellation: CancellationToken,
) -> Result<std::process::ExitStatus, MediaError> {
    tokio::select! {
        () = cancellation.cancelled() => { let _ = child.kill().await; Err(MediaError::Cancelled) }
        result = timeout(limit, child.wait()) => if let Ok(status) = result {
            status.map_err(MediaError::Spawn)
        } else {
            let _ = child.kill().await;
            Err(MediaError::Timeout)
        }
    }
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .next()
        .unwrap_or("unknown")
        .chars()
        .take(160)
        .collect()
}

#[derive(Debug, Deserialize)]
struct RawInspection {
    title: Option<String>,
    extractor_key: Option<String>,
    thumbnail: Option<String>,
    duration: Option<f64>,
    availability: Option<String>,
    formats: Option<Vec<RawFormat>>,
    subtitles: Option<serde_json::Map<String, serde_json::Value>>,
    has_drm: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawFormat {
    format_id: Option<String>,
    ext: Option<String>,
    container: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    vcodec: Option<String>,
    acodec: Option<String>,
    abr: Option<f64>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
}

fn parse_inspection(bytes: &[u8], fallback_url: &str) -> Result<MediaInspection, MediaError> {
    let raw: RawInspection = serde_json::from_slice(bytes)
        .map_err(|error| MediaError::InvalidMetadata(error.to_string()))?;
    let title = raw
        .title
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| MediaError::InvalidMetadata("title is missing".into()))?;
    let formats = raw
        .formats
        .unwrap_or_default()
        .into_iter()
        .filter_map(|format| {
            let id = format.format_id.filter(|id| valid_format_id(id))?;
            let video = format
                .vcodec
                .as_deref()
                .is_some_and(|codec| codec != "none");
            let audio = format
                .acodec
                .as_deref()
                .is_some_and(|codec| codec != "none");
            (video || audio).then_some(MediaFormat {
                id,
                extension: format.ext,
                container: format.container,
                width: format.width,
                height: format.height,
                frame_rate: format.fps,
                video_codec: format.vcodec.filter(|v| v != "none"),
                audio_codec: format.acodec.filter(|v| v != "none"),
                audio_bitrate_kbps: format.abr,
                estimated_bytes: format.filesize.or(format.filesize_approx),
                has_video: video,
                has_audio: audio,
            })
        })
        .collect::<Vec<_>>();
    if formats.is_empty() {
        return Err(MediaError::InvalidMetadata(
            "no downloadable formats were reported".into(),
        ));
    }
    let availability = if raw.has_drm == Some(true) {
        Availability::DrmProtected
    } else {
        match raw.availability.as_deref() {
            Some("public") | None => Availability::Public,
            Some("needs_auth" | "premium_only" | "subscriber_only" | "needs_subscription") => {
                Availability::LoginRequired
            }
            Some("private" | "unlisted") => Availability::Unavailable,
            _ => Availability::Unknown,
        }
    };
    let subtitles = raw
        .subtitles
        .unwrap_or_default()
        .into_iter()
        .map(|(language, entries)| {
            let extensions = entries
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.get("ext")?.as_str().map(str::to_owned))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            SubtitleTrack {
                language,
                name: None,
                extensions,
            }
        })
        .collect();
    Ok(MediaInspection {
        source_url: fallback_url.to_owned(),
        website: raw.extractor_key,
        title,
        thumbnail_url: raw.thumbnail,
        duration_seconds: raw.duration,
        availability,
        restriction_reason: None,
        formats,
        subtitles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actual_formats_without_inventing_qualities() {
        let json = br#"{"title":"Licensed sample","webpage_url":"https://media.example/watch/1","extractor_key":"Generic","duration":12.5,"availability":"public","formats":[{"format_id":"v720","ext":"mp4","width":1280,"height":720,"fps":30,"vcodec":"avc1","acodec":"none","filesize":1000},{"format_id":"a1","ext":"m4a","vcodec":"none","acodec":"mp4a","abr":128}],"subtitles":{"en":[{"ext":"vtt"}]}}"#;
        let result = parse_inspection(json, "https://fallback.example").expect("metadata");
        assert_eq!(result.formats.len(), 2);
        assert!(result.formats[0].has_video);
        assert!(!result.formats[0].has_audio);
        assert_eq!(result.subtitles[0].extensions, vec!["vtt"]);
    }

    #[test]
    fn rejects_private_and_credentialed_urls() {
        for url in [
            "http://127.0.0.1/a",
            "http://[::1]/a",
            "https://user:secret@example.com/a",
            "https://media.example/a?token=secret",
            "file:///tmp/a",
        ] {
            assert!(matches!(
                ensure_authorized_url(url, true),
                Err(MediaError::UnsafeUrl)
            ));
        }
        assert!(ensure_authorized_url("https://media.example/a", true).is_ok());
    }

    #[test]
    fn requires_explicit_authorization() {
        assert!(matches!(
            ensure_authorized_url("https://media.example/a", false),
            Err(MediaError::AuthorizationRequired)
        ));
    }

    #[test]
    fn rejects_drm_only_metadata_without_formats() {
        let json = br#"{"title":"Protected","has_drm":true,"formats":[]}"#;
        assert!(parse_inspection(json, "https://example.com").is_err());
    }
}
