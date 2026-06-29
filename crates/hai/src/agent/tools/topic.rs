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
        context::topic_section,
        runtime::ctx::RoundContext,
        tools::util::{
            MapToolErr, deserialize_lenient_i64_vec, deserialize_option_lenient_i64_vec, tool_data,
            tool_ok,
        },
    },
    agentcore::render::render_json,
    domain::{service::DbServices, vo::ChatId},
};

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct CreateTopicArgs {
    #[input(description = "标题")]
    pub title: String,
    #[input(description = "初始摘要")]
    pub summary: String,
    #[serde(default, deserialize_with = "deserialize_option_lenient_i64_vec")]
    #[input(description = "关联消息 ID")]
    pub message_ids: Option<Vec<i64>>,
}

#[tool(
    name = "create_topic",
    description = "创建话题",
    input = CreateTopicArgs,
)]
pub struct CreateTopic {
    pub chat_id: ChatId,
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for CreateTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: CreateTopicArgs = serde_json::from_value(args)?;

        let msg_ids: Vec<crate::domain::vo::MessageId> = typed_args
            .message_ids
            .iter()
            .flat_map(|v| v.iter())
            .map(|id| crate::domain::vo::MessageId(*id))
            .collect();
        let topic = self
            .services
            .topic
            .create_topic(
                self.chat_id,
                &typed_args.title,
                &typed_args.summary,
                &msg_ids,
                None,
            )
            .await
            .into_tool_err()?;

        tool_data(serde_json::json!({ "topic": render_json(topic_section(&[topic])) }))
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct AssignTopicArgs {
    #[input(description = "话题 ID")]
    pub topic_id: Uuid,
    #[serde(deserialize_with = "deserialize_lenient_i64_vec")]
    #[input(description = "消息 ID")]
    pub message_ids: Vec<i64>,
}

#[tool(
    name = "assign_topic",
    description = "消息归入话题",
    input = AssignTopicArgs,
)]
pub struct AssignTopic {
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for AssignTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: AssignTopicArgs = serde_json::from_value(args)?;

        let msg_ids: Vec<crate::domain::vo::MessageId> = typed_args
            .message_ids
            .iter()
            .map(|id| crate::domain::vo::MessageId(*id))
            .collect();
        self.services
            .topic
            .assign_topic(&msg_ids, typed_args.topic_id)
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct ListTopicsArgs {
    #[input(description = "状态: active/closed")]
    pub status: Option<String>,
    #[input(description = "数量限制")]
    pub limit: Option<i64>,
    #[input(description = "偏移量")]
    pub offset: Option<i64>,
}

#[tool(
    name = "list_topics",
    description = "列出话题",
    input = ListTopicsArgs,
)]
pub struct ListTopics {
    pub chat_id: ChatId,
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for ListTopics {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: ListTopicsArgs = serde_json::from_value(args)?;
        let limit = typed_args.limit.unwrap_or(10);
        let offset = typed_args.offset.unwrap_or(0);

        let topics = self
            .services
            .topic
            .list_topics(self.chat_id, typed_args.status.as_deref(), limit, offset)
            .await
            .into_tool_err()?;

        if topics.is_empty() {
            return tool_ok();
        }

        tool_data(serde_json::json!({ "topics": render_json(topic_section(&topics)) }))
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SearchTopicsArgs {
    #[input(description = "搜索关键词")]
    pub query: String,
    #[input(description = "数量限制")]
    pub limit: Option<i64>,
}

#[tool(
    name = "search_topics",
    description = "搜索话题",
    input = SearchTopicsArgs,
)]
pub struct SearchTopics {
    pub chat_id: ChatId,
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for SearchTopics {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: SearchTopicsArgs = serde_json::from_value(args)?;
        let limit = typed_args.limit.unwrap_or(10);

        let topics = self
            .services
            .topic
            .search_topics_by_query(self.chat_id, &typed_args.query, limit)
            .await
            .into_tool_err()?;
        let topic_entities: Vec<_> = topics.into_iter().map(|t| t.topic).collect();

        let section = topic_section(&topic_entities);
        tool_data(serde_json::json!({ "topics": render_json(section) }))
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct CorrectTopicArgs {
    #[input(description = "话题 ID")]
    pub topic_id: Uuid,
    #[input(description = "新标题")]
    pub title: Option<String>,
    #[input(description = "新摘要（覆盖已有摘要）")]
    pub summary: Option<String>,
}

#[tool(
    name = "correct_topic",
    description = "修正话题信息",
    input = CorrectTopicArgs,
)]
pub struct CorrectTopic {
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for CorrectTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: CorrectTopicArgs = serde_json::from_value(args)?;

        self.services
            .topic
            .update_topic(
                typed_args.topic_id,
                typed_args.title.as_deref(),
                typed_args.summary.as_deref(),
            )
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct PushTopicSummaryArgs {
    #[input(description = "话题 ID")]
    pub topic_id: Uuid,
    #[input(description = "追加的摘要内容（不重复已有信息）")]
    pub summary: String,
}

#[tool(
    name = "push_topic_summary",
    description = "向活跃话题追加话题摘要",
    input = PushTopicSummaryArgs,
)]
pub struct PushTopicSummary {
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for PushTopicSummary {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: PushTopicSummaryArgs = serde_json::from_value(args)?;

        self.services
            .topic
            .append_summary(typed_args.topic_id, &typed_args.summary)
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct CloseTopicArgs {
    #[input(description = "话题 ID")]
    pub topic_id: Uuid,
    #[input(description = "新标题（可选）")]
    pub title: Option<String>,
    #[input(description = "最终摘要（背景+历程+结论）")]
    pub summary: String,
}

#[tool(
    name = "close_topic",
    description = "关闭话题",
    input = CloseTopicArgs,
)]
pub struct CloseTopic {
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for CloseTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: CloseTopicArgs = serde_json::from_value(args)?;

        self.services
            .topic
            .update_topic(typed_args.topic_id, typed_args.title.as_deref(), None)
            .await
            .into_tool_err()?;

        self.services
            .topic
            .close_topic(typed_args.topic_id, &typed_args.summary)
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct DeleteTopicArgs {
    #[input(description = "话题 ID")]
    pub topic_id: Uuid,
}

#[tool(
    name = "delete_topic",
    description = "删除话题",
    input = DeleteTopicArgs,
)]
pub struct DeleteTopic {
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for DeleteTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: DeleteTopicArgs = serde_json::from_value(args)?;
        self.services
            .topic
            .delete_topic(typed_args.topic_id)
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

pub fn tools(ctx: &RoundContext) -> Vec<Arc<dyn ToolT>> {
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
