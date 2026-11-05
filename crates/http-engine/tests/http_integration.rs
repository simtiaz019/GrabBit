use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Response, StatusCode, header},
    response::Redirect,
    routing::{any, get},
};
use download_common::RemoteMetadata;
use futures_util::stream;
use http_engine::{DownloadRequest, EngineConfig, EngineError, EngineEvent, HttpEngine};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use tokio::time::sleep;
use tokio::{
    io::{AsyncSeekExt, AsyncWriteExt, SeekFrom},
    net::TcpListener,
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

const ETAG: &str = "\"fixture-v1\"";

#[derive(Clone)]
struct Fixture {
    bytes: Arc<Vec<u8>>,
    rate_requests: Arc<AtomicUsize>,
    drop_requests: Arc<AtomicUsize>,
}

async fn spawn_server() -> (SocketAddr, Vec<u8>) {
    let bytes: Vec<u8> = (0_u32..256 * 1024)
        .map(|value| u8::try_from(value % 251).expect("value is bounded"))
        .collect();
    let state = Fixture {
        bytes: Arc::new(bytes.clone()),
        rate_requests: Arc::new(AtomicUsize::new(0)),
        drop_requests: Arc::new(AtomicUsize::new(0)),
    };
    let app = Router::new()
        .route("/file.bin", any(range_file))
        .route("/no-range.bin", any(no_range))
        .route("/ignores-range.bin", any(ignores_range))
        .route("/bad-range.bin", any(bad_range))
        .route("/changed.bin", any(changed))
        .route("/rate.bin", any(rate_limited))
        .route("/unknown.bin", any(unknown_length))
        .route("/slow.bin", any(slow_file))
        .route("/drop.bin", any(dropping_file))
        .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
        .route(
            "/redirect",
            get(|| async { Redirect::temporary("/file.bin") }),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture");
    let address = listener.local_addr().expect("fixture address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("fixture server");
    });
    (address, bytes)
}

async fn range_file(
    State(fixture): State<Fixture>,
    method: Method,
    headers: HeaderMap,
) -> Response<Body> {
    let length = fixture.bytes.len();
    if method == Method::HEAD {
        return response(
            StatusCode::OK,
            Body::empty(),
            &[
                (header::CONTENT_LENGTH, length.to_string()),
                (header::ACCEPT_RANGES, "bytes".into()),
                (header::ETAG, ETAG.into()),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=fixture.bin".into(),
                ),
            ],
        );
    }
    if let Some((start, end)) = requested_range(&headers, length) {
        let body = fixture.bytes[start..=end].to_vec();
        return response(
            StatusCode::PARTIAL_CONTENT,
            Body::from(body),
            &[
                (
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{length}"),
                ),
                (header::CONTENT_LENGTH, (end - start + 1).to_string()),
                (header::ACCEPT_RANGES, "bytes".into()),
                (header::ETAG, ETAG.into()),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=fixture.bin".into(),
                ),
            ],
        );
    }
    response(
        StatusCode::OK,
        Body::from(fixture.bytes.as_ref().clone()),
        &[
            (header::CONTENT_LENGTH, length.to_string()),
            (header::ETAG, ETAG.into()),
        ],
    )
}

async fn no_range(State(fixture): State<Fixture>, method: Method) -> Response<Body> {
    let headers = [(header::CONTENT_LENGTH, fixture.bytes.len().to_string())];
    if method == Method::HEAD {
        response(StatusCode::OK, Body::empty(), &headers)
    } else {
        response(
            StatusCode::OK,
            Body::from(fixture.bytes.as_ref().clone()),
            &headers,
        )
    }
}

async fn ignores_range(State(fixture): State<Fixture>, method: Method) -> Response<Body> {
    let headers = [
        (header::CONTENT_LENGTH, fixture.bytes.len().to_string()),
        (header::ACCEPT_RANGES, "bytes".into()),
    ];
    if method == Method::HEAD {
        response(StatusCode::OK, Body::empty(), &headers)
    } else {
        response(
            StatusCode::OK,
            Body::from(fixture.bytes.as_ref().clone()),
            &headers,
        )
    }
}

async fn bad_range(State(fixture): State<Fixture>, method: Method) -> Response<Body> {
    if method == Method::HEAD {
        return response(
            StatusCode::OK,
            Body::empty(),
            &[
                (header::CONTENT_LENGTH, fixture.bytes.len().to_string()),
                (header::ACCEPT_RANGES, "bytes".into()),
            ],
        );
    }
    response(
        StatusCode::PARTIAL_CONTENT,
        Body::from(vec![1_u8]),
        &[(header::CONTENT_RANGE, "bytes 9-1/100".into())],
    )
}

async fn changed(State(fixture): State<Fixture>, headers: HeaderMap) -> Response<Body> {
    let length = fixture.bytes.len();
    let (start, end) = requested_range(&headers, length).unwrap_or((0, length - 1));
    response(
        StatusCode::PARTIAL_CONTENT,
        Body::from(fixture.bytes[start..=end].to_vec()),
        &[
            (
                header::CONTENT_RANGE,
                format!("bytes {start}-{end}/{length}"),
            ),
            (header::ETAG, "\"fixture-v2\"".into()),
        ],
    )
}

async fn rate_limited(State(fixture): State<Fixture>, method: Method) -> Response<Body> {
    if fixture.rate_requests.fetch_add(1, Ordering::SeqCst) == 0 {
        return response(StatusCode::TOO_MANY_REQUESTS, Body::empty(), &[]);
    }
    no_range(State(fixture), method).await
}

async fn unknown_length(State(fixture): State<Fixture>, method: Method) -> Response<Body> {
    if method == Method::HEAD {
        return response(StatusCode::METHOD_NOT_ALLOWED, Body::empty(), &[]);
    }
    let chunks = fixture
        .bytes
        .chunks(4096)
        .map(|chunk| Ok::<_, Infallible>(chunk.to_vec()))
        .collect::<Vec<_>>();
    response(StatusCode::OK, Body::from_stream(stream::iter(chunks)), &[])
}

async fn slow_file(
    State(fixture): State<Fixture>,
    method: Method,
    headers: HeaderMap,
) -> Response<Body> {
    if method == Method::HEAD {
        return range_file(State(fixture), method, headers).await;
    }
    let length = fixture.bytes.len();
    let range = requested_range(&headers, length);
    let (start, end) = range.unwrap_or((0, length - 1));
    let chunks = fixture.bytes[start..=end]
        .chunks(4096)
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    let body = stream::unfold(chunks.into_iter(), |mut chunks| async move {
        let chunk = chunks.next()?;
        sleep(Duration::from_millis(3)).await;
        Some((Ok::<_, Infallible>(chunk), chunks))
    });
    if range.is_some() {
        response(
            StatusCode::PARTIAL_CONTENT,
            Body::from_stream(body),
            &[
                (
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{length}"),
                ),
                (header::CONTENT_LENGTH, (end - start + 1).to_string()),
                (header::ACCEPT_RANGES, "bytes".into()),
                (header::ETAG, ETAG.into()),
            ],
        )
    } else {
        response(
            StatusCode::OK,
            Body::from_stream(body),
            &[
                (header::CONTENT_LENGTH, length.to_string()),
                (header::ACCEPT_RANGES, "bytes".into()),
                (header::ETAG, ETAG.into()),
            ],
        )
    }
}

async fn dropping_file(
    State(fixture): State<Fixture>,
    method: Method,
    headers: HeaderMap,
) -> Response<Body> {
    if method == Method::HEAD || headers.contains_key(header::RANGE) {
        return range_file(State(fixture), method, headers).await;
    }
    if fixture.drop_requests.fetch_add(1, Ordering::SeqCst) == 0 {
        let stream = stream::iter([
            Ok::<_, std::io::Error>(fixture.bytes[..4096].to_vec()),
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "fixture connection drop",
            )),
        ]);
        return response(
            StatusCode::OK,
            Body::from_stream(stream),
            &[
                (header::CONTENT_LENGTH, fixture.bytes.len().to_string()),
                (header::ACCEPT_RANGES, "bytes".into()),
                (header::ETAG, ETAG.into()),
            ],
        );
    }
    range_file(State(fixture), method, headers).await
}

fn requested_range(headers: &HeaderMap, length: usize) -> Option<(usize, usize)> {
    let value = headers
        .get(header::RANGE)?
        .to_str()
        .ok()?
        .strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;
    let start = start.parse().ok()?;
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse().ok()?
    };
    Some((start, end.min(length - 1)))
}

fn response(
    status: StatusCode,
    body: Body,
    headers: &[(header::HeaderName, String)],
) -> Response<Body> {
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    builder.body(body).expect("valid fixture response")
}

fn engine() -> HttpEngine {
    HttpEngine::new(EngineConfig::default()).expect("engine")
}

#[tokio::test]
async fn probes_redirects_and_downloads_segmented_content_exactly() {
    let (address, expected) = spawn_server().await;
    let engine = engine();
    let metadata = engine
        .probe(&format!("http://{address}/redirect"))
        .await
        .expect("probe");
    assert_eq!(metadata.content_length, Some(expected.len() as u64));
    assert!(metadata.supports_ranges);
    assert_eq!(metadata.file_name, "fixture.bin");
    assert_eq!(metadata.etag.as_deref(), Some(ETAG));

    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("payload.part");
    let result = engine
        .download(
            DownloadRequest {
                url: &metadata.final_url,
                path: &path,
                metadata: &metadata,
                segments: 8,
                resume_from: 0,
                segment_offsets: &[],
            },
            CancellationToken::new(),
            None,
        )
        .await
        .expect("segmented download");
    assert!(result.segmented);
    let actual = tokio::fs::read(path).await.expect("downloaded file");
    assert_eq!(actual.len(), expected.len());
    assert_eq!(Sha256::digest(&actual), Sha256::digest(&expected));
}

#[tokio::test]
async fn sequential_mode_supports_servers_without_ranges() {
    let (address, expected) = spawn_server().await;
    let engine = engine();
    let url = format!("http://{address}/no-range.bin");
    let metadata = engine.probe(&url).await.expect("probe");
    assert!(!metadata.supports_ranges);
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("payload.part");
    let result = engine
        .download(
            DownloadRequest {
                url: &url,
                path: &path,
                metadata: &metadata,
                segments: 8,
                resume_from: 0,
                segment_offsets: &[],
            },
            CancellationToken::new(),
            None,
        )
        .await
        .expect("sequential download");
    assert!(!result.segmented);
    assert_eq!(tokio::fs::read(&path).await.expect("file"), expected);
}

#[tokio::test]
async fn falls_back_when_a_server_ignores_ranges() {
    let (address, expected) = spawn_server().await;
    let engine = engine();
    let url = format!("http://{address}/ignores-range.bin");
    let metadata = engine.probe(&url).await.expect("probe");
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("payload.part");
    let result = engine
        .download(
            DownloadRequest {
                url: &url,
                path: &path,
                metadata: &metadata,
                segments: 4,
                resume_from: 0,
                segment_offsets: &[],
            },
            CancellationToken::new(),
            None,
        )
        .await
        .expect("fallback download");
    assert!(!result.segmented);
    assert_eq!(tokio::fs::read(&path).await.expect("file"), expected);
}

#[tokio::test]
async fn rejects_changed_resources_and_malformed_ranges() {
    let (address, bytes) = spawn_server().await;
    let engine = engine();
    let directory = tempdir().expect("temporary directory");
    let metadata = RemoteMetadata {
        final_url: format!("http://{address}/changed.bin"),
        file_name: "changed.bin".into(),
        content_length: Some(bytes.len() as u64),
        supports_ranges: true,
        etag: Some(ETAG.into()),
        last_modified: None,
        content_type: None,
    };
    let changed = engine
        .download(
            DownloadRequest {
                url: &metadata.final_url,
                path: &directory.path().join("changed.part"),
                metadata: &metadata,
                segments: 2,
                resume_from: 0,
                segment_offsets: &[],
            },
            CancellationToken::new(),
            None,
        )
        .await;
    assert!(matches!(changed, Err(EngineError::ResourceChanged)));

    let bad_url = format!("http://{address}/bad-range.bin");
    let bad_metadata = engine.probe(&bad_url).await.expect("head probe");
    let malformed = engine
        .download(
            DownloadRequest {
                url: &bad_url,
                path: &directory.path().join("bad.part"),
                metadata: &bad_metadata,
                segments: 2,
                resume_from: 0,
                segment_offsets: &[],
            },
            CancellationToken::new(),
            None,
        )
        .await;
    assert!(matches!(
        malformed,
        Err(EngineError::InvalidContentRange | EngineError::LengthMismatch)
    ));
}

#[tokio::test]
async fn reports_http_errors() {
    let (address, _) = spawn_server().await;
    let error = engine()
        .probe(&format!("http://{address}/missing"))
        .await
        .expect_err("404 must fail");
    assert!(matches!(
        error,
        EngineError::HttpStatus(StatusCode::NOT_FOUND)
    ));
}

#[tokio::test]
async fn retries_rate_limits_and_supports_unknown_lengths() {
    let (address, expected) = spawn_server().await;
    let engine = engine();
    let rate = engine
        .probe(&format!("http://{address}/rate.bin"))
        .await
        .expect("rate-limited probe retries");
    assert_eq!(rate.content_length, Some(expected.len() as u64));

    let url = format!("http://{address}/unknown.bin");
    let metadata = engine.probe(&url).await.expect("unknown probe");
    assert_eq!(metadata.content_length, None);
    let directory = tempdir().expect("directory");
    let path = directory.path().join("unknown.part");
    engine
        .download(
            DownloadRequest {
                url: &url,
                path: &path,
                metadata: &metadata,
                segments: 1,
                resume_from: 0,
                segment_offsets: &[],
            },
            CancellationToken::new(),
            None,
        )
        .await
        .expect("unknown-length download");
    assert_eq!(tokio::fs::read(&path).await.expect("file"), expected);
}

#[tokio::test]
async fn cancellation_flushes_a_safe_sequential_resume_point() {
    let (address, expected) = spawn_server().await;
    let engine = engine();
    let url = format!("http://{address}/slow.bin");
    let metadata = engine.probe(&url).await.expect("probe");
    let directory = tempdir().expect("directory");
    let path = directory.path().join("resume.part");
    let token = CancellationToken::new();
    let cancel = token.clone();
    let request = DownloadRequest {
        url: &url,
        path: &path,
        metadata: &metadata,
        segments: 1,
        resume_from: 0,
        segment_offsets: &[],
    };
    let download = engine.download(request, token, None);
    tokio::pin!(download);
    tokio::select! {
        result = &mut download => panic!("download finished before cancellation: {result:?}"),
        () = sleep(Duration::from_millis(15)) => cancel.cancel(),
    }
    assert!(matches!(download.await, Err(EngineError::Cancelled)));
    let checkpoint = tokio::fs::metadata(&path).await.expect("partial").len();
    assert!(checkpoint > 0 && checkpoint < expected.len() as u64);
    engine
        .download(
            DownloadRequest {
                url: &url,
                path: &path,
                metadata: &metadata,
                segments: 1,
                resume_from: checkpoint,
                segment_offsets: &[],
            },
            CancellationToken::new(),
            None,
        )
        .await
        .expect("resume");
    assert_eq!(tokio::fs::read(&path).await.expect("file"), expected);
}

#[tokio::test]
async fn segmented_transfer_resumes_each_durable_checkpoint() {
    let (address, expected) = spawn_server().await;
    let engine = engine();
    let url = format!("http://{address}/file.bin");
    let metadata = engine.probe(&url).await.expect("probe");
    let directory = tempdir().expect("directory");
    let path = directory.path().join("segmented-resume.part");
    let segments = 4_u8;
    let offsets = vec![1024_u64; usize::from(segments)];
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)
        .await
        .expect("partial");
    file.set_len(expected.len() as u64)
        .await
        .expect("preallocate");
    for index in 0..segments {
        let start = expected.len() as u64 * u64::from(index) / u64::from(segments);
        let start_index = usize::try_from(start).expect("fixture offset fits usize");
        file.seek(SeekFrom::Start(start)).await.expect("seek");
        file.write_all(&expected[start_index..start_index + 1024])
            .await
            .expect("checkpoint bytes");
    }
    file.sync_data().await.expect("durable partial");
    drop(file);

    let result = engine
        .download(
            DownloadRequest {
                url: &url,
                path: &path,
                metadata: &metadata,
                segments,
                resume_from: 0,
                segment_offsets: &offsets,
            },
            CancellationToken::new(),
            None,
        )
        .await
        .expect("segmented resume");
    assert!(result.segmented);
    assert_eq!(tokio::fs::read(path).await.expect("file"), expected);
}

#[tokio::test]
async fn segmented_cancellation_checkpoints_each_partial_range_before_resume() {
    let (address, expected) = spawn_server().await;
    let engine = engine();
    let url = format!("http://{address}/slow.bin");
    let metadata = engine.probe(&url).await.expect("probe");
    let directory = tempdir().expect("directory");
    let path = directory.path().join("segmented-pause.part");
    let segments = 4_u8;
    let token = CancellationToken::new();
    let cancel = token.clone();
    let (events, mut receiver) = mpsc::unbounded_channel();
    let request = DownloadRequest {
        url: &url,
        path: &path,
        metadata: &metadata,
        segments,
        resume_from: 0,
        segment_offsets: &[],
    };
    let download = engine.download(request, token, Some(events));
    tokio::pin!(download);
    tokio::select! {
        result = &mut download => panic!("download finished before cancellation: {result:?}"),
        () = sleep(Duration::from_millis(35)) => cancel.cancel(),
    }
    assert!(matches!(download.await, Err(EngineError::Cancelled)));

    let mut offsets = vec![0_u64; usize::from(segments)];
    let mut checkpoint_count = 0;
    while let Some(event) = receiver.recv().await {
        if let EngineEvent::SegmentCheckpoint { index, downloaded } = event {
            offsets[usize::from(index)] = downloaded;
            checkpoint_count += 1;
        }
    }
    assert_eq!(checkpoint_count, usize::from(segments));
    assert!(offsets.iter().sum::<u64>() > 0);
    assert!(offsets.iter().sum::<u64>() < expected.len() as u64);

    engine
        .download(
            DownloadRequest {
                url: &url,
                path: &path,
                metadata: &metadata,
                segments,
                resume_from: 0,
                segment_offsets: &offsets,
            },
            CancellationToken::new(),
            None,
        )
        .await
        .expect("resume from partial segment checkpoints");
    assert_eq!(tokio::fs::read(&path).await.expect("file"), expected);
}

#[tokio::test]
async fn recovers_from_a_midstream_connection_drop() {
    let (address, expected) = spawn_server().await;
    let engine = engine();
    let url = format!("http://{address}/drop.bin");
    let metadata = engine.probe(&url).await.expect("probe");
    let directory = tempdir().expect("directory");
    let path = directory.path().join("drop.part");
    engine
        .download(
            DownloadRequest {
                url: &url,
                path: &path,
                metadata: &metadata,
                segments: 1,
                resume_from: 0,
                segment_offsets: &[],
            },
            CancellationToken::new(),
            None,
        )
        .await
        .expect("retry after dropped stream");
    assert_eq!(tokio::fs::read(path).await.expect("file"), expected);
}
