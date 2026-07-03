use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    agent::{context::related_memories_section, runtime::context::ToolContext},
    agentcore::{
        render::render_json,
        tool::{AgentTool, MapToolErr, ToolError, tool_data, tool_err, tool_ok},
    },
    domain::{
        model::MemoryKind,
        service::DbServices,
        vo::{ChatId, MemoryId},
    },
};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordMemoryCategory {
    UserFact,
    Knowledge,
    Note,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RecordMemoryArgs {
    /// 分类
    pub category: RecordMemoryCategory,
    /// 内容
    pub content: String,
    /// 关联用户 ID（user_fact 必填）
    pub account_id: Option<i64>,
    /// 引用: {"topics":["uuid"],"messages":[123]}
    pub references: Option<serde_json::Value>,
}

/// 记录记忆（群友特征/知识/笔记）
#[hai_macros::tool]
pub struct RecordMemory {
    pub chat_id: ChatId,
    pub services: DbServices,
}

impl RecordMemory {
    async fn exec(&self, args: RecordMemoryArgs) -> Result<Value, ToolError> {
        let kind = match args.category {
            RecordMemoryCategory::UserFact => {
                if args.account_id.is_none() {
                    return Err(tool_err("account_id is required for 'user_fact'"));
                }
                MemoryKind::UserFact
            }
            RecordMemoryCategory::Knowledge => MemoryKind::Knowledge,
            RecordMemoryCategory::Note => MemoryKind::Note,
        };

        self.services
            .memory
            .create(
                kind,
                self.chat_id,
                args.content,
                args.account_id,
                args.references,
            )
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CorrectMemoryArgs {
    /// 记忆 ID
    pub id: Uuid,
    /// 内容
    pub content: Option<String>,
    /// 重要性
    pub importance: Option<i32>,
}

/// 更新记忆
#[hai_macros::tool]
pub struct CorrectMemory {
    pub services: DbServices,
}

impl CorrectMemory {
    async fn exec(&self, args: CorrectMemoryArgs) -> Result<Value, ToolError> {
        self.services
            .memory
            .update(args.id, args.content, args.importance)
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchMemoryArgs {
    /// 搜索词
    pub query: String,
    /// 数量限制（默认 10）
    pub limit: Option<i64>,
}

/// 搜索记忆
#[hai_macros::tool]
pub struct SearchMemory {
    pub chat_id: ChatId,
    pub services: DbServices,
}

impl SearchMemory {
    async fn exec(&self, args: SearchMemoryArgs) -> Result<Value, ToolError> {
        let limit = args.limit.unwrap_or(10);

        let memories = self
            .services
            .memory
            .search_knowledge(self.chat_id, &args.query, limit)
            .await
            .into_tool_err()?;

        let section = related_memories_section(&memories, "memories");
        tool_data(serde_json::json!({ "memories": render_json(section) }))
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DeleteMemoryArgs {
    /// 记忆/笔记 UUID
    pub id: Uuid,
}

/// 删除记忆
#[hai_macros::tool]
pub struct DeleteMemory {
    pub services: DbServices,
}

impl DeleteMemory {
    async fn exec(&self, args: DeleteMemoryArgs) -> Result<Value, ToolError> {
        self.services
            .memory
            .delete(MemoryId(args.id))
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(RecordMemory {
            chat_id: ctx.chat_id,
            services: ctx.db.clone(),
        }),
        Arc::new(CorrectMemory {
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
