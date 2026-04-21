use std::sync::Arc;

use anyhow::Result;
use autoagents::async_trait;
use autoagents::core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::agent::components::topic_section;
use crate::agent::render::render_json;
use crate::agent::tools::util::{ToolResult, toolcall_anyhow_err};
use crate::domain::service::Services;

// --- Create Topic Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct CreateTopicArgs {
    #[input(description = "chat_id")]
    pub chat_id: i64,
    #[input(description = "标题")]
    pub title: String,
    #[input(description = "初始摘要")]
    pub summary: String,
    #[input(description = "关联消息 ID")]
    pub message_ids: Option<Vec<i64>>,
}

#[tool(
    name = "create_topic",
    description = "创建话题",
    input = CreateTopicArgs,
)]
pub struct CreateTopic {
    pub services: Arc<Services>,
}

#[async_trait]
impl ToolRuntime for CreateTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: CreateTopicArgs = serde_json::from_value(args)?;

        let topic = self
            .services
            .topic
            .create_topic(
                typed_args.chat_id,
                &typed_args.title,
                &typed_args.summary,
                typed_args.message_ids.as_deref().unwrap_or(&[]),
                None,
            )
            .await
            .map_err(toolcall_anyhow_err)?;

        Ok(ToolResult::success_with_data(
            "话题已创建",
            serde_json::json!({ "topic": render_json(topic_section(&[topic])) }),
        )
        .to_value())
    }
}

// --- Assign Topic Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct AssignTopicArgs {
    #[input(description = "chat_id")]
    pub chat_id: i64,
    #[input(description = "话题 ID")]
    pub topic_id: Uuid,
    #[input(description = "消息 ID")]
    pub message_ids: Vec<i64>,
}

#[tool(
    name = "assign_topic",
    description = "消息归入话题",
    input = AssignTopicArgs,
)]
pub struct AssignTopic {
    pub services: Arc<Services>,
}

#[async_trait]
impl ToolRuntime for AssignTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: AssignTopicArgs = serde_json::from_value(args)?;

        let count = self
            .services
            .topic
            .assign_topic(&typed_args.message_ids, typed_args.topic_id)
            .await
            .map_err(toolcall_anyhow_err)?;

        Ok(ToolResult::success_with_data(
            format!("已归类 {count} 条消息"),
            serde_json::json!({ "topic_id": typed_args.topic_id.to_string(), "count": count }),
        )
        .to_value())
    }
}

// --- List Topics Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct ListTopicsArgs {
    #[input(description = "chat_id")]
    pub chat_id: i64,
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
    pub services: Arc<Services>,
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
            .list_topics(
                typed_args.chat_id,
                typed_args.status.as_deref(),
                limit,
                offset,
            )
            .await
            .map_err(toolcall_anyhow_err)?;

        if topics.is_empty() {
            return Ok(ToolResult::success("没有话题").to_value());
        }

        Ok(ToolResult::success_with_data(
            "话题列表",
            serde_json::json!({ "topics": render_json(topic_section(&topics)) }),
        )
        .to_value())
    }
}

// --- Search Topics Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SearchTopicsArgs {
    #[input(description = "chat_id")]
    pub chat_id: i64,
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
    pub services: Arc<Services>,
}

#[async_trait]
impl ToolRuntime for SearchTopics {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: SearchTopicsArgs = serde_json::from_value(args)?;
        let limit = typed_args.limit.unwrap_or(10);

        let topics = self
            .services
            .topic
            .search_topics_by_query(typed_args.chat_id, &typed_args.query, limit)
            .await
            .map_err(toolcall_anyhow_err)?;
        let topic_entities: Vec<_> = topics.into_iter().map(|t| t.topic).collect();

        let section = topic_section(&topic_entities);
        Ok(ToolResult::success_with_data(
            "搜索结果",
            serde_json::json!({ "topics": render_json(section), "query": typed_args.query }),
        )
        .to_value())
    }
}

// --- Update Topic Tool ---

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
    description = "修正话题",
    input = CorrectTopicArgs,
)]
pub struct CorrectTopic {
    pub services: Arc<Services>,
}

#[async_trait]
impl ToolRuntime for CorrectTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: CorrectTopicArgs = serde_json::from_value(args)?;

        if let Some(title) = &typed_args.title {
            self.services
                .topic
                .update_title(typed_args.topic_id, title)
                .await
                .map_err(toolcall_anyhow_err)?;
            return Ok(ToolResult::success("标题已更新").to_value());
        }

        if let Some(summary) = &typed_args.summary {
            self.services
                .topic
                .update_summary(typed_args.topic_id, summary)
                .await
                .map_err(toolcall_anyhow_err)?;
            return Ok(ToolResult::success("摘要已更新").to_value());
        }

        Ok(ToolResult::success("无事发生").to_value())
    }
}

// --- Push Topic Summary Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct PushTopicSummaryArgs {
    #[input(description = "话题 ID")]
    pub topic_id: Uuid,
    #[input(description = "追加的摘要内容（不重复已有信息）")]
    pub summary: String,
}

#[tool(
    name = "push_topic_summary",
    description = "追加话题摘要（不覆盖已有内容）",
    input = PushTopicSummaryArgs,
)]
pub struct PushTopicSummary {
    pub services: Arc<Services>,
}

#[async_trait]
impl ToolRuntime for PushTopicSummary {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: PushTopicSummaryArgs = serde_json::from_value(args)?;

        self.services
            .topic
            .push_summary(typed_args.topic_id, &typed_args.summary)
            .await
            .map_err(toolcall_anyhow_err)?;

        Ok(ToolResult::success("摘要已追加").to_value())
    }
}

// --- Finish Topic Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct FinishTopicArgs {
    #[input(description = "话题 ID")]
    pub topic_id: Uuid,
    #[input(description = "新标题（可选）")]
    pub title: Option<String>,
    #[input(description = "最终摘要（背景+历程+结论）")]
    pub summary: String,
}

#[tool(
    name = "finish_topic",
    description = "结项话题并写最终摘要",
    input = FinishTopicArgs,
)]
pub struct FinishTopic {
    pub services: Arc<Services>,
}

#[async_trait]
impl ToolRuntime for FinishTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: FinishTopicArgs = serde_json::from_value(args)?;

        if let Some(title) = &typed_args.title {
            self.services
                .topic
                .update_title(typed_args.topic_id, title)
                .await
                .map_err(toolcall_anyhow_err)?;
        }

        self.services
            .topic
            .finish_topic(typed_args.topic_id, &typed_args.summary)
            .await
            .map_err(toolcall_anyhow_err)?;
        Ok(ToolResult::success("已结项").to_value())
    }
}

// --- Delete Topic Tool ---

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
    pub services: Arc<Services>,
}

#[async_trait]
impl ToolRuntime for DeleteTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: DeleteTopicArgs = serde_json::from_value(args)?;
        let count = self
            .services
            .topic
            .delete_topic(typed_args.topic_id)
            .await
            .map_err(toolcall_anyhow_err)?;
        Ok(
            ToolResult::success_with_data("已删除", serde_json::json!({ "deleted_count": count }))
                .to_value(),
        )
    }
}

pub fn get_topic_tools(services: Arc<Services>) -> Vec<Arc<dyn ToolT>> {
    vec![
        Arc::new(CreateTopic {
            services: Arc::clone(&services),
        }),
        Arc::new(AssignTopic {
            services: Arc::clone(&services),
        }),
        Arc::new(ListTopics {
            services: Arc::clone(&services),
        }),
        Arc::new(SearchTopics {
            services: Arc::clone(&services),
        }),
        Arc::new(CorrectTopic {
            services: Arc::clone(&services),
        }),
        Arc::new(PushTopicSummary {
            services: Arc::clone(&services),
        }),
        Arc::new(FinishTopic {
            services: Arc::clone(&services),
        }),
        Arc::new(DeleteTopic {
            services: Arc::clone(&services),
        }),
    ]
}
