//! Byte-accurate HTTP/HTTPS metadata and payload transport.
//!
//! Queue policy, database checkpoints, and destination authorization belong to
//! higher layers; this crate owns HTTP and range validation.

use std::{path::Path, sync::Arc, time::Duration};

use download_common::RemoteMetadata;
use futures_util::StreamExt;
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{
        ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
        HeaderMap, IF_RANGE, LAST_MODIFIED, RANGE,
    },
    redirect,
};
use thiserror::Error;
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::{Semaphore, mpsc},
    time::sleep,
};
use tokio_util::sync::CancellationToken;

const MAX_REDIRECTS: usize = 10;
const MAX_SEGMENTS: u8 = 32;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub retry_limit: u8,
    pub retry_base_delay: Duration,
    pub max_concurrent_segments: u8,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            request_timeout: Duration::from_secs(60),
            retry_limit: 3,
            retry_base_delay: Duration::from_millis(250),
            max_concurrent_segments: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    BytesWritten(u64),
    SegmentCheckpoint { index: u8, downloaded: u64 },
}

#[derive(Debug, Clone)]
pub struct DownloadRequest<'a> {
    pub url: &'a str,
    pub path: &'a Path,
    pub metadata: &'a RemoteMetadata,
    pub segments: u8,
    pub resume_from: u64,
    /// Durable bytes already written for each segment, in segment-index order.
    pub segment_offsets: &'a [u64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadResult {
    pub bytes_written: u64,
    pub segmented: bool,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("only HTTP and HTTPS URLs are allowed")]
    UnsupportedScheme,
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("HTTP status {0}")]
    HttpStatus(StatusCode),
    #[error("invalid Content-Range header")]
    InvalidContentRange,
    #[error("server ignored a byte-range request")]
    RangeIgnored,
    #[error("remote resource changed while downloading")]
    ResourceChanged,
    #[error("response length did not match the expected length")]
    LengthMismatch,
    #[error("download was cancelled")]
    Cancelled,
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("segment task failed: {0}")]
    Task(String),
}

#[derive(Clone)]
pub struct HttpEngine {
    client: Client,
    config: EngineConfig,
}

impl std::fmt::Debug for HttpEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpEngine")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl HttpEngine {
    /// Creates an engine with TLS verification enabled and restricted redirects.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be constructed.
    pub fn new(config: EngineConfig) -> Result<Self, EngineError> {
        let client = Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .user_agent(concat!("GrabBit/", env!("CARGO_PKG_VERSION")))
            .redirect(redirect::Policy::custom(|attempt| {
                if !matches!(attempt.url().scheme(), "http" | "https") {
                    return attempt.error("redirected to a non-HTTP URL");
                }
                if attempt.previous().len() >= MAX_REDIRECTS {
                    return attempt.error("too many redirects");
                }
                attempt.follow()
            }))
            .build()
            .map_err(|error| EngineError::Request(error.to_string()))?;
        Ok(Self { client, config })
    }

    /// Parses a user URL and restricts it to HTTP(S) without URL credentials.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed URLs, credentials, or unsupported schemes.
    pub fn validate_url(value: &str) -> Result<Url, EngineError> {
        let url = Url::parse(value).map_err(|error| EngineError::InvalidUrl(error.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(EngineError::UnsupportedScheme);
        }
        if !url.username().is_empty() || url.password().is_some() || url.host_str().is_none() {
            return Err(EngineError::InvalidUrl(
                "credentials and missing hosts are not allowed".to_owned(),
            ));
        }
        Ok(url)
    }

    /// Probes remote metadata using HEAD with a bounded GET fallback.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid URLs, transport failures, or non-success status.
    pub async fn probe(&self, value: &str) -> Result<RemoteMetadata, EngineError> {
        let url = Self::validate_url(value)?;
        let head = self.send_with_retry(|| self.client.head(url.clone())).await;
        let response = match head {
            Ok(response)
                if response.status().is_success()
                    && (response.headers().contains_key(CONTENT_LENGTH)
                        || response.headers().contains_key(ACCEPT_RANGES)) =>
            {
                response
            }
            _ => {
                let response = self
                    .send_with_retry(|| self.client.get(url.clone()).header(RANGE, "bytes=0-0"))
                    .await?;
                ensure_success_or_partial(&response)?;
                response
            }
        };
        ensure_success_or_partial(&response)?;
        metadata_from_response(&response)
    }

    /// Streams a resource into a partial file, using ranges when safe.
    ///
    /// # Errors
    ///
    /// Returns an error for cancellation, protocol violations, changed
    /// resources, short writes, or filesystem failures.
    pub async fn download(
        &self,
        request: DownloadRequest<'_>,
        cancellation: CancellationToken,
        events: Option<mpsc::UnboundedSender<EngineEvent>>,
    ) -> Result<DownloadResult, EngineError> {
        let url = Self::validate_url(request.url)?;
        let segments = request.segments.clamp(1, MAX_SEGMENTS);
        if segments > 1
            && request.resume_from == 0
            && request.metadata.supports_ranges
            && request
                .metadata
                .content_length
                .is_some_and(|length| length > u64::from(segments))
        {
            match self
                .download_segmented(
                    url.clone(),
                    request.path,
                    request.metadata,
                    segments,
                    request.segment_offsets,
                    cancellation.clone(),
                    events.clone(),
                )
                .await
            {
                Ok(bytes_written) => {
                    return Ok(DownloadResult {
                        bytes_written,
                        segmented: true,
                    });
                }
                Err(EngineError::RangeIgnored | EngineError::InvalidContentRange) => {
                    tracing::warn!("range transfer rejected; falling back to sequential mode");
                    if let Err(error) = tokio::fs::remove_file(request.path).await
                        && error.kind() != std::io::ErrorKind::NotFound
                    {
                        return Err(EngineError::Io(error));
                    }
                }
                Err(error) => return Err(error),
            }
        }

        let bytes_written = self
            .download_sequential(
                url,
                request.path,
                request.metadata,
                request.resume_from,
                cancellation,
                events,
            )
            .await?;
        Ok(DownloadResult {
            bytes_written,
            segmented: false,
        })
    }

    async fn download_sequential(
        &self,
        url: Url,
        path: &Path,
        metadata: &RemoteMetadata,
        resume_from: u64,
        cancellation: CancellationToken,
        events: Option<mpsc::UnboundedSender<EngineEvent>>,
    ) -> Result<u64, EngineError> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(resume_from == 0)
            .open(path)
            .await?;
        if resume_from > 0 {
            file.seek(SeekFrom::Start(resume_from)).await?;
        }

        let mut offset = resume_from;
        let mut stream_attempt = 0;
        loop {
            if cancellation.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            let response = self
                .send_with_retry(|| {
                    let mut builder = self.client.get(url.clone());
                    if offset > 0 {
                        builder = builder.header(RANGE, format!("bytes={offset}-"));
                        if let Some(validator) = metadata.validator() {
                            builder = builder.header(IF_RANGE, validator);
                        }
                    }
                    builder
                })
                .await?;
            if offset > 0 {
                validate_range_response(&response, offset, None)?;
            } else {
                ensure_success(&response)?;
            }
            validate_identity(&response, metadata)?;

            let mut stream = response.bytes_stream();
            let mut failure = None;
            while let Some(next) = tokio::select! {
                () = cancellation.cancelled() => {
                    file.flush().await?;
                    file.sync_data().await?;
                    return Err(EngineError::Cancelled);
                },
                next = stream.next() => next,
            } {
                match next {
                    Ok(bytes) => {
                        file.write_all(&bytes).await?;
                        let count =
                            u64::try_from(bytes.len()).map_err(|_| EngineError::LengthMismatch)?;
                        offset = offset
                            .checked_add(count)
                            .ok_or(EngineError::LengthMismatch)?;
                        if let Some(sender) = &events {
                            let _unused = sender.send(EngineEvent::BytesWritten(count));
                        }
                    }
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            if failure.is_none() {
                break;
            }
            if stream_attempt >= self.config.retry_limit || !metadata.supports_ranges {
                return Err(EngineError::Request(failure.map_or_else(
                    || "stream failed".to_owned(),
                    |error| error.to_string(),
                )));
            }
            stream_attempt += 1;
            file.flush().await?;
            sleep(self.retry_delay(stream_attempt)).await;
        }

        file.flush().await?;
        file.sync_data().await?;
        if metadata
            .content_length
            .is_some_and(|expected| offset != expected)
        {
            return Err(EngineError::LengthMismatch);
        }
        Ok(offset.saturating_sub(resume_from))
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_segmented(
        &self,
        url: Url,
        path: &Path,
        metadata: &RemoteMetadata,
        segments: u8,
        offsets: &[u64],
        cancellation: CancellationToken,
        events: Option<mpsc::UnboundedSender<EngineEvent>>,
    ) -> Result<u64, EngineError> {
        let total = metadata.content_length.ok_or(EngineError::LengthMismatch)?;
        let resumed = offsets.iter().any(|offset| *offset > 0);
        if resumed {
            if File::open(path).await?.metadata().await?.len() != total {
                return Err(EngineError::LengthMismatch);
            }
        } else {
            let file = File::create(path).await?;
            file.set_len(total).await?;
            file.sync_data().await?;
            drop(file);
        }

        let semaphore = Arc::new(Semaphore::new(usize::from(
            self.config.max_concurrent_segments.max(1),
        )));
        let mut tasks = Vec::with_capacity(usize::from(segments));
        for index in 0..segments {
            let start = total * u64::from(index) / u64::from(segments);
            let end = total * (u64::from(index) + 1) / u64::from(segments) - 1;
            let durable = offsets.get(usize::from(index)).copied().unwrap_or(0);
            let engine = self.clone();
            let url = url.clone();
            let path = path.to_path_buf();
            let metadata = metadata.clone();
            let cancellation = cancellation.clone();
            let events = events.clone();
            let semaphore = semaphore.clone();
            tasks.push(tokio::spawn(async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .map_err(|error| EngineError::Task(error.to_string()))?;
                engine
                    .download_segment(
                        index,
                        url,
                        &path,
                        &metadata,
                        start,
                        end,
                        durable,
                        cancellation,
                        events,
                    )
                    .await
            }));
        }

        let mut written = offsets.iter().copied().sum::<u64>();
        let mut first_error = None;
        for task in tasks {
            let result = task
                .await
                .map_err(|error| EngineError::Task(error.to_string()))
                .and_then(std::convert::identity);
            match result {
                Ok(bytes) => {
                    if let Some(total_written) = written.checked_add(bytes) {
                        written = total_written;
                    } else if first_error.is_none() {
                        first_error = Some(EngineError::LengthMismatch);
                    }
                }
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        if written != total {
            return Err(EngineError::LengthMismatch);
        }
        let file = OpenOptions::new().write(true).open(path).await?;
        file.sync_data().await?;
        Ok(written)
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_segment(
        &self,
        index: u8,
        url: Url,
        path: &Path,
        metadata: &RemoteMetadata,
        start: u64,
        end: u64,
        durable: u64,
        cancellation: CancellationToken,
        events: Option<mpsc::UnboundedSender<EngineEvent>>,
    ) -> Result<u64, EngineError> {
        let expected = end - start + 1;
        if durable > expected {
            return Err(EngineError::LengthMismatch);
        }
        if durable == expected {
            return Ok(0);
        }
        let request_start = start + durable;
        let mut attempt = 0;
        loop {
            if cancellation.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            let mut builder = self
                .client
                .get(url.clone())
                .header(RANGE, format!("bytes={request_start}-{end}"));
            if let Some(validator) = metadata.validator() {
                builder = builder.header(IF_RANGE, validator);
            }
            match builder.send().await {
                Ok(response) => {
                    validate_range_response(&response, request_start, Some(end))?;
                    validate_identity(&response, metadata)?;
                    let mut file = OpenOptions::new().write(true).open(path).await?;
                    file.seek(SeekFrom::Start(request_start)).await?;
                    let mut stream = response.bytes_stream();
                    let mut written = 0_u64;
                    let mut failure = None;
                    while let Some(next) = tokio::select! {
                        () = cancellation.cancelled() => {
                            file.flush().await?;
                            file.sync_data().await?;
                            if let Some(sender) = &events {
                                let _unused = sender.send(EngineEvent::SegmentCheckpoint {
                                    index,
                                    downloaded: durable + written,
                                });
                            }
                            return Err(EngineError::Cancelled);
                        },
                        next = stream.next() => next,
                    } {
                        match next {
                            Ok(bytes) => {
                                file.write_all(&bytes).await?;
                                let count = u64::try_from(bytes.len())
                                    .map_err(|_| EngineError::LengthMismatch)?;
                                written = written
                                    .checked_add(count)
                                    .ok_or(EngineError::LengthMismatch)?;
                                if let Some(sender) = &events {
                                    let _unused = sender.send(EngineEvent::BytesWritten(count));
                                }
                            }
                            Err(error) => {
                                failure = Some(error);
                                break;
                            }
                        }
                    }
                    let remaining = end - request_start + 1;
                    if failure.is_none() && written == remaining {
                        file.flush().await?;
                        file.sync_data().await?;
                        if let Some(sender) = &events {
                            let _unused = sender.send(EngineEvent::SegmentCheckpoint {
                                index,
                                downloaded: durable + written,
                            });
                        }
                        return Ok(written);
                    }
                    if failure.is_none() {
                        return Err(EngineError::LengthMismatch);
                    }
                }
                Err(error) if attempt >= self.config.retry_limit => {
                    return Err(EngineError::Request(error.to_string()));
                }
                Err(error) => {
                    tracing::warn!(segment = index, attempt, error = %error, "retrying segment");
                }
            }
            if attempt >= self.config.retry_limit {
                return Err(EngineError::Request("segment stream failed".to_owned()));
            }
            attempt += 1;
            sleep(self.retry_delay(attempt)).await;
        }
    }

    async fn send_with_retry<F>(&self, mut request: F) -> Result<Response, EngineError>
    where
        F: FnMut() -> reqwest::RequestBuilder,
    {
        let mut attempt = 0;
        loop {
            match request().send().await {
                Ok(response)
                    if (response.status() == StatusCode::TOO_MANY_REQUESTS
                        || response.status().is_server_error())
                        && attempt < self.config.retry_limit =>
                {
                    attempt += 1;
                    sleep(self.retry_delay(attempt)).await;
                }
                Ok(response) => return Ok(response),
                Err(error) if attempt < self.config.retry_limit => {
                    attempt += 1;
                    tracing::warn!(attempt, error = %error, "retrying HTTP request");
                    sleep(self.retry_delay(attempt)).await;
                }
                Err(error) => return Err(EngineError::Request(error.to_string())),
            }
        }
    }

    fn retry_delay(&self, attempt: u8) -> Duration {
        self.config
            .retry_base_delay
            .saturating_mul(2_u32.saturating_pow(u32::from(attempt.saturating_sub(1))))
    }
}

fn ensure_success(response: &Response) -> Result<(), EngineError> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(EngineError::HttpStatus(response.status()))
    }
}

fn ensure_success_or_partial(response: &Response) -> Result<(), EngineError> {
    if response.status().is_success() || response.status() == StatusCode::PARTIAL_CONTENT {
        Ok(())
    } else {
        Err(EngineError::HttpStatus(response.status()))
    }
}

fn metadata_from_response(response: &Response) -> Result<RemoteMetadata, EngineError> {
    let headers = response.headers();
    let range = headers
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range);
    if response.status() == StatusCode::PARTIAL_CONTENT && range.is_none() {
        return Err(EngineError::InvalidContentRange);
    }
    let content_length = range
        .and_then(|(_, _, total)| total)
        .or_else(|| header_u64(headers, CONTENT_LENGTH));
    let supports_ranges = response.status() == StatusCode::PARTIAL_CONTENT
        || headers
            .get(ACCEPT_RANGES)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("bytes"));
    let final_url = response.url().clone();
    Ok(RemoteMetadata {
        file_name: safe_filename(headers, &final_url),
        final_url: final_url.to_string(),
        content_length,
        supports_ranges,
        etag: strong_etag(headers),
        last_modified: header_string(headers, LAST_MODIFIED),
        content_type: header_string(headers, CONTENT_TYPE),
    })
}

fn validate_range_response(
    response: &Response,
    expected_start: u64,
    expected_end: Option<u64>,
) -> Result<(), EngineError> {
    if response.status() == StatusCode::OK {
        return Err(EngineError::RangeIgnored);
    }
    if response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(EngineError::HttpStatus(response.status()));
    }
    let (start, end, _) = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)
        .ok_or(EngineError::InvalidContentRange)?;
    if start != expected_start || expected_end.is_some_and(|expected| end != expected) {
        return Err(EngineError::InvalidContentRange);
    }
    Ok(())
}

fn validate_identity(response: &Response, expected: &RemoteMetadata) -> Result<(), EngineError> {
    if let (Some(expected_etag), Some(actual)) =
        (expected.etag.as_deref(), strong_etag(response.headers()))
        && expected_etag != actual
    {
        return Err(EngineError::ResourceChanged);
    }
    if expected.etag.is_none()
        && let (Some(expected_date), Some(actual)) = (
            expected.last_modified.as_deref(),
            header_string(response.headers(), LAST_MODIFIED),
        )
        && expected_date != actual
    {
        return Err(EngineError::ResourceChanged);
    }
    Ok(())
}

fn parse_content_range(value: &str) -> Option<(u64, u64, Option<u64>)> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    if end < start {
        return None;
    }
    let total = if total == "*" {
        None
    } else {
        Some(total.parse().ok()?)
    };
    Some((start, end, total))
}

fn header_u64(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}

fn header_string(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_owned)
}

fn strong_etag(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(ETAG)?.to_str().ok()?;
    (!value.starts_with("W/")).then(|| value.to_owned())
}

fn safe_filename(headers: &HeaderMap, url: &Url) -> String {
    let from_header = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.split(';').find_map(|part| {
                let (name, value) = part.trim().split_once('=')?;
                name.eq_ignore_ascii_case("filename")
                    .then(|| value.trim_matches(['"', '\'']).to_owned())
            })
        });
    sanitize_filename(
        from_header
            .as_deref()
            .or_else(|| url.path_segments()?.next_back())
            .unwrap_or("download"),
    )
}

fn sanitize_filename(value: &str) -> String {
    let result: String = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    let result = result.trim().trim_matches('.');
    if result.is_empty() {
        "download".to_owned()
    } else {
        result.chars().take(255).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_urls_and_credentials() {
        assert!(matches!(
            HttpEngine::validate_url("file:///tmp/a"),
            Err(EngineError::UnsupportedScheme)
        ));
        assert!(HttpEngine::validate_url("https://user:pass@example.com/a").is_err());
    }

    #[test]
    fn sanitizes_untrusted_filenames() {
        assert_eq!(sanitize_filename("../../bad:name.txt"), "_.._bad_name.txt");
        assert_eq!(sanitize_filename(".."), "download");
    }

    #[test]
    fn parses_content_ranges_strictly() {
        assert_eq!(
            parse_content_range("bytes 10-19/100"),
            Some((10, 19, Some(100)))
        );
        assert_eq!(parse_content_range("bytes 19-10/100"), None);
        assert_eq!(parse_content_range("garbage"), None);
    }
}
