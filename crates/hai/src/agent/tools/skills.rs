use std::sync::Arc;

use autoagents::{
    async_trait,
    core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT},
};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{agent::tools::util::tool_err, agentcore::skills::SkillManager};

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct LoadSkillArgs {
    #[input(description = "要激活的 skill 名称")]
    pub command: String,
}

#[tool(
    name = "load_skill",
    description = "激活一个专项 skill，获取该 skill 的详细操作指令。\
        当你判断用户的请求需要特定 skill 的能力时，调用此工具加载对应的指令。",
    input = LoadSkillArgs,
)]
pub struct LoadSkill {
    pub skill_manager: Arc<SkillManager>,
}

#[async_trait]
impl ToolRuntime for LoadSkill {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed: LoadSkillArgs = serde_json::from_value(args)?;

        let skill = self
            .skill_manager
            .find(&typed.command)
            .ok_or_else(|| tool_err(format!("Skill '{}' not found", typed.command)))?;

        let body = skill.resolved_body();

        Ok(serde_json::json!({
            "skill": typed.command,
            "instructions": body,
        }))
    }
}

pub fn load_skill_tool(skill_manager: Arc<SkillManager>) -> Vec<Arc<dyn ToolT>> {
    if skill_manager.is_empty() {
        return vec![];
    }
    vec![Arc::new(LoadSkill { skill_manager })]
}
