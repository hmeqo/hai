use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    agent::{
        context::topic_section,
        runtime::context::ToolContext,
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
    pub title: String,
    pub summary: String,
    #[serde(default, deserialize_with = "deserialize_option_lenient_i64_vec")]
    pub message_ids: Option<Vec<i64>>,
}

/// 开始讨论以前没聊过的话题时创建话题，把消息归到话题下，方便以后关联。
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
    pub topic_id: Uuid,
    #[serde(deserialize_with = "deserialize_lenient_i64_vec")]
    pub message_ids: Vec<i64>,
}

/// 把消息归到已有话题下。
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
            .assign_topic(&msg_ids, crate::domain::vo::TopicId::from(args.topic_id))
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchTopicsArgs {
    /// 语义关键词
    pub query: Option<String>,
    /// 起始时间（ISO 8601）
    pub since: Option<String>,
    /// 截止时间（ISO 8601）
    pub until: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// 按关键词或时间范围查找话题，两者可叠加过滤。
#[hai_macros::tool]
pub struct SearchTopics {
    pub chat_id: ChatId,
    pub services: DbServices,
}

impl SearchTopics {
    async fn exec(&self, args: SearchTopicsArgs) -> Result<Value, ToolError> {
        let limit = args.limit.unwrap_or(10).max(1);
        let parse = |s: &str| {
            s.parse::<jiff::Timestamp>()
                .map_err(|e| ToolError::Msg(format!("invalid time `{s}`: {e}")))
        };
        let since = args.since.as_deref().map(parse).transpose()?;
        let until = args.until.as_deref().map(parse).transpose()?;
        let offset = args.offset.unwrap_or(0).max(0);
        let topics = self
            .services
            .topic
            .search_topics(
                self.chat_id,
                args.query.as_deref(),
                since,
                until,
                limit,
                offset,
            )
            .await
            .into_tool_err()?;
        let entities: Vec<_> = topics.into_iter().map(|t| t.topic).collect();
        tool_data(serde_json::json!({ "topics": render_json(topic_section(&entities)) }))
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CorrectTopicArgs {
    pub topic_id: Uuid,
    pub title: Option<String>,
    /// 覆盖已有摘要
    pub summary: Option<String>,
}

/// 修正话题的标题或摘要。
#[hai_macros::tool]
pub struct CorrectTopic {
    pub services: DbServices,
}

impl CorrectTopic {
    async fn exec(&self, args: CorrectTopicArgs) -> Result<Value, ToolError> {
        self.services
            .topic
            .update_topic(
                crate::domain::vo::TopicId::from(args.topic_id),
                args.title.as_deref(),
                args.summary.as_deref(),
            )
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AppendTopicSummaryArgs {
    pub topic_id: Uuid,
    /// 不重复已有信息
    pub summary: String,
}

/// 话题有新进展时追加内容摘要（不覆盖已有信息），保持话题跟上对话。
#[hai_macros::tool]
pub struct AppendTopicSummary {
    pub services: DbServices,
}

impl AppendTopicSummary {
    async fn exec(&self, args: AppendTopicSummaryArgs) -> Result<Value, ToolError> {
        self.services
            .topic
            .append_summary(
                crate::domain::vo::TopicId::from(args.topic_id),
                &args.summary,
            )
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CloseTopicArgs {
    pub topic_id: Uuid,
    pub title: Option<String>,
    pub summary: String,
}

/// 话题聊完了归档，附最终摘要（背景+历程+结论）。
#[hai_macros::tool]
pub struct CloseTopic {
    pub services: DbServices,
}

impl CloseTopic {
    async fn exec(&self, args: CloseTopicArgs) -> Result<Value, ToolError> {
        self.services
            .topic
            .update_topic(
                crate::domain::vo::TopicId::from(args.topic_id),
                args.title.as_deref(),
                None,
            )
            .await
            .into_tool_err()?;
        self.services
            .topic
            .close_topic(
                crate::domain::vo::TopicId::from(args.topic_id),
                &args.summary,
            )
            .await
            .into_tool_err()?;
        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DeleteTopicArgs {
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
            .delete_topic(crate::domain::vo::TopicId::from(args.topic_id))
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
        Arc::new(SearchTopics {
            chat_id: ctx.chat_id,
            services: ctx.db.clone(),
        }),
        Arc::new(CorrectTopic {
            services: ctx.db.clone(),
        }),
        Arc::new(AppendTopicSummary {
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
