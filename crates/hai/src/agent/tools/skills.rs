use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    agent::runtime::context::ToolContext,
    agentcore::{
        skills::SkillManager,
        tool::{AgentTool, ToolError},
    },
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LoadSkillArgs {
    /// 要激活的 skill 名称
    pub command: String,
}

/// 激活一个专项 skill，获取该 skill 的详细操作指令。
/// 当你判断用户的请求需要特定 skill 的能力时，调用此工具加载对应的指令。
#[hai_macros::tool]
pub struct LoadSkill {
    pub skill_manager: SkillManager,
}

impl LoadSkill {
    async fn exec(&self, args: LoadSkillArgs) -> Result<Value, ToolError> {
        let skill = self
            .skill_manager
            .find(&args.command)
            .ok_or_else(|| ToolError::Msg(format!("Skill '{}' not found", args.command)))?;

        let body = skill.body();

        Ok(serde_json::json!({
            "skill": args.command,
            "instructions": body,
        }))
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    if ctx.skill_manager.is_empty() {
        return vec![];
    }
    vec![Arc::new(LoadSkill {
        skill_manager: ctx.skill_manager.clone(),
    })]
}
