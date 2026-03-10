use autoagents::core::tool::ToolCallError;

pub fn toolcall_std_err<E: std::error::Error>(e: E) -> ToolCallError {
    ToolCallError::RuntimeError(e.to_string().into())
}

pub fn toolcall_anyhow_err(e: anyhow::Error) -> ToolCallError {
    ToolCallError::RuntimeError(e.to_string().into())
}

pub fn toolcall_err(msg: impl Into<String>) -> ToolCallError {
    ToolCallError::RuntimeError(Into::<String>::into(msg).into())
}
