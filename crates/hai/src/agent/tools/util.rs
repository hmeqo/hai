use autoagents::core::tool::ToolCallError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ToolResult {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: None,
        }
    }

    pub fn success_with_data(message: impl Into<String>, data: Value) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: Some(data),
        }
    }

    pub fn fail(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self)
            .unwrap_or_else(|_| Value::String("Failed to serialize tool result".into()))
    }
}

pub fn toolcall_std_err<E: std::error::Error>(e: E) -> ToolCallError {
    ToolCallError::RuntimeError(e.to_string().into())
}

pub fn toolcall_anyhow_err(e: anyhow::Error) -> ToolCallError {
    ToolCallError::RuntimeError(e.to_string().into())
}

pub fn toolcall_err(msg: impl Into<String>) -> ToolCallError {
    ToolCallError::RuntimeError(Into::<String>::into(msg).into())
}
