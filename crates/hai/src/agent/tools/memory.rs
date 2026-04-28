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
        tools::{
            ToolContext,
            util::{MapToolErr, tool_data, tool_err, tool_ok},
        },
    },
    agentcore::render::render_json,
    domain::{service::DbServices, vo::MemoryInput},
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
    #[input(description = "chat_id")]
    pub chat_id: i64,
    #[input(description = "分类")]
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
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for RecordMemory {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: RecordMemoryArgs = serde_json::from_value(args)?;
        let RecordMemoryArgs {
            chat_id,
            content,
            account_id,
            references,
            ..
        } = typed_args;
        let input = match typed_args.category {
            RecordMemoryCategory::UserFact => {
                let account_id =
                    account_id.ok_or_else(|| tool_err("account_id is required for 'user_fact'"))?;
                MemoryInput::CreateUserFact {
                    account_id,
                    chat_id,
                    content,
                }
            }
            RecordMemoryCategory::Knowledge => MemoryInput::CreateKnowledge { chat_id, content },
            RecordMemoryCategory::Note => MemoryInput::CreateAgentNote {
                chat_id,
                references,
                content,
            },
            RecordMemoryCategory::ChatRule => MemoryInput::UpsertChatRule { chat_id, content },
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
    #[input(description = "chat_id")]
    pub chat_id: i64,
    #[input(description = "记忆 ID")]
    pub id: Uuid,
    #[input(description = "分类")]
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
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for CorrectMemory {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: CorrectMemoryArgs = serde_json::from_value(args)?;
        let CorrectMemoryArgs {
            chat_id,
            id,
            content,
            importance,
            ..
        } = typed_args;
        let input = match typed_args.category {
            RecordMemoryCategory::UserFact => MemoryInput::UpdateUserFact {
                id,
                content,
                importance,
            },
            RecordMemoryCategory::Knowledge => MemoryInput::UpdateKnowledge {
                id,
                content,
                importance,
            },
            RecordMemoryCategory::Note => MemoryInput::UpdateAgentNote {
                id,
                content,
                importance,
            },
            RecordMemoryCategory::ChatRule => MemoryInput::UpsertChatRule {
                chat_id,
                content: content.ok_or_else(|| tool_err("content is required for 'chat_rule'"))?,
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
    #[input(description = "chat_id")]
    pub chat_id: i64,
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
            .search_knowledge(typed_args.chat_id, &typed_args.query, limit)
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
        let count = self
            .services
            .memory
            .delete(typed_args.id)
            .await
            .into_tool_err()?;
        tool_data(serde_json::json!({ "deleted_count": count }))
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn ToolT>> {
    vec![
        Arc::new(RecordMemory {
            services: ctx.services(),
        }),
        Arc::new(CorrectMemory {
            services: ctx.services(),
        }),
        Arc::new(SearchMemory {
            services: ctx.services(),
        }),
        Arc::new(DeleteMemory {
            services: ctx.services(),
        }),
    ]
}
