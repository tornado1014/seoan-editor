use std::{collections::BTreeMap, fmt, io, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    IoError,
    PermissionDenied,
    PathNotAllowed,
    NotHlt,
    HeaderTruncated,
    HeaderInvalid,
    DeflateInvalid,
    DeflateTrailingData,
    FooterTruncated,
    FooterCrcMismatch,
    FooterSizeMismatch,
    WindowNotProven,
    Cp949Invalid,
    StructureInvalid,
    GeometryInvalid,
    RevisionConflict,
    OutputExists,
    OutputSameAsInput,
    ResourceTooLarge,
    SizeLimitExceeded,
    DiskFull,
    Cancelled,
    UnsupportedFormat,
    ConversionFailed,
    VerificationFailed,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        formatter.write_str(value.as_str().ok_or(fmt::Error)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreError {
    pub code: ErrorCode,
    pub message: String,
    pub phase: String,
    pub source: Option<String>,
    #[serde(with = "crate::serde_u64::option")]
    pub offset: Option<u64>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub retryable: bool,
    pub details: BTreeMap<String, String>,
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CoreError {}

impl CoreError {
    pub fn new(code: ErrorCode, phase: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: code.to_string(),
            phase: phase.into(),
            source: None,
            offset: None,
            expected: None,
            actual: None,
            retryable,
            details: BTreeMap::new(),
        }
    }
}

pub(crate) fn safe_source(source: &str) -> Option<String> {
    Path::new(source)
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
}

pub(crate) fn io_error(error: &io::Error, phase: &str, source: &str) -> CoreError {
    let code = if error.kind() == io::ErrorKind::PermissionDenied {
        ErrorCode::PermissionDenied
    } else {
        ErrorCode::IoError
    };
    let mut core_error = CoreError::new(code, phase, false);
    core_error.source = safe_source(source);
    core_error
        .details
        .insert("io_kind".to_string(), format!("{:?}", error.kind()));
    core_error
}
