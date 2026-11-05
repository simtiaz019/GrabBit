//! Versioned and size-bounded browser native-messaging contracts.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const SCHEMA_VERSION: u16 = 1;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const CHROMIUM_ORIGIN: &str = "chrome-extension://hdfipadppabbjkbbodcdhjelmiifamjj/";
pub const FIREFOX_EXTENSION_ID: &str = "grabbit@grabbit.download";

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("native message is larger than the allowed limit")]
    TooLarge,
    #[error("native message ended unexpectedly")]
    Truncated,
    #[error("native message I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("native message JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported schema version")]
    Version,
    #[error("unauthorized browser extension")]
    Unauthorized,
    #[error("request field is invalid: {0}")]
    InvalidField(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEnvelope {
    pub schema_version: u16,
    pub request_id: Uuid,
    #[serde(flatten)]
    pub message: BrowserRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BrowserRequest {
    Ping,
    GetCapabilities,
    CaptureDownload(CaptureDownload),
    InspectMedia(InspectMedia),
    CreateMediaDownload(CreateMediaDownload),
    GetRequestStatus(RequestStatusQuery),
    CancelPendingCapture(RequestStatusQuery),
    OpenApplication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDownload {
    pub url: String,
    pub suggested_file_name: Option<String>,
    pub browser: BrowserKind,
    pub source_page_origin: Option<String>,
    pub idempotency_key: Uuid,
    #[serde(default)]
    pub automatic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectMedia {
    pub url: String,
    pub authorization_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMediaDownload {
    pub inspection_id: Uuid,
    pub format_ids: Vec<String>,
    pub output: String,
    pub authorization_confirmed: bool,
    pub idempotency_key: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestStatusQuery {
    pub request_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserKind {
    Chrome,
    Brave,
    Edge,
    Firefox,
    Safari,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseEnvelope {
    pub schema_version: u16,
    pub request_id: Option<Uuid>,
    pub ok: bool,
    pub code: ResponseCode,
    pub message: String,
    pub job_id: Option<Uuid>,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseCode {
    Accepted,
    Pong,
    Capabilities,
    Rejected,
    InvalidRequest,
    Unauthorized,
    AppUnavailable,
    Duplicate,
    NotFound,
    InternalError,
}

impl ResponseEnvelope {
    #[must_use]
    pub fn error(request_id: Option<Uuid>, code: ResponseCode, message: impl Into<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id,
            ok: false,
            code,
            message: message.into(),
            job_id: None,
            data: None,
        }
    }
}

/// Validates the browser-provided extension identity against the compiled allowlist.
///
/// # Errors
/// Returns [`ProtocolError::Unauthorized`] for any unapproved identity.
pub fn validate_origin(origin: &str) -> Result<(), ProtocolError> {
    let normalized = origin.trim_end_matches('/');
    let chromium = CHROMIUM_ORIGIN.trim_end_matches('/');
    if normalized == chromium
        || normalized == FIREFOX_EXTENSION_ID
        || normalized == format!("moz-extension://{FIREFOX_EXTENSION_ID}")
    {
        Ok(())
    } else {
        Err(ProtocolError::Unauthorized)
    }
}

/// Validates the schema version and all request fields before IPC forwarding.
///
/// # Errors
/// Returns a protocol error for unsupported versions or invalid fields.
pub fn validate_request(request: &RequestEnvelope) -> Result<(), ProtocolError> {
    if request.schema_version != SCHEMA_VERSION {
        return Err(ProtocolError::Version);
    }
    match &request.message {
        BrowserRequest::CaptureDownload(value) => {
            validate_http_url(&value.url)?;
            if value
                .suggested_file_name
                .as_ref()
                .is_some_and(|name| name.len() > 255 || name.contains(['/', '\\', '\0']))
            {
                return Err(ProtocolError::InvalidField("suggestedFileName"));
            }
            if let Some(origin) = &value.source_page_origin {
                let parsed = url::Url::parse(origin)
                    .map_err(|_| ProtocolError::InvalidField("sourcePageOrigin"))?;
                if origin.len() > 2048
                    || !matches!(parsed.scheme(), "http" | "https")
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.query().is_some()
                    || parsed.fragment().is_some()
                {
                    return Err(ProtocolError::InvalidField("sourcePageOrigin"));
                }
            }
        }
        BrowserRequest::InspectMedia(value) => validate_http_url(&value.url)?,
        BrowserRequest::CreateMediaDownload(value)
            if value.format_ids.is_empty()
                || value.format_ids.len() > 4
                || value
                    .format_ids
                    .iter()
                    .any(|id| id.is_empty() || id.len() > 128) =>
        {
            return Err(ProtocolError::InvalidField("formatIds"));
        }
        _ => {}
    }
    Ok(())
}

fn validate_http_url(value: &str) -> Result<(), ProtocolError> {
    if value.len() > 8192 || !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err(ProtocolError::InvalidField("url"));
    }
    Ok(())
}

/// Reads one browser-native frame from a synchronous stream.
///
/// # Errors
/// Returns an I/O, truncation, or size-limit error for malformed framing.
pub fn read_native_message<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>, ProtocolError> {
    let mut length = [0_u8; 4];
    let first = reader.read(&mut length[..1])?;
    if first == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..]).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            ProtocolError::Truncated
        } else {
            ProtocolError::Io(error)
        }
    })?;
    let length = u32::from_ne_bytes(length) as usize;
    if length > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            ProtocolError::Truncated
        } else {
            ProtocolError::Io(error)
        }
    })?;
    Ok(Some(payload))
}

/// Serializes and writes one browser-native frame.
///
/// # Errors
/// Returns a JSON, I/O, or size-limit error.
pub fn write_native_message<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), ProtocolError> {
    let payload = serde_json::to_vec(value)?;
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| ProtocolError::TooLarge)?;
    writer.write_all(&length.to_ne_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trips() {
        let value = ResponseEnvelope::error(None, ResponseCode::AppUnavailable, "not running");
        let mut bytes = Vec::new();
        write_native_message(&mut bytes, &value).expect("write");
        let payload = read_native_message(&mut bytes.as_slice())
            .expect("read")
            .expect("message");
        let decoded: ResponseEnvelope = serde_json::from_slice(&payload).expect("json");
        assert_eq!(decoded, value);
    }

    #[test]
    fn rejects_oversized_and_truncated_messages() {
        let oversized = (u32::try_from(MAX_MESSAGE_BYTES).expect("limit") + 1)
            .to_ne_bytes()
            .to_vec();
        assert!(matches!(
            read_native_message(&mut oversized.as_slice()),
            Err(ProtocolError::TooLarge)
        ));
        let mut truncated = 10_u32.to_ne_bytes().to_vec();
        truncated.extend_from_slice(b"short");
        assert!(matches!(
            read_native_message(&mut truncated.as_slice()),
            Err(ProtocolError::Truncated)
        ));
    }

    #[test]
    fn validates_schema_origin_url_and_filename() {
        assert!(validate_origin(CHROMIUM_ORIGIN).is_ok());
        assert!(validate_origin("chrome-extension://attacker/").is_err());
        let request = RequestEnvelope {
            schema_version: SCHEMA_VERSION,
            request_id: Uuid::new_v4(),
            message: BrowserRequest::CaptureDownload(CaptureDownload {
                url: "file:///secret".into(),
                suggested_file_name: Some("../secret".into()),
                browser: BrowserKind::Chrome,
                source_page_origin: None,
                idempotency_key: Uuid::new_v4(),
                automatic: false,
            }),
        };
        assert!(validate_request(&request).is_err());
    }
}
