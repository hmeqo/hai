use std::sync::Arc;

use autoagents::{
    async_trait,
    core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT},
};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    agent::{
        context::related_memories_section,
        runtime::ctx::RoundContext,
        tools::util::{MapToolErr, tool_data, tool_err, tool_ok},
    },
    agentcore::render::render_json,
    domain::{
        service::DbServices,
        vo::{ChatId, MemoryInput},
    },
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordMemoryCategory {
    UserFact,
    Knowledge,
    Note,
    ChatRule,
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct RecordMemoryArgs {
    #[input(description = "分类", choice = ["user_fact", "knowledge", "note", "chat_rule"])]
    pub category: RecordMemoryCategory,
    #[input(description = "内容")]
    pub content: String,
    #[input(description = "关联用户 ID（user_fact 必填）")]
    pub account_id: Option<i64>,
    #[input(description = "引用: {\"topics\":[\"uuid\"],\"messages\":[123]}")]
    pub references: Option<serde_json::Value>,
}

#[tool(
    name = "record_memory",
    description = "记录记忆（群友特征/知识/笔记/群规）",
    input = RecordMemoryArgs,
)]
pub struct RecordMemory {
    pub chat_id: ChatId,
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for RecordMemory {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: RecordMemoryArgs = serde_json::from_value(args)?;
        let input = match typed_args.category {
            RecordMemoryCategory::UserFact => {
                let account_id = typed_args
                    .account_id
                    .ok_or_else(|| tool_err("account_id is required for 'user_fact'"))?;
                MemoryInput::CreateUserFact {
                    account_id,
                    chat_id: self.chat_id,
                    content: typed_args.content,
                }
            }
            RecordMemoryCategory::Knowledge => MemoryInput::CreateKnowledge {
                chat_id: self.chat_id,
                content: typed_args.content,
            },
            RecordMemoryCategory::Note => MemoryInput::CreateAgentNote {
                chat_id: self.chat_id,
                references: typed_args.references,
                content: typed_args.content,
            },
            RecordMemoryCategory::ChatRule => MemoryInput::UpsertChatRule {
                chat_id: self.chat_id,
                content: typed_args.content,
            },
        };

        self.services
            .memory
            .save_memory(input)
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct CorrectMemoryArgs {
    #[input(description = "记忆 ID")]
    pub id: Uuid,
    #[input(description = "分类", choice = ["user_fact", "knowledge", "note", "chat_rule"])]
    pub category: RecordMemoryCategory,
    #[input(description = "内容")]
    pub content: Option<String>,
    #[input(description = "重要性")]
    pub importance: Option<i32>,
}

#[tool(
    name = "correct_memory",
    description = "更新记忆",
    input = CorrectMemoryArgs,
)]
pub struct CorrectMemory {
    pub chat_id: ChatId,
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for CorrectMemory {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: CorrectMemoryArgs = serde_json::from_value(args)?;
        let input = match typed_args.category {
            RecordMemoryCategory::UserFact => MemoryInput::UpdateUserFact {
                id: typed_args.id,
                content: typed_args.content,
                importance: typed_args.importance,
            },
            RecordMemoryCategory::Knowledge => MemoryInput::UpdateKnowledge {
                id: typed_args.id,
                content: typed_args.content,
                importance: typed_args.importance,
            },
            RecordMemoryCategory::Note => MemoryInput::UpdateAgentNote {
                id: typed_args.id,
                content: typed_args.content,
                importance: typed_args.importance,
            },
            RecordMemoryCategory::ChatRule => MemoryInput::UpsertChatRule {
                chat_id: self.chat_id,
                content: typed_args
                    .content
                    .ok_or_else(|| tool_err("content is required for 'chat_rule'"))?,
            },
        };

        self.services
            .memory
            .save_memory(input)
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SearchMemoryArgs {
    #[input(description = "搜索词")]
    pub query: String,
    #[input(description = "数量限制（默认 10）")]
    pub limit: Option<i64>,
}

#[tool(
    name = "search_memory",
    description = "搜索记忆",
    input = SearchMemoryArgs,
)]
pub struct SearchMemory {
    pub chat_id: ChatId,
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for SearchMemory {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: SearchMemoryArgs = serde_json::from_value(args)?;
        let limit = typed_args.limit.unwrap_or(10);

        let memories = self
            .services
            .memory
            .search_knowledge(self.chat_id, &typed_args.query, limit)
            .await
            .into_tool_err()?;

        let section = related_memories_section(&memories, "memories");
        tool_data(serde_json::json!({ "memories": render_json(section) }))
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct DeleteMemoryArgs {
    #[input(description = "记忆/笔记 UUID")]
    pub id: Uuid,
}

#[tool(
    name = "delete_memory",
    description = "删除记忆",
    input = DeleteMemoryArgs,
)]
pub struct DeleteMemory {
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for DeleteMemory {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: DeleteMemoryArgs = serde_json::from_value(args)?;
        self.services
            .memory
            .delete(crate::domain::vo::MemoryId(typed_args.id))
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

pub fn tools(ctx: &RoundContext) -> Vec<Arc<dyn ToolT>> {
    vec![
        Arc::new(RecordMemory {
            chat_id: ctx.chat_id,
            services: ctx.db.clone(),
        }),
        Arc::new(CorrectMemory {
            chat_id: ctx.chat_id,
            services: ctx.db.clone(),
        }),
        Arc::new(SearchMemory {
            chat_id: ctx.chat_id,
            services: ctx.db.clone(),
        }),
        Arc::new(DeleteMemory {
            services: ctx.db.clone(),
        }),
    ]
}
