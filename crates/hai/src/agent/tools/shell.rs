use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{
    agent::{
        runtime::{context::ToolContext, shell::ShellRuntime},
        tools::util::deserialize_option_lenient_u64,
    },
    agentcore::tool::{AgentTool, ToolError, tool_data, tool_err},
};

#[derive(Debug, Deserialize, JsonSchema)]
struct ShellArgs {
    pub command: String,
    pub workdir: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_lenient_u64")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug)]
pub struct RunShell {
    pub description: String,
    pub shell: Arc<Mutex<ShellRuntime>>,
}

#[async_trait]
impl AgentTool for RunShell {
    fn name(&self) -> &str {
        "run_shell"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Option<Value> {
        Some(serde_json::to_value(schemars::schema_for!(ShellArgs)).expect("valid schema"))
    }

    async fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let typed: ShellArgs = serde_json::from_value(args)?;

        let output = self
            .shell
            .lock()
            .await
            .execute(&typed.command, typed.workdir, typed.timeout_secs)
            .await
            .map_err(|e| tool_err(e.to_string()))?;

        tool_data(json!({
            "stdout": output.stdout,
            "stderr": output.stderr,
            "exit_code": output.exit_code,
        }))
    }
}

pub fn tools(ctx: &ToolContext, sandbox_image: Option<&str>) -> Vec<Arc<dyn AgentTool>> {
    let description = match sandbox_image {
        Some(img) => format!("执行 shell 命令。运行在容器中（镜像: {img}）；workdir 默认 /tmp。",),
        None => "执行 shell 命令。".into(),
    };
    vec![Arc::new(RunShell {
        description,
        shell: ctx.shell.clone(),
    })]
}
