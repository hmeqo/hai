use std::sync::Arc;

use async_trait::async_trait;
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, ContentBlock, Tool},
    service::RunningService,
    transport::child_process::TokioChildProcess,
};
use serde_json::{Map, Value};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use super::tool::{AgentTool, ToolError};
use crate::{
    config::AppConfig,
    error::{AppResultExt, ErrorKind, Result},
};

pub struct McpManager {
    servers: Vec<Arc<McpServerHandle>>,
}

pub struct McpServerHandle {
    name: String,
    service: RunningService<RoleClient, ()>,
}

impl McpManager {
    pub async fn from_config(config: &AppConfig) -> Result<Self> {
        let mut servers = Vec::new();
        for (name, mcp_cfg) in &config.mcp {
            let handle = McpServerHandle::connect(name, mcp_cfg).await.err_kind_msg(
                ErrorKind::Internal,
                format!("Failed to connect MCP server: {name}"),
            )?;
            servers.push(Arc::new(handle));
        }
        Ok(Self { servers })
    }

    pub async fn list_all_tools(&self) -> Vec<Arc<dyn AgentTool>> {
        let mut tools: Vec<Arc<dyn AgentTool>> = Vec::new();
        for server in &self.servers {
            match server.list_tools().await {
                Ok(server_tools) => {
                    for t in server_tools {
                        tools.push(Arc::new(McpAgentTool {
                            server: Arc::clone(server),
                            tool: t,
                        }));
                    }
                }
                Err(e) => {
                    tracing::error!(server = %server.name, "Failed to list tools: {e}");
                }
            }
        }
        tools
    }
}

impl McpServerHandle {
    async fn connect(name: &str, cfg: &crate::config::schema::McpConfig) -> Result<Self> {
        if cfg.r#type != crate::config::schema::McpTransport::Stdio {
            return Err(ErrorKind::Config.msg(format!(
                "MCP transport `{type}` not implemented (only stdio)",
                type = cfg.r#type
            )));
        }
        let mut cmd = Command::new(&cfg.command);
        cmd.args(&cfg.args);
        if let Some(env_map) = &cfg.env {
            cmd.envs(env_map);
        }
        cmd.kill_on_drop(true);

        let (process, stderr_opt) = TokioChildProcess::builder(cmd)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .err_kind_msg(ErrorKind::Internal, "Failed to spawn MCP process")?;

        if let Some(stderr) = stderr_opt {
            let server_name = name.to_string();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                while reader.read_line(&mut line).await.is_ok() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        tracing::debug!(target: "hai::mcp", server = %server_name, "{trimmed}");
                    }
                    line.clear();
                }
            });
        }

        let service = ()
            .serve(process)
            .await
            .err_kind_msg(ErrorKind::Internal, "Failed to serve MCP transport")?;

        tracing::info!(server = %name, "MCP server connected");

        Ok(Self {
            name: name.to_string(),
            service,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    async fn list_tools(&self) -> Result<Vec<Tool>> {
        self.service
            .peer()
            .list_all_tools()
            .await
            .err_kind_msg(ErrorKind::Internal, "Failed to list MCP tools")
    }

    async fn call_tool(
        &self,
        name: &str,
        args: Map<String, Value>,
    ) -> std::result::Result<Value, ToolError> {
        let params = CallToolRequestParams::new(name.to_string()).with_arguments(args);
        let result = self
            .service
            .peer()
            .call_tool(params)
            .await
            .map_err(|e| ToolError::Msg(format!("MCP tool call failed: {e}")))?;

        if let Some(structured) = result.structured_content {
            return Ok(structured);
        }

        let text: String = result
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect::<Vec<&str>>()
            .join("\n");

        if result.is_error.unwrap_or(false) {
            return Err(ToolError::Msg(if text.is_empty() {
                "MCP tool returned an error".into()
            } else {
                text
            }));
        }

        if text.is_empty() {
            return Ok(Value::Null);
        }

        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
}

struct McpAgentTool {
    server: Arc<McpServerHandle>,
    tool: Tool,
}

#[async_trait]
impl AgentTool for McpAgentTool {
    fn name(&self) -> &str {
        &self.tool.name
    }

    fn description(&self) -> &str {
        self.tool.description.as_deref().unwrap_or("")
    }

    fn schema(&self) -> Option<Value> {
        match serde_json::to_value(&*self.tool.input_schema) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!(tool = %self.name(), error = %e, "MCP tool schema serialization failed");
                None
            }
        }
    }

    async fn execute(&self, args: Value) -> std::result::Result<Value, ToolError> {
        let json_map = args.as_object().ok_or_else(|| {
            ToolError::Msg(format!(
                "MCP tool {} expected object args, got {:?}",
                self.name(),
                args
            ))
        })?;
        self.server
            .call_tool(&self.tool.name, json_map.clone())
            .await
    }
}
