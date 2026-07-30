//! All error types for the langcodec crate.
//!
//! These are returned from all fallible operations (parsing, serialization, conversion, etc.).

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Stable machine-readable category for [`enum@Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnknownFormat,
    Parse,
    XmlParse,
    CsvParse,
    Io,
    DataMismatch,
    InvalidResource,
    UnsupportedFormat,
    Conversion,
    Validation,
    MissingLanguage,
    AmbiguousMatch,
    PolicyViolation,
}

/// Optional structured metadata attached to an error.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ErrorContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub candidates: Vec<String>,
}

/// Serializable structured representation of an [`enum@Error`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ErrorContext>,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("unknown format `{0}`")]
    UnknownFormat(String),

    #[error("parse error: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("XML parse error: {0}")]
    XmlParse(#[from] quick_xml::Error),

    #[error("CSV parse error: {0}")]
    CsvParse(#[from] csv::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid data: {0}")]
    DataMismatch(String),

    #[error("invalid resource: {0}")]
    InvalidResource(String),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("conversion error: {message}")]
    Conversion {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("validation error: {0}")]
    Validation(String),

    #[error("missing language for `{path}` ({format})")]
    MissingLanguage { path: String, format: String },

    #[error("ambiguous match for key `{key}` in language `{language}`: {candidates:?}")]
    AmbiguousMatch {
        key: String,
        language: String,
        candidates: Vec<String>,
    },

    #[error("policy violation: {0}")]
    PolicyViolation(String),

    /// Adds path metadata without changing the underlying error category or source.
    #[error("{source} (path: `{path}`)")]
    WithPath {
        path: String,
        #[source]
        source: Box<Error>,
    },
}

impl Error {
    /// Creates a new conversion error with optional source error
    pub fn conversion_error(
        message: impl Into<String>,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Error::Conversion {
            message: message.into(),
            source,
        }
    }

    /// Creates a new validation error
    pub fn validation_error(message: impl Into<String>) -> Self {
        Error::Validation(message.into())
    }

    /// Creates a new missing-language error.
    pub fn missing_language(path: impl Into<String>, format: impl Into<String>) -> Self {
        Error::MissingLanguage {
            path: path.into(),
            format: format.into(),
        }
    }

    /// Creates a new policy violation error.
    pub fn policy_violation(message: impl Into<String>) -> Self {
        Error::PolicyViolation(message.into())
    }

    /// Attaches a filesystem path while retaining this error as the source.
    pub fn with_path(self, path: impl AsRef<Path>) -> Self {
        Error::WithPath {
            path: path.as_ref().to_string_lossy().into_owned(),
            source: Box::new(self),
        }
    }

    /// Returns a machine-readable error code.
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Error::UnknownFormat(_) => ErrorCode::UnknownFormat,
            Error::Parse(_) => ErrorCode::Parse,
            Error::XmlParse(_) => ErrorCode::XmlParse,
            Error::CsvParse(_) => ErrorCode::CsvParse,
            Error::Io(_) => ErrorCode::Io,
            Error::DataMismatch(_) => ErrorCode::DataMismatch,
            Error::InvalidResource(_) => ErrorCode::InvalidResource,
            Error::UnsupportedFormat(_) => ErrorCode::UnsupportedFormat,
            Error::Conversion { .. } => ErrorCode::Conversion,
            Error::Validation(_) => ErrorCode::Validation,
            Error::MissingLanguage { .. } => ErrorCode::MissingLanguage,
            Error::AmbiguousMatch { .. } => ErrorCode::AmbiguousMatch,
            Error::PolicyViolation(_) => ErrorCode::PolicyViolation,
            Error::WithPath { source, .. } => source.error_code(),
        }
    }

    /// Returns optional structured context for the error.
    pub fn context(&self) -> Option<ErrorContext> {
        match self {
            Error::MissingLanguage { path, format } => Some(ErrorContext {
                path: Some(path.clone()),
                format: Some(format.clone()),
                ..ErrorContext::default()
            }),
            Error::AmbiguousMatch {
                key,
                language,
                candidates,
            } => Some(ErrorContext {
                key: Some(key.clone()),
                language: Some(language.clone()),
                candidates: candidates.clone(),
                ..ErrorContext::default()
            }),
            Error::WithPath { path, source } => {
                let mut context = source.context().unwrap_or_default();
                context.path = Some(path.clone());
                Some(context)
            }
            _ => None,
        }
    }

    /// Converts this error into a serializable structured shape.
    pub fn structured(&self) -> StructuredError {
        StructuredError {
            code: self.error_code(),
            message: self.to_string(),
            context: self.context(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_unknown_format_error() {
        let error = Error::UnknownFormat("invalid_format".to_string());
        assert_eq!(error.to_string(), "unknown format `invalid_format`");
    }

    #[test]
    fn test_parse_error() {
        let json_error = serde_json::from_str::<serde_json::Value>("{ invalid json }").unwrap_err();
        let error = Error::Parse(json_error);
        assert!(error.to_string().contains("parse error"));
    }

    #[test]
    fn test_io_error() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let error = Error::Io(io_error);
        assert!(error.to_string().contains("I/O error"));
    }

    #[test]
    fn test_data_mismatch_error() {
        let error = Error::DataMismatch("Invalid data format".to_string());
        assert_eq!(error.to_string(), "invalid data: Invalid data format");
    }

    #[test]
    fn test_invalid_resource_error() {
        let error = Error::InvalidResource("Missing required field".to_string());
        assert_eq!(
            error.to_string(),
            "invalid resource: Missing required field"
        );
    }

    #[test]
    fn test_unsupported_format_error() {
        let error = Error::UnsupportedFormat("xyz".to_string());
        assert_eq!(error.to_string(), "unsupported format: xyz");
    }

    #[test]
    fn test_conversion_error_with_source() {
        let source_error = Box::new(io::Error::new(io::ErrorKind::NotFound, "Source error"));
        let error = Error::conversion_error("Conversion failed", Some(source_error));
        assert!(
            error
                .to_string()
                .contains("conversion error: Conversion failed")
        );
    }

    #[test]
    fn test_conversion_error_without_source() {
        let error = Error::conversion_error("Conversion failed", None);
        assert!(
            error
                .to_string()
                .contains("conversion error: Conversion failed")
        );
    }

    #[test]
    fn test_validation_error() {
        let error = Error::validation_error("Validation failed");
        assert_eq!(error.to_string(), "validation error: Validation failed");
    }

    #[test]
    fn test_error_display() {
        let errors = vec![
            Error::UnknownFormat("test".to_string()),
            Error::DataMismatch("test".to_string()),
            Error::InvalidResource("test".to_string()),
            Error::UnsupportedFormat("test".to_string()),
            Error::Validation("test".to_string()),
            Error::PolicyViolation("test".to_string()),
        ];

        for error in errors {
            let display = format!("{}", error);
            assert!(!display.is_empty());
            assert!(display.contains("test"));
        }
    }

    #[test]
    fn test_error_debug() {
        let error = Error::UnknownFormat("test".to_string());
        let debug = format!("{:?}", error);
        assert!(debug.contains("UnknownFormat"));
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_structured_error_for_missing_language() {
        let error = Error::missing_language("/tmp/Localizable.strings", "strings");
        let structured = error.structured();
        assert_eq!(structured.code, ErrorCode::MissingLanguage);
        assert_eq!(
            structured.context.as_ref().and_then(|c| c.path.as_deref()),
            Some("/tmp/Localizable.strings")
        );
    }

    #[test]
    fn test_structured_error_for_ambiguous_match() {
        let error = Error::AmbiguousMatch {
            key: "welcome".to_string(),
            language: "fr".to_string(),
            candidates: vec!["a".to_string(), "b".to_string()],
        };
        let structured = error.structured();
        assert_eq!(structured.code, ErrorCode::AmbiguousMatch);
        assert_eq!(
            structured.context.as_ref().map(|c| c.candidates.clone()),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }
}
