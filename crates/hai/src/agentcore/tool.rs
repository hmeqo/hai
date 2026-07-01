use async_trait::async_trait;
use serde_json::{Value, json};
use thiserror::Error;

use crate::error::AppError;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("{0}")]
    Msg(String),

    #[error("Tool execution failed: {0}")]
    Execution(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<AppError> for ToolError {
    fn from(e: AppError) -> Self {
        ToolError::Msg(e.to_string())
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        ToolError::Msg(e.to_string())
    }
}

#[async_trait]
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Option<Value>;
    async fn execute(&self, args: Value) -> Result<Value, ToolError>;
}

// ── Tool 结果辅助 ──

pub fn tool_ok() -> Result<Value, ToolError> {
    Ok(json!({ "ok": true }))
}

pub fn tool_data(data: Value) -> Result<Value, ToolError> {
    Ok(json!({ "ok": true, "data": data }))
}

pub fn tool_err(msg: impl Into<String>) -> ToolError {
    ToolError::Msg(msg.into())
}

pub trait MapToolErr<T> {
    fn into_tool_err(self) -> Result<T, ToolError>;
}

impl<T> MapToolErr<T> for Result<T, AppError> {
    fn into_tool_err(self) -> Result<T, ToolError> {
        self.map_err(|e| ToolError::Msg(e.to_string()))
    }
}
