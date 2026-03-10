use std::fmt::{Debug, Display};
use std::net::AddrParseError;

use serde_json::Value;
use strum::{EnumString, IntoStaticStr};
use thiserror::Error;

type AnyError = dyn std::error::Error + Send + Sync + 'static;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, strum::Display, IntoStaticStr)]
pub enum ErrorKind {
    // ========================================================
    // Request & Data
    // ========================================================
    #[strum(serialize = "data.bad_request")]
    BadRequest,
    #[strum(serialize = "data.parse")]
    DataParse,
    #[strum(serialize = "data.validation_failed")]
    ValidationFailed,
    #[strum(serialize = "data.invalid")]
    InvalidParameter,

    #[strum(serialize = "auth.unauthorized")]
    Unauthorized,
    #[strum(serialize = "auth.forbidden")]
    Forbidden,
    #[strum(serialize = "auth.permission_denied")]
    PermissionDenied,
    #[strum(serialize = "auth.invalid_credentials")]
    InvalidCredentials,

    // ========================================================
    // Resource
    // ========================================================
    #[strum(serialize = "res.not_found")]
    NotFound,
    #[strum(serialize = "res.already_exists")]
    AlreadyExists,

    // ========================================================
    // System & Environment
    // ========================================================
    #[strum(serialize = "sys.config")]
    Config,
    #[strum(serialize = "err.internal")]
    Internal,
}

impl ErrorKind {
    pub fn code(&self) -> &'static str {
        self.into()
    }

    pub fn default_message(&self) -> &'static str {
        match self {
            Self::DataParse => "Data parsing failed",
            Self::InvalidParameter => "Invalid parameter",
            Self::Unauthorized => "Unauthorized",
            Self::Forbidden => "Forbidden",
            Self::PermissionDenied => "Permission denied",
            Self::InvalidCredentials => "Invalid credentials",
            Self::ValidationFailed => "Validation failed",
            Self::BadRequest => "Bad request",

            Self::NotFound => "Resource not found",
            Self::AlreadyExists => "Resource already exists",

            Self::Config => "Configuration error",
            _ => "Internal server error",
        }
    }

    pub fn is_internal_error(&self) -> bool {
        matches!(self, Self::Config | Self::Internal)
    }

    pub fn to_error(self) -> AppError {
        AppError::new(self)
    }

    pub fn with_message(self, msg: impl Into<String>) -> AppError {
        AppError::new(self).with_message(msg)
    }

    /// Wraps any error into an AppError of this kind
    pub fn with_error<E>(self, err: E) -> AppError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        AppError::new(self).with_source(err)
    }

    /// Wraps any error into an AppError of this kind with a custom message
    pub fn with_source<E>(self, err: E, msg: impl Into<String>) -> AppError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        AppError::new(self).with_message(msg).with_source(err)
    }

    pub fn wrap_internal<E>(err: E) -> AppError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Internal.with_error(err)
    }
}

#[derive(Error)]
pub struct AppError {
    kind: ErrorKind,

    message: Option<String>,

    errors: Option<Value>,

    #[source]
    source: Option<Box<AnyError>>,
}

impl AppError {
    fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            message: None,
            errors: None,
            source: None,
        }
    }

    fn with_message(mut self, msg: impl Into<String>) -> Self {
        self.message = Some(msg.into());
        self
    }

    fn with_source<E>(mut self, err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        if self.message.is_none() && !self.kind.is_internal_error() {
            self.message = Some((&err).to_string());
        }

        self.source = Some(Box::new(err));
        self
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn code(&self) -> &str {
        self.kind.into()
    }

    pub fn message(&self) -> &str {
        self.message
            .as_deref()
            .unwrap_or_else(|| self.kind.default_message())
    }

    pub fn errors(&self) -> Option<&Value> {
        self.errors.as_ref()
    }

    pub fn trace_source(&self) {
        if self.kind.is_internal_error() {
            tracing::error!("Internal error: {}", self);
        }
        if let Some(err) = self.source.as_ref() {
            tracing::error!(error = err)
        }
    }
}

impl Debug for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppError")
            .field("code", &self.code())
            .field("message", &self.message())
            .field("source", &self.source)
            .finish()
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

impl From<ErrorKind> for AppError {
    fn from(kind: ErrorKind) -> Self {
        kind.to_error()
    }
}

macro_rules! register_errors {
    ( $( $err_type:ty => $kind:expr $(, $msg:literal)? );* $(;)? ) => {
        $(
            impl From<$err_type> for AppError {
                fn from(e: $err_type) -> Self {
                    let err = $kind;
                    $(
                        return err.with_source(e, $msg);
                    )?
                    #[allow(unreachable_code)]
                    err.with_error(e)
                }
            }
        )*
    };
}

// 注册错误转换
register_errors! {
    std::io::Error      => ErrorKind::Internal;
    serde_json::Error   => ErrorKind::DataParse;
    config::ConfigError => ErrorKind::Config;
    AddrParseError      => ErrorKind::InvalidParameter, "Invalid address format";
}
