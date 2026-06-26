use std::sync::Arc;

use autoagents::{
    async_trait,
    core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT},
};
use autoagents_derive::ToolInput;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{
    agent::{
        runtime::{ctx::RoundContext, shell::ShellRuntime},
        tools::util::tool_data,
    },
    agentcore::skills::SkillManager,
};

#[derive(Debug, Serialize, Deserialize, ToolInput)]
struct ShellArgs {
    #[input(description = "要执行的 shell 命令")]
    pub command: String,
    #[input(description = "工作目录")]
    pub workdir: Option<String>,
    #[input(description = "关联的 skill 名称")]
    pub skill: Option<String>,
    #[input(description = "超时秒数，默认 30")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug)]
pub struct RunShell {
    pub description: String,
    pub skill_manager: SkillManager,
    pub shell: Arc<Mutex<ShellRuntime>>,
}

impl ToolT for RunShell {
    fn name(&self) -> &str {
        "run_shell"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn args_schema(&self) -> Value {
        serde_json::from_str(ShellArgs::io_schema()).expect("Failed to parse shell args schema")
    }
}

#[async_trait]
impl ToolRuntime for RunShell {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
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
            .await?;

        tool_data(json!({
            "stdout": output.stdout,
            "stderr": output.stderr,
            "exit_code": output.exit_code,
        }))
    }
}

pub fn tools(ctx: &RoundContext) -> Vec<Arc<dyn ToolT>> {
    let description = if ctx.sandbox.enabled {
        format!(
            "执行 shell 命令。运行在容器中（镜像: {}）。可通过 skill 参数自动挂载 skill 目录。",
            ctx.sandbox.image
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
