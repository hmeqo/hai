use async_trait::async_trait;
use serde_json::{Value, json};
use thiserror::Error;

use crate::error::AppError;

pub(crate) mod schema;

/// Tool failure text as the model sees it; both `From` impls flatten their error into this
/// message.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct ToolError(String);

impl From<AppError> for ToolError {
    fn from(e: AppError) -> Self {
        Self(e.to_string())
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        Self(e.to_string())
    }
}

/// What one tool call produced: the JSON the model receives, plus media that only a model with the
/// matching input capability consumes.
#[derive(Debug)]
pub struct ToolOutput {
    pub data: Value,
    pub media: Vec<ToolMedia>,
}

#[derive(Clone)]
pub enum ToolMedia {
    Image {
        mime: String,
        data: Vec<u8>,
        /// The caller's focus, carried into the user message beside the image.
        focus: Option<String>,
    },
}

impl std::fmt::Debug for ToolMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image { mime, data, focus } => f
                .debug_struct("ToolMedia::Image")
                .field("mime", mime)
                .field("bytes", &data.len())
                .field("focus", focus)
                .finish(),
        }
    }
}

impl ToolOutput {
    /// A tool result carrying a JSON value as the model receives it.
    pub fn data(data: Value) -> Self {
        Self {
            data,
            media: Vec::new(),
        }
    }

    pub fn with_media(data: Value, media: Vec<ToolMedia>) -> Self {
        Self { data, media }
    }

    /// A successful result whose payload goes under `data`, the envelope every tool result shares.
    pub fn ok(data: Value) -> Self {
        Self::data(envelope(data))
    }
}

fn envelope(data: Value) -> Value {
    json!({ "ok": true, "data": data })
}

/// The capabilities of the model a tool call runs on.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolCallContext {
    pub image_input: bool,
}

#[async_trait]
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// The tool's argument schema. A tool that takes no arguments states
    /// `agentcore::tool::schema::empty`; the wire never sees an absent one.
    fn schema(&self) -> Value;
    async fn execute(&self, args: Value, ctx: &ToolCallContext) -> Result<ToolOutput, ToolError>;

    fn ends_turn(&self) -> bool {
        false
    }

    /// Whether this call produces something the user receives (a message, an image, or a voice
    /// note).
    fn speaks(&self) -> bool {
        false
    }

    /// Whether this call makes something (spending a model call) without putting it in front of the
    /// user — a wrap-up must not start one.
    fn produces(&self) -> bool {
        false
    }

    /// Whether this call only reads: nothing in the chat, the workspace or on the platform changes.
    fn reads_only(&self) -> bool {
        false
    }
}

/// JSON Schema for a tool's args as schemars emits it; the root is normalized for the wire by
/// `agentcore/tool/schema.rs`.
pub fn schema_of<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("valid schema")
}

pub fn tool_ok() -> Result<Value, ToolError> {
    Ok(json!({ "ok": true }))
}

pub fn tool_data(data: Value) -> Result<Value, ToolError> {
    Ok(envelope(data))
}

pub fn tool_err(msg: impl Into<String>) -> ToolError {
    ToolError(msg.into())
}
