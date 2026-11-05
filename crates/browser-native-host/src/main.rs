use std::{env, io, path::PathBuf};

use browser_native_host::{
    ProtocolError, RequestEnvelope, ResponseCode, ResponseEnvelope, read_native_message,
    validate_origin, validate_request, write_native_message,
};

#[cfg(any(unix, windows))]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;

#[tokio::main]
async fn main() {
    let origin = env::args().skip(1).find(|arg| {
        arg.starts_with("chrome-extension://")
            || arg.starts_with("moz-extension://")
            || arg == browser_native_host::FIREFOX_EXTENSION_ID
    });
    if let Err(error) = origin
        .as_deref()
        .ok_or(ProtocolError::Unauthorized)
        .and_then(validate_origin)
    {
        let _ = write_native_message(
            &mut io::stdout().lock(),
            &ResponseEnvelope::error(None, ResponseCode::Unauthorized, error.to_string()),
        );
        return;
    }
    loop {
        let payload = match read_native_message(&mut io::stdin().lock()) {
            Ok(Some(payload)) => payload,
            Ok(None) => break,
            Err(error) => {
                let _ = write_error(None, ResponseCode::InvalidRequest, &error.to_string());
                break;
            }
        };
        let parsed = serde_json::from_slice::<RequestEnvelope>(&payload);
        let response = match parsed {
            Ok(request) => match validate_request(&request) {
                Ok(()) => forward(&payload, request.request_id).await,
                Err(error) => ResponseEnvelope::error(
                    Some(request.request_id),
                    ResponseCode::InvalidRequest,
                    error.to_string(),
                ),
            },
            Err(_) => ResponseEnvelope::error(
                None,
                ResponseCode::InvalidRequest,
                "invalid request schema",
            ),
        };
        if write_native_message(&mut io::stdout().lock(), &response).is_err() {
            break;
        }
    }
}

fn write_error(
    request_id: Option<uuid::Uuid>,
    code: ResponseCode,
    message: &str,
) -> Result<(), ProtocolError> {
    write_native_message(
        &mut io::stdout().lock(),
        &ResponseEnvelope::error(request_id, code, message),
    )
}

#[cfg(unix)]
async fn forward(payload: &[u8], request_id: uuid::Uuid) -> ResponseEnvelope {
    let endpoint = ipc_endpoint();
    let exchange = async {
        let mut stream = UnixStream::connect(endpoint).await?;
        let length =
            u32::try_from(payload.len()).map_err(|_| io::Error::other("message too large"))?;
        stream.write_all(&length.to_le_bytes()).await?;
        stream.write_all(payload).await?;
        let mut header = [0_u8; 4];
        stream.read_exact(&mut header).await?;
        let response_length = u32::from_le_bytes(header) as usize;
        if response_length > browser_native_host::MAX_MESSAGE_BYTES {
            return Err(io::Error::other("response too large"));
        }
        let mut response = vec![0; response_length];
        stream.read_exact(&mut response).await?;
        serde_json::from_slice::<ResponseEnvelope>(&response).map_err(io::Error::other)
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(8), exchange)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "IPC response timed out"))
        .and_then(std::convert::identity);
    result.unwrap_or_else(|_| {
        ResponseEnvelope::error(
            Some(request_id),
            ResponseCode::AppUnavailable,
            "GrabBit is not running or its private IPC endpoint is unavailable",
        )
    })
}

#[cfg(windows)]
async fn forward(payload: &[u8], request_id: uuid::Uuid) -> ResponseEnvelope {
    let result = async {
        let endpoint = windows_ipc_endpoint().await?;
        let started = tokio::time::Instant::now();
        let mut stream = loop {
            match ClientOptions::new().open(&endpoint) {
                Ok(stream) => break stream,
                Err(error) if started.elapsed() < std::time::Duration::from_secs(2) => {
                    let _ = error;
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(error) => return Err(error),
            }
        };
        let exchange = async {
            let length =
                u32::try_from(payload.len()).map_err(|_| io::Error::other("message too large"))?;
            stream.write_all(&length.to_le_bytes()).await?;
            stream.write_all(payload).await?;
            let mut header = [0_u8; 4];
            stream.read_exact(&mut header).await?;
            let response_length = u32::from_le_bytes(header) as usize;
            if response_length > browser_native_host::MAX_MESSAGE_BYTES {
                return Err(io::Error::other("response too large"));
            }
            let mut response = vec![0; response_length];
            stream.read_exact(&mut response).await?;
            serde_json::from_slice::<ResponseEnvelope>(&response).map_err(io::Error::other)
        };
        tokio::time::timeout(std::time::Duration::from_secs(8), exchange)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "IPC response timed out"))?
    }
    .await;
    result.unwrap_or_else(|_| {
        ResponseEnvelope::error(
            Some(request_id),
            ResponseCode::AppUnavailable,
            "GrabBit is not running or its private named pipe is unavailable",
        )
    })
}

#[cfg(windows)]
async fn windows_ipc_endpoint() -> io::Result<String> {
    let path = env::var_os("GRABBIT_IPC_ENDPOINT_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("APPDATA")
                .map(PathBuf::from)
                .map(|path| path.join("com.grabbit.desktop/browser-ipc.endpoint"))
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "app data directory unavailable"))?;
    let endpoint = tokio::fs::read_to_string(path).await?;
    let endpoint = endpoint.trim();
    let id = endpoint
        .strip_prefix(r"\\.\pipe\grabbit-")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid pipe endpoint"))?;
    uuid::Uuid::parse_str(id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid pipe capability"))?;
    Ok(endpoint.to_owned())
}

#[cfg(not(any(unix, windows)))]
async fn forward(_payload: &[u8], request_id: uuid::Uuid) -> ResponseEnvelope {
    ResponseEnvelope::error(
        Some(request_id),
        ResponseCode::AppUnavailable,
        "native IPC is unsupported",
    )
}

#[cfg(unix)]
fn ipc_endpoint() -> PathBuf {
    if let Some(path) = env::var_os("GRABBIT_IPC_ENDPOINT") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "macos")]
    let base = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Library/Application Support/com.grabbit.desktop");
    #[cfg(not(target_os = "macos"))]
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_default()
        .join("com.grabbit.desktop");
    base.join("browser-ipc.sock")
}

#[cfg(not(any(unix, windows)))]
fn ipc_endpoint() -> PathBuf {
    PathBuf::new()
}
