//! Errors. One serializable type serves both a failed task (kept on the task, shown in the UI)
//! and a failed request to the manager: the UI words the message from `code`, `message` is the
//! technical detail for the log and the tooltip.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Not a URL, or a scheme no engine handles.
    InvalidUrl,
    /// The protocol arrives in a later phase (FTP, BitTorrent).
    Unsupported,
    /// Connection refused, reset, DNS failure.
    Network,
    Timeout,
    Tls,
    /// Any other HTTP status the server answered with (`status`).
    HttpStatus,
    /// An FTP server refused a command for good (`status` holds its reply code).
    FtpReply,
    NotFound,
    /// 401 / 407 without usable credentials.
    AuthRequired,
    Forbidden,
    /// ETag, Last-Modified or length changed since the download started.
    RemoteChanged,
    /// The partial file disappeared.
    FileMissing,
    /// The finished file does not have the checksum the user gave (the file stays).
    ChecksumMismatch,
    DiskFull,
    Io,
    /// A task id that does not exist, an operation that does not apply.
    InvalidRequest,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    /// HTTP status for `HttpStatus`, `NotFound`, `Forbidden`, `AuthRequired`; the FTP reply code
    /// for `FtpReply`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }

    pub fn internal(message: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::Internal, message.to_string())
    }

    /// An I/O error, keeping "disk full" apart.
    pub fn io(e: &std::io::Error, context: impl std::fmt::Display) -> Self {
        let code = match e.kind() {
            std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded => {
                ErrorCode::DiskFull
            }
            _ => ErrorCode::Io,
        };
        Self::new(code, format!("{context}: {e}"))
    }

    /// Whether trying again later may succeed (the scheduler retries these).
    pub fn is_transient(&self) -> bool {
        match self.code {
            ErrorCode::Network | ErrorCode::Timeout => true,
            ErrorCode::HttpStatus => {
                matches!(self.status, Some(408 | 425 | 429 | 500..=599))
            }
            _ => false,
        }
    }
}

impl From<omnidl_storage::Error> for Error {
    fn from(e: omnidl_storage::Error) -> Self {
        match &e {
            omnidl_storage::Error::Io(io) => Self::io(io, "storage"),
            _ => Self::internal(e),
        }
    }
}
