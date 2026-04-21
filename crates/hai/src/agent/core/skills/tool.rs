//! `load_skill` 元工具（meta-tool）
//!
//! 实现 Agent Skills 规范的 Level 2 激活：
//! LLM 调用此工具时，返回对应 SKILL.md 的完整正文，
//! 该正文作为工具结果注入到对话上下文，指导 LLM 完成复杂任务。

use std::sync::Arc;

use anyhow::Result;
use autoagents::async_trait;
use autoagents::core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::manager::SkillManager;
use crate::agent::tools::util::toolcall_err;

// --- Args ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct LoadSkillArgs {
    #[input(description = "要激活的 skill 名称（对应 SKILL.md frontmatter 中的 name 字段）")]
    pub command: String,
}

// --- Tool ---

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
            .ok_or_else(|| toolcall_err(format!("Skill '{}' not found", typed.command)))?;

        let body = skill.resolved_body();

        Ok(serde_json::json!({
            "skill": typed.command,
            "instructions": body,
        }))
    }
}

/// 构建 load_skill 工具实例
pub fn load_skill_tool(skill_manager: Arc<SkillManager>) -> Arc<dyn ToolT> {
    Arc::new(LoadSkill { skill_manager })
}
