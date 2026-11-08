//! Safe, Tauri-independent media inspection and execution adapters.

use std::{
    collections::BTreeSet,
    net::IpAddr,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
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
    #[error("thumbnail was rejected by security policy")]
    ThumbnailRejected,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaStage {
    Downloading,
    PostProcessing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaProgress {
    pub stage: MediaStage,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub speed_bytes_per_second: Option<u64>,
    pub eta_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureThumbnail {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Fetches a public HTTPS image without forwarding credentials or following
/// unrestricted redirects.
///
/// # Errors
/// Returns [`MediaError::ThumbnailRejected`] for unsafe locations, types, sizes,
/// redirects, DNS results, or HTTP failures.
pub async fn fetch_secure_thumbnail(value: &str) -> Result<SecureThumbnail, MediaError> {
    const MAX_BYTES: usize = 3 * 1024 * 1024;
    const MAX_REDIRECTS: usize = 2;
    let mut url = Url::parse(value).map_err(|_| MediaError::ThumbnailRejected)?;
    for redirect_count in 0..=MAX_REDIRECTS {
        validate_thumbnail_url(&url)?;
        let host = url
            .host_str()
            .ok_or(MediaError::ThumbnailRejected)?
            .to_owned();
        let port = url
            .port_or_known_default()
            .ok_or(MediaError::ThumbnailRejected)?;
        let addresses = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|_| MediaError::ThumbnailRejected)?
            .collect::<Vec<_>>();
        if addresses.is_empty() || addresses.iter().any(|address| !is_global_ip(address.ip())) {
            return Err(MediaError::ThumbnailRejected);
        }
        let mut builder = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(12))
            .user_agent("GrabBit/0.1 thumbnail-fetcher");
        if matches!(url.host(), Some(Host::Domain(_))) {
            builder = builder.resolve(&host, addresses[0]);
        }
        let response = builder
            .build()
            .map_err(|_| MediaError::ThumbnailRejected)?
            .get(url.clone())
            .send()
            .await
            .map_err(|_| MediaError::ThumbnailRejected)?;
        if response.status().is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err(MediaError::ThumbnailRejected);
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|header| header.to_str().ok())
                .ok_or(MediaError::ThumbnailRejected)?;
            url = url
                .join(location)
                .map_err(|_| MediaError::ThumbnailRejected)?;
            continue;
        }
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > MAX_BYTES as u64)
        {
            return Err(MediaError::ThumbnailRejected);
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|header| header.to_str().ok())
            .and_then(|value| value.split(';').next())
            .filter(|value| {
                matches!(
                    *value,
                    "image/jpeg" | "image/png" | "image/webp" | "image/gif"
                )
            })
            .ok_or(MediaError::ThumbnailRejected)?
            .to_owned();
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| MediaError::ThumbnailRejected)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
                return Err(MediaError::ThumbnailRejected);
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(SecureThumbnail {
            content_type,
            bytes,
        });
    }
    Err(MediaError::ThumbnailRejected)
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
        progress: Option<mpsc::Sender<MediaProgress>>,
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
            "--no-warnings".to_owned(),
            "--newline".to_owned(),
            "--progress-delta".to_owned(),
            "0.25".to_owned(),
            "--progress-template".to_owned(),
            "download:grabbit-progress|download|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s".to_owned(),
            "--progress-template".to_owned(),
            "postprocess:grabbit-progress|postprocess|NA|NA|NA|NA|NA".to_owned(),
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
        run_download_tool(
            &self.tools.yt_dlp,
            &args,
            Duration::from_hours(24),
            cancellation,
            progress,
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

fn validate_thumbnail_url(url: &Url) -> Result<(), MediaError> {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url
            .query_pairs()
            .any(|(name, _)| is_sensitive_query_name(&name))
    {
        return Err(MediaError::ThumbnailRejected);
    }
    match url.host() {
        Some(Host::Ipv4(ip)) if is_global_ipv4(ip) => Ok(()),
        Some(Host::Ipv6(ip)) if is_global_ipv6(ip) => Ok(()),
        Some(Host::Domain(name))
            if !name.eq_ignore_ascii_case("localhost") && !name.ends_with(".localhost") =>
        {
            Ok(())
        }
        _ => Err(MediaError::ThumbnailRejected),
    }
}

fn is_global_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_global_ipv4(ip),
        IpAddr::V6(ip) => is_global_ipv6(ip),
    }
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

async fn run_download_tool(
    path: &Path,
    args: &[&str],
    limit: Duration,
    cancellation: CancellationToken,
    progress: Option<mpsc::Sender<MediaProgress>>,
) -> Result<(), MediaError> {
    let mut child = restricted_command(path, args).spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| MediaError::InvalidMetadata("stdout unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| MediaError::InvalidMetadata("stderr unavailable".into()))?;
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut observed = 0_usize;
        while let Some(line) = lines.next_line().await? {
            observed = observed.saturating_add(line.len());
            if observed > MAX_TOOL_OUTPUT {
                return Err(MediaError::OutputLimit);
            }
            if let Some(event) = parse_progress_line(&line)
                && let Some(sender) = &progress
            {
                let _ = sender.send(event).await;
            }
        }
        Ok::<(), MediaError>(())
    });
    let stderr_task = tokio::spawn(read_bounded(stderr));
    let status = wait_child(&mut child, limit, cancellation).await?;
    stdout_task
        .await
        .map_err(|error| MediaError::InvalidMetadata(error.to_string()))??;
    let stderr = stderr_task
        .await
        .map_err(|error| MediaError::InvalidMetadata(error.to_string()))??;
    if status.success() {
        Ok(())
    } else {
        drop(stderr);
        Err(tool_exit_error(status))
    }
}

fn parse_progress_line(line: &str) -> Option<MediaProgress> {
    let fields = line
        .strip_prefix("grabbit-progress|")?
        .split('|')
        .collect::<Vec<_>>();
    if fields.len() != 6 {
        return None;
    }
    let stage = match fields[0] {
        "download" => MediaStage::Downloading,
        "postprocess" => MediaStage::PostProcessing,
        _ => return None,
    };
    Some(MediaProgress {
        stage,
        downloaded_bytes: parse_progress_number(fields[1]),
        total_bytes: parse_progress_number(fields[2]).or_else(|| parse_progress_number(fields[3])),
        speed_bytes_per_second: parse_progress_number(fields[4]),
        eta_seconds: parse_progress_number(fields[5]),
    })
}

fn parse_progress_number(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() || matches!(value, "NA" | "None") {
        return None;
    }
    value.parse::<f64>().ok().and_then(|number| {
        (number.is_finite() && number >= 0.0)
            .then(|| format!("{number:.0}").parse::<u64>().ok())
            .flatten()
    })
}

async fn run_tool(
    path: &Path,
    args: &[&str],
    limit: Duration,
    cancellation: CancellationToken,
) -> Result<Vec<u8>, MediaError> {
    let mut child = restricted_command(path, args).spawn()?;
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
        return Err(tool_exit_error(status));
    }
    Ok(stdout)
}

fn restricted_command(path: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(path);
    command
        .args(args)
        .env_clear()
        .env("PATH", path.parent().unwrap_or_else(|| Path::new("")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    command
}

fn tool_exit_error(status: std::process::ExitStatus) -> MediaError {
    MediaError::ToolFailed(format!(
        "process exited with status {} (details withheld to protect source credentials)",
        status
            .code()
            .map_or_else(|| "signal".into(), |code| code.to_string())
    ))
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
        () = cancellation.cancelled() => { terminate_child(child).await; Err(MediaError::Cancelled) }
        result = timeout(limit, child.wait()) => if let Ok(status) = result {
            status.map_err(MediaError::Spawn)
        } else {
            terminate_child(child).await;
            Err(MediaError::Timeout)
        }
    }
}

async fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    if let Some(id) = child.id()
        && let Ok(pid) = i32::try_from(id)
    {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
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
    format_id: Option<String>,
    ext: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    vcodec: Option<String>,
    acodec: Option<String>,
    abr: Option<f64>,
    filesize: Option<u64>,
    subtitles: Option<serde_json::Map<String, serde_json::Value>>,
    has_drm: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawFormat {
    format_id: Option<String>,
    ext: Option<String>,
    container: Option<String>,
    audio_ext: Option<String>,
    video_ext: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
    vcodec: Option<String>,
    acodec: Option<String>,
    abr: Option<f64>,
    asr: Option<u32>,
    audio_channels: Option<u32>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
}

fn parse_inspection(bytes: &[u8], fallback_url: &str) -> Result<MediaInspection, MediaError> {
    let raw: RawInspection = serde_json::from_slice(bytes)
        .map_err(|error| MediaError::InvalidMetadata(error.to_string()))?;
    let formats = inspection_formats(&raw)
        .into_iter()
        .filter_map(parse_format)
        .collect::<Vec<_>>();
    let title = raw
        .title
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| MediaError::InvalidMetadata("title is missing".into()))?;
    if formats.is_empty() {
        if raw.has_drm == Some(true) {
            return Err(MediaError::InvalidMetadata(
                "the source is DRM-protected and has no downloadable formats".into(),
            ));
        }
        return Err(MediaError::InvalidMetadata(
            "the extractor returned no recognizable audio or video streams; update yt-dlp and verify the source is publicly accessible and downloadable".into(),
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

fn inspection_formats(raw: &RawInspection) -> Vec<RawFormat> {
    let mut formats = raw.formats.clone().unwrap_or_default();
    if formats.is_empty()
        && let Some(format_id) = raw.format_id.clone()
    {
        formats.push(RawFormat {
            format_id: Some(format_id),
            ext: raw.ext.clone(),
            container: None,
            audio_ext: None,
            video_ext: None,
            width: raw.width,
            height: raw.height,
            fps: raw.fps,
            vcodec: raw.vcodec.clone(),
            acodec: raw.acodec.clone(),
            abr: raw.abr,
            asr: None,
            audio_channels: None,
            filesize: raw.filesize,
            filesize_approx: None,
        });
    }
    formats
}

fn parse_format(format: RawFormat) -> Option<MediaFormat> {
    let id = format.format_id.filter(|id| valid_format_id(id))?;
    let video_codec = format.vcodec.filter(|codec| has_media_codec(codec));
    let audio_codec = format.acodec.filter(|codec| has_media_codec(codec));
    let video = video_codec.is_some()
        || format.width.is_some()
        || format.height.is_some()
        || format.video_ext.as_deref().is_some_and(has_media_codec);
    let audio = audio_codec.is_some()
        || format.audio_ext.as_deref().is_some_and(has_media_codec)
        || (!video
            && (format.abr.is_some() || format.asr.is_some() || format.audio_channels.is_some()));
    (video || audio).then_some(MediaFormat {
        id,
        extension: format.ext,
        container: format.container,
        width: format.width,
        height: format.height,
        frame_rate: format.fps,
        video_codec,
        audio_codec,
        audio_bitrate_kbps: format.abr,
        estimated_bytes: format.filesize.or(format.filesize_approx),
        has_video: video,
        has_audio: audio,
    })
}

fn has_media_codec(value: &str) -> bool {
    !value.trim().is_empty() && !value.eq_ignore_ascii_case("none")
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
    fn recognizes_stream_type_from_dimensions_and_extractor_extensions() {
        let json = br#"{"title":"Modern metadata","formats":[{"format_id":"video-720","ext":"mp4","width":1280,"height":720,"video_ext":"mp4","audio_ext":"none"},{"format_id":"audio-128","ext":"m4a","audio_ext":"m4a","video_ext":"none","abr":128}]}"#;

        let result = parse_inspection(json, "https://media.example/watch/1").expect("metadata");

        assert_eq!(result.formats.len(), 2);
        assert!(result.formats[0].has_video);
        assert!(!result.formats[0].has_audio);
        assert!(!result.formats[1].has_video);
        assert!(result.formats[1].has_audio);
        assert_eq!(result.formats[0].video_codec, None);
    }

    #[test]
    fn accepts_single_format_metadata_without_a_formats_array() {
        let json = br#"{"title":"Direct media","format_id":"source","ext":"mp4","width":1920,"height":1080,"vcodec":"NONE"}"#;

        let result = parse_inspection(json, "https://media.example/video.mp4").expect("metadata");

        assert_eq!(result.formats.len(), 1);
        assert_eq!(result.formats[0].id, "source");
        assert!(result.formats[0].has_video);
        assert_eq!(result.formats[0].video_codec, None);
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
        let error = parse_inspection(json, "https://example.com").expect_err("no media formats");
        assert!(error.to_string().contains("DRM-protected"));
    }

    #[test]
    fn explains_when_extractor_reports_no_media_streams() {
        let json = br#"{"title":"Unavailable","formats":[]}"#;
        let error = parse_inspection(json, "https://example.com").expect_err("no media formats");
        assert!(error.to_string().contains("update yt-dlp"));
    }

    #[test]
    fn parses_delimited_progress_without_inventing_unknown_values() {
        let event = parse_progress_line("grabbit-progress|download|1048576|2097152|NA|524288|2")
            .expect("progress");
        assert_eq!(event.stage, MediaStage::Downloading);
        assert_eq!(event.downloaded_bytes, Some(1_048_576));
        assert_eq!(event.total_bytes, Some(2_097_152));
        assert_eq!(event.speed_bytes_per_second, Some(524_288));
        assert_eq!(event.eta_seconds, Some(2));

        let unknown = parse_progress_line("grabbit-progress|postprocess|NA|NA|NA|NA|NA")
            .expect("postprocess");
        assert_eq!(unknown.stage, MediaStage::PostProcessing);
        assert_eq!(unknown.total_bytes, None);
    }

    #[test]
    fn thumbnail_policy_requires_public_credential_free_https() {
        for value in [
            "http://example.com/image.jpg",
            "https://127.0.0.1/image.jpg",
            "https://user:secret@example.com/image.jpg",
            "https://example.com/image.jpg?token=secret",
        ] {
            let url = Url::parse(value).expect("url");
            assert!(matches!(
                validate_thumbnail_url(&url),
                Err(MediaError::ThumbnailRejected)
            ));
        }
        assert!(
            validate_thumbnail_url(
                &Url::parse("https://images.example.com/preview.webp").expect("url")
            )
            .is_ok()
        );
    }
}
