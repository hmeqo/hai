use autoagents::core::tool::ToolCallError;
use serde_json::{Value, json};

use crate::error::AppError;

pub fn tool_ok() -> Result<Value, ToolCallError> {
    Ok(json!({ "ok": true }))
}

pub fn tool_msg(msg: impl Into<String>) -> Result<Value, ToolCallError> {
    Ok(json!({ "ok": true, "message": msg.into() }))
}

pub fn tool_data(data: Value) -> Result<Value, ToolCallError> {
    Ok(json!({ "ok": true, "data": data }))
}

pub fn tool_with(msg: impl Into<String>, data: Value) -> Result<Value, ToolCallError> {
    Ok(json!({ "ok": true, "message": msg.into(), "data": data }))
}

pub fn tool_err(msg: impl Into<String>) -> ToolCallError {
    ToolCallError::RuntimeError(msg.into().into())
}

pub trait MapToolErr<T> {
    fn into_tool_err(self) -> Result<T, ToolCallError>;
}

impl<T> MapToolErr<T> for Result<T, AppError> {
    fn into_tool_err(self) -> Result<T, ToolCallError> {
        self.map_err(|e| ToolCallError::RuntimeError(Box::new(e)))
    }
}
