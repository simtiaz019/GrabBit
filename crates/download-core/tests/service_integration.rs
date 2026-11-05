use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Response, StatusCode, header},
    routing::any,
};
use download_common::DownloadState;
use download_core::{AddDownloadRequest, DownloadService};
use tempfile::tempdir;
use tokio::{net::TcpListener, time::sleep};

async fn fixture(
    State(bytes): State<Arc<Vec<u8>>>,
    method: Method,
    headers: HeaderMap,
) -> Response<Body> {
    let length = bytes.len();
    if method == Method::HEAD {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, length)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ETAG, "\"service-v1\"")
            .body(Body::empty())
            .expect("response");
    }
    if let Some(value) = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
    {
        let value = value.strip_prefix("bytes=").expect("range prefix");
        let (start, end) = value.split_once('-').expect("range bounds");
        let start: usize = start.parse().expect("start");
        let end: usize = if end.is_empty() {
            length - 1
        } else {
            end.parse().expect("end")
        };
        return Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{end}/{length}"),
            )
            .header(header::CONTENT_LENGTH, end - start + 1)
            .header(header::ETAG, "\"service-v1\"")
            .body(Body::from(bytes[start..=end].to_vec()))
            .expect("response");
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, length)
        .body(Body::from(bytes.as_ref().clone()))
        .expect("response")
}

#[tokio::test]
async fn persists_downloads_and_finalizes_exact_content() {
    let bytes = Arc::new(
        (0_u32..128 * 1024)
            .map(|value| (value % 239) as u8)
            .collect::<Vec<_>>(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let app = Router::new()
        .route("/payload.bin", any(fixture))
        .with_state(bytes.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.expect("server") });

    let data = tempdir().expect("data directory");
    let downloads = tempdir().expect("download directory");
    let service = DownloadService::open(data.path()).await.expect("service");
    let job = service
        .add(AddDownloadRequest {
            url: format!("http://{address}/payload.bin"),
            destination_directory: downloads.path().to_path_buf(),
            segments: 4,
            sha256: None,
        })
        .await
        .expect("enqueue");

    for _ in 0..100 {
        let current = service.list().await.expect("list");
        let current = current
            .iter()
            .find(|candidate| candidate.id == job.id)
            .expect("job");
        if current.state == DownloadState::Completed {
            let actual = tokio::fs::read(&current.destination)
                .await
                .expect("final file");
            assert_eq!(actual, *bytes);
            drop(service);
            let reopened = DownloadService::open(data.path()).await.expect("reopen");
            assert_eq!(
                reopened.list().await.expect("history")[0].state,
                DownloadState::Completed
            );
            return;
        }
        assert!(
            current.state != DownloadState::Failed,
            "download failed: {:?}",
            current.error
        );
        sleep(Duration::from_millis(25)).await;
    }
    panic!("download did not complete in time");
}
