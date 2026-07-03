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
    agentcore::{
        skills::SkillManager,
        tool::{AgentTool, ToolError, tool_data, tool_err},
    },
};

#[derive(Debug, Deserialize, JsonSchema)]
struct ShellArgs {
    /// 要执行的 shell 命令
    pub command: String,
    /// 工作目录
    pub workdir: Option<String>,
    /// 关联的 skill 名称
    pub skill: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_lenient_u64")]
    /// 超时秒数，默认 30
    pub timeout_secs: Option<u64>,
}

#[derive(Debug)]
pub struct RunShell {
    pub description: String,
    pub skill_manager: SkillManager,
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

        let skill_dir = typed
            .skill
            .as_deref()
            .and_then(|name| self.skill_manager.find(name))
            .map(|s| s.base_dir.clone());

        let output = self
            .shell
            .lock()
            .await
            .execute(
                &typed.command,
                typed.workdir,
                skill_dir.as_deref(),
                typed.timeout_secs,
            )
            .await
            .map_err(|e| tool_err(e.to_string()))?;

        tool_data(json!({
            "stdout": output.stdout,
            "stderr": output.stderr,
            "exit_code": output.exit_code,
        }))
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    let description = if ctx.sandbox_enabled {
        format!(
            "执行 shell 命令。运行在容器中（镜像: {}）。可通过 skill 参数自动挂载 skill 目录。",
            ctx.sandbox_image.as_deref().unwrap_or("unknown"),
        )
    } else {
        "执行 shell 命令。可通过 skill 参数自动挂载 skill 目录。".into()
    };
    vec![Arc::new(RunShell {
        description,
        skill_manager: ctx.skill_manager.clone(),
        shell: ctx.shell.clone(),
    })]
}
