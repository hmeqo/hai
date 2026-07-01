use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    agent::{
        context::topic_section,
        runtime::tool_ctx::ToolContext,
        tools::util::{deserialize_lenient_i64_vec, deserialize_option_lenient_i64_vec},
    },
    agentcore::{
        render::render_json,
        tool::{AgentTool, MapToolErr, ToolError, tool_data, tool_ok},
    },
    domain::{service::DbServices, vo::ChatId},
};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateTopicArgs {
    /// 标题
    pub title: String,
    /// 初始摘要
    pub summary: String,
    #[serde(default, deserialize_with = "deserialize_option_lenient_i64_vec")]
    /// 关联消息 ID
    pub message_ids: Option<Vec<i64>>,
}

/// 创建话题
#[hai_macros::tool]
pub struct CreateTopic {
    pub chat_id: ChatId,
    pub services: DbServices,
}

impl CreateTopic {
    async fn exec(&self, args: CreateTopicArgs) -> Result<Value, ToolError> {
        let msg_ids: Vec<crate::domain::vo::MessageId> = args
            .message_ids
            .iter()
            .flat_map(|v| v.iter())
            .map(|id| crate::domain::vo::MessageId(*id))
            .collect();
        let topic = self
            .services
            .topic
            .create_topic(self.chat_id, &args.title, &args.summary, &msg_ids, None)
            .await
            .into_tool_err()?;
        tool_data(serde_json::json!({ "topic": render_json(topic_section(&[topic])) }))
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AssignTopicArgs {
    /// 话题 ID
    pub topic_id: Uuid,
    #[serde(deserialize_with = "deserialize_lenient_i64_vec")]
    /// 消息 ID
    pub message_ids: Vec<i64>,
}

/// 消息归入话题
#[hai_macros::tool]
pub struct AssignTopic {
    pub services: DbServices,
}

impl AssignTopic {
    async fn exec(&self, args: AssignTopicArgs) -> Result<Value, ToolError> {
        let msg_ids: Vec<crate::domain::vo::MessageId> = args
            .message_ids
            .iter()
            .map(|id| crate::domain::vo::MessageId(*id))
            .collect();
        self.services
            .topic
            .assign_topic(&msg_ids, args.topic_id)
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListTopicsArgs {
    /// 状态: active/closed
    pub status: Option<String>,
    /// 数量限制
    pub limit: Option<i64>,
    /// 偏移量
    pub offset: Option<i64>,
}

/// 列出话题
#[hai_macros::tool]
pub struct ListTopics {
    pub chat_id: ChatId,
    pub services: DbServices,
}

impl ListTopics {
    async fn exec(&self, args: ListTopicsArgs) -> Result<Value, ToolError> {
        let limit = args.limit.unwrap_or(10);
        let offset = args.offset.unwrap_or(0);
        let topics = self
            .services
            .topic
            .list_topics(self.chat_id, args.status.as_deref(), limit, offset)
            .await
            .into_tool_err()?;
        if topics.is_empty() {
            return tool_ok();
        }
        tool_data(serde_json::json!({ "topics": render_json(topic_section(&topics)) }))
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchTopicsArgs {
    /// 搜索关键词
    pub query: String,
    /// 数量限制
    pub limit: Option<i64>,
}

/// 搜索话题
#[hai_macros::tool]
pub struct SearchTopics {
    pub chat_id: ChatId,
    pub services: DbServices,
}

impl SearchTopics {
    async fn exec(&self, args: SearchTopicsArgs) -> Result<Value, ToolError> {
        let limit = args.limit.unwrap_or(10);
        let topics = self
            .services
            .topic
            .search_topics_by_query(self.chat_id, &args.query, limit)
            .await
            .into_tool_err()?;
        let entities: Vec<_> = topics.into_iter().map(|t| t.topic).collect();
        tool_data(serde_json::json!({ "topics": render_json(topic_section(&entities)) }))
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CorrectTopicArgs {
    /// 话题 ID
    pub topic_id: Uuid,
    /// 新标题
    pub title: Option<String>,
    /// 新摘要（覆盖已有摘要）
    pub summary: Option<String>,
}

/// 修正话题信息
#[hai_macros::tool]
pub struct CorrectTopic {
    pub services: DbServices,
}

impl CorrectTopic {
    async fn exec(&self, args: CorrectTopicArgs) -> Result<Value, ToolError> {
        self.services
            .topic
            .update_topic(
                args.topic_id,
                args.title.as_deref(),
                args.summary.as_deref(),
            )
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PushTopicSummaryArgs {
    /// 话题 ID
    pub topic_id: Uuid,
    /// 追加的摘要内容（不重复已有信息）
    pub summary: String,
}

/// 向活跃话题追加话题摘要
#[hai_macros::tool]
pub struct PushTopicSummary {
    pub services: DbServices,
}

impl PushTopicSummary {
    async fn exec(&self, args: PushTopicSummaryArgs) -> Result<Value, ToolError> {
        self.services
            .topic
            .append_summary(args.topic_id, &args.summary)
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CloseTopicArgs {
    /// 话题 ID
    pub topic_id: Uuid,
    /// 新标题（可选）
    pub title: Option<String>,
    /// 最终摘要（背景+历程+结论）
    pub summary: String,
}

/// 关闭话题
#[hai_macros::tool]
pub struct CloseTopic {
    pub services: DbServices,
}

impl CloseTopic {
    async fn exec(&self, args: CloseTopicArgs) -> Result<Value, ToolError> {
        self.services
            .topic
            .update_topic(args.topic_id, args.title.as_deref(), None)
            .await
            .into_tool_err()?;
        self.services
            .topic
            .close_topic(args.topic_id, &args.summary)
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DeleteTopicArgs {
    /// 话题 ID
    pub topic_id: Uuid,
}

/// 删除话题
#[hai_macros::tool]
pub struct DeleteTopic {
    pub services: DbServices,
}

impl DeleteTopic {
    async fn exec(&self, args: DeleteTopicArgs) -> Result<Value, ToolError> {
        self.services
            .topic
            .delete_topic(args.topic_id)
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CreateTopic {
            chat_id: ctx.chat_id,
            services: ctx.db.clone(),
        }),
        Arc::new(AssignTopic {
            services: ctx.db.clone(),
        }),
        Arc::new(ListTopics {
            chat_id: ctx.chat_id,
            services: ctx.db.clone(),
        }),
        Arc::new(SearchTopics {
            chat_id: ctx.chat_id,
            services: ctx.db.clone(),
        }),
        Arc::new(CorrectTopic {
            services: ctx.db.clone(),
        }),
        Arc::new(PushTopicSummary {
            services: ctx.db.clone(),
        }),
        Arc::new(CloseTopic {
            services: ctx.db.clone(),
        }),
        Arc::new(DeleteTopic {
            services: ctx.db.clone(),
        }),
    ]
}
