use std::{
    fmt::{Debug, Display},
    net::AddrParseError,
};

use reqwest::Error as ReqwestError;
use serde_json::Value;
use strum::{EnumString, IntoStaticStr};
use teloxide::RequestError;
use thiserror::Error;

type DynError = dyn std::error::Error + Send + Sync + 'static;
type BoxedDynError = Box<DynError>;

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
    #[strum(serialize = "err.external")]
    External,
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
            Self::External => "External service error",
            Self::Internal => "Internal server error",
        }
    }

    pub fn is_internal_error(&self) -> bool {
        matches!(self, Self::Config | Self::Internal)
    }

    pub fn to_error(self) -> AppError {
        AppError::new(self)
    }

    pub fn with_msg(self, msg: impl Into<String>) -> AppError {
        AppError::new(self).with_msg(msg)
    }

    /// Wraps any error into an AppError of this kind
    pub fn with_err<E>(self, err: E) -> AppError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        AppError::new(self).with_err(err)
    }

    pub fn with_dyn_err(self, err: BoxedDynError) -> AppError {
        AppError::new(self).with_dyn_err(err)
    }

    pub fn with_err_msg<E>(self, err: E, msg: impl Into<String>) -> AppError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        AppError::new(self).with_msg(msg).with_err(err)
    }

    pub fn wrap_internal<E>(err: E) -> AppError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Internal.with_err(err)
    }
}

#[derive(Error)]
pub struct AppError {
    kind: ErrorKind,

    message: Option<String>,

    errors: Option<Value>,

    #[source]
    source: Option<Box<DynError>>,
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

    fn with_msg(mut self, msg: impl Into<String>) -> Self {
        self.message = Some(msg.into());
        self
    }

    fn with_err<E>(mut self, err: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        if self.message.is_none() && !self.kind.is_internal_error() {
            self.message = Some(err.to_string());
        }

        self.source = Some(Box::new(err));
        self
    }

    fn with_dyn_err(mut self, err: BoxedDynError) -> Self {
        if self.message.is_none() && !self.kind.is_internal_error() {
            self.message = Some(err.to_string());
        }
        self.source = Some(err);
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
        write!(f, "[{}] {}", self.kind, self.message()).and_then(|_| {
            if let Some(err) = self.source.as_ref() {
                write!(f, "\nCause: {}", err)
            } else {
                Ok(())
            }
        })
    }
}

pub trait OptionAppExt<T> {
    fn ok_or_err(self, kind: ErrorKind) -> Result<T>;
    fn ok_or_err_msg(self, kind: ErrorKind, msg: impl Into<String>) -> Result<T>;
}

impl<T> OptionAppExt<T> for Option<T> {
    fn ok_or_err(self, kind: ErrorKind) -> Result<T> {
        self.ok_or_else(|| kind.to_error())
    }

    fn ok_or_err_msg(self, kind: ErrorKind, msg: impl Into<String>) -> Result<T> {
        self.ok_or_else(|| kind.with_msg(msg))
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

pub trait AppResultExt<T> {
    fn change_err(self, kind: ErrorKind) -> Result<T>;
    fn change_err_msg(self, kind: ErrorKind, msg: impl Into<String>) -> Result<T>;
}

impl<T, E> AppResultExt<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn change_err(self, kind: ErrorKind) -> Result<T> {
        self.map_err(|e| kind.with_err(e))
    }

    fn change_err_msg(self, kind: ErrorKind, msg: impl Into<String>) -> Result<T> {
        self.map_err(|e| kind.with_err_msg(e, msg))
    }
}

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
                        return err.with_err_msg(e, $msg);
                    )?
                    #[allow(unreachable_code)]
                    err.with_err(e)
                }
            }
        )*
    };
}

// 注册错误转换
register_errors! {
    std::io::Error                  => ErrorKind::Internal;
    serde_json::Error               => ErrorKind::DataParse;
    config::ConfigError             => ErrorKind::Config;
    AddrParseError                  => ErrorKind::InvalidParameter, "Invalid address format";
    RequestError                    => ErrorKind::Internal;
    ReqwestError                    => ErrorKind::BadRequest, "HTTP request failed";
    sqlx::Error                     => ErrorKind::Internal;
    sqlx::migrate::MigrateError      => ErrorKind::Internal;
    std::num::ParseIntError         => ErrorKind::DataParse, "Failed to parse integer";
    std::time::SystemTimeError      => ErrorKind::Internal;
    jiff::Error                     => ErrorKind::Internal;
    toml::ser::Error                => ErrorKind::DataParse;
}
