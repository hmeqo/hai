use std::sync::Arc;

use anyhow::Result;
use autoagents::async_trait;
use autoagents::core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agent::render::{
    render_account_info, render_conversation_log, render_related_memories_section,
    render_topic_section,
};
use crate::agent::tools::util::{toolcall_anyhow_err, toolcall_err};
use crate::app::domain::model::MemoryInput;
use crate::app::service::{
    MemoryService, MessageService, PlatformService, ServiceContext, TopicService,
};
use crate::trigger::BotSignal;

// --- Send Message Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SendMessageArgs {
    #[input(description = "The content of the message")]
    pub content: String,
    #[input(description = "The UUID of the topic this message belongs to")]
    pub topic_id: Option<Uuid>,
    #[input(description = "Optional platform message ID to reply to")]
    pub reply_to_platform_id: Option<String>,
    #[input(description = "Optional list of message IDs to mark as seen")]
    pub seen_message_ids: Option<Vec<i64>>,
    #[input(description = "Optional list of message IDs to mark as replied")]
    pub replied_message_ids: Option<Vec<i64>>,
}

#[tool(
    name = "send_message",
    description = "在群里发言（直接发言或回复）。同时可标记对应的消息。",
    input = SendMessageArgs,
)]
pub struct SendMessage {
    pub chat_id: i64,
    pub signal_tx: mpsc::UnboundedSender<BotSignal>,
    pub topic_service: Arc<TopicService>,
}

#[async_trait]
impl ToolRuntime for SendMessage {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: SendMessageArgs = serde_json::from_value(args)?;

        if let Some(ids) = &typed_args.seen_message_ids {
            let _ = self
                .topic_service
                .mark_as_seen(ids)
                .await
                .map_err(toolcall_anyhow_err)?;
        }

        if let Some(ids) = &typed_args.replied_message_ids {
            let _ = self
                .topic_service
                .mark_as_replied(ids)
                .await
                .map_err(toolcall_anyhow_err)?;
        }

        let _ = self.signal_tx.send(BotSignal::SendMessage {
            chat_id: self.chat_id,
            content: typed_args.content,
            topic_id: typed_args.topic_id,
            reply_to_platform_id: typed_args.reply_to_platform_id,
        });
        Ok(Value::String("消息已发送".into()))
    }
}

// --- Organize Messages Tool ---

#[derive(Debug, Serialize, Deserialize)]
pub struct TopicAssignment {
    pub message_ids: Vec<i64>,
    pub topic_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TopicCreation {
    pub message_ids: Vec<i64>,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct OrganizeMessagesArgs {
    #[input(description = "The internal chat ID")]
    pub chat_id: i64,
    #[input(description = "Optional list of message IDs to mark as seen")]
    pub seen_message_ids: Option<Vec<i64>>,
    #[input(description = "Optional list of message IDs to mark as replied")]
    pub replied_message_ids: Option<Vec<i64>>,
    #[input(description = "Optional list of topic assignments")]
    pub assignments: Option<Vec<TopicAssignment>>,
    #[input(description = "Optional list of topic creations")]
    pub creations: Option<Vec<TopicCreation>>,
}

#[tool(
    name = "organize_messages",
    description = "批量标记消息为已阅/已回复，或将消息归类到话题。",
    input = OrganizeMessagesArgs,
)]
pub struct OrganizeMessages {
    pub topic_service: Arc<TopicService>,
}

#[async_trait]
impl ToolRuntime for OrganizeMessages {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: OrganizeMessagesArgs = serde_json::from_value(args)?;
        let mut results = Vec::new();

        if let Some(ids) = &typed_args.seen_message_ids {
            let count = self
                .topic_service
                .mark_as_seen(ids)
                .await
                .map_err(toolcall_anyhow_err)?;
            results.push(format!("已标记 {} 条消息为已阅", count));
        }

        if let Some(ids) = &typed_args.replied_message_ids {
            let count = self
                .topic_service
                .mark_as_replied(ids)
                .await
                .map_err(toolcall_anyhow_err)?;
            results.push(format!("已标记 {} 条消息为已回复", count));
        }

        if let Some(assignments) = &typed_args.assignments {
            for assignment in assignments {
                self.topic_service
                    .assign_topic(&assignment.message_ids, assignment.topic_id)
                    .await
                    .map_err(toolcall_anyhow_err)?;
                results.push(format!(
                    "已将 {} 条消息归类到话题 {}",
                    assignment.message_ids.len(),
                    assignment.topic_id
                ));
            }
        }

        if let Some(creations) = &typed_args.creations {
            for creation in creations {
                let topic = self
                    .topic_service
                    .create_topic(
                        typed_args.chat_id,
                        Some(&creation.title),
                        &creation.message_ids,
                        None,
                    )
                    .await
                    .map_err(toolcall_anyhow_err)?;
                results.push(format!(
                    "新话题创建成功：\n{}",
                    render_topic_section(&[topic])
                ));
            }
        }

        if results.is_empty() {
            Ok(Value::String("未执行任何整理操作".into()))
        } else {
            Ok(Value::String(results.join("\n")))
        }
    }
}

// --- List Topics Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct ListTopicsArgs {
    #[input(description = "The internal chat ID")]
    pub chat_id: i64,
    #[input(description = "Optional status to filter by ('active', 'closed')")]
    pub status: Option<String>,
    #[input(description = "Optional limit for the number of results (default 10)")]
    pub limit: Option<i64>,
    #[input(description = "Optional offset for pagination")]
    pub offset: Option<i64>,
}

#[tool(
    name = "list_topics",
    description = "查看话题列表，可按状态筛选或翻页。",
    input = ListTopicsArgs,
)]
pub struct ListTopics {
    pub topic_service: Arc<TopicService>,
}

#[async_trait]
impl ToolRuntime for ListTopics {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: ListTopicsArgs = serde_json::from_value(args)?;
        let limit = typed_args.limit.unwrap_or(10);
        let offset = typed_args.offset.unwrap_or(0);

        let topics = self
            .topic_service
            .list_topics(
                typed_args.chat_id,
                typed_args.status.as_deref(),
                limit,
                offset,
            )
            .await
            .map_err(toolcall_anyhow_err)?;

        let mut result = String::from("话题列表：\n");
        if topics.is_empty() {
            result.push_str("没有找到话题。");
        } else {
            result.push_str(&render_topic_section(&topics));
        }
        Ok(Value::String(result))
    }
}

// --- Search Topics Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SearchTopicsArgs {
    #[input(description = "The internal chat ID")]
    pub chat_id: i64,
    #[input(description = "The search query")]
    pub query: String,
    #[input(description = "Optional limit for the number of results (default 10)")]
    pub limit: Option<i64>,
}

#[tool(
    name = "search_topics",
    description = "搜索话题：根据关键词查找相关话题。",
    input = SearchTopicsArgs,
)]
pub struct SearchTopics {
    pub topic_service: Arc<TopicService>,
}

#[async_trait]
impl ToolRuntime for SearchTopics {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: SearchTopicsArgs = serde_json::from_value(args)?;
        let limit = typed_args.limit.unwrap_or(10);

        let topics = self
            .topic_service
            .search_topics_by_query(typed_args.chat_id, &typed_args.query, limit)
            .await
            .map_err(toolcall_anyhow_err)?;
        let topic_entities: Vec<_> = topics.into_iter().map(|t| t.topic).collect();

        let results = render_topic_section(&topic_entities);
        Ok(Value::String(results))
    }
}
// --- Update Topic Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct UpdateTopicArgs {
    #[input(description = "The UUID of the topic to update")]
    pub topic_id: Uuid,
    #[input(description = "Optional new title for the topic")]
    pub title: Option<String>,
    #[input(description = "Optional new summary for the topic")]
    pub summary: Option<String>,
}

#[tool(
    name = "update_topic",
    description = "更新话题信息：改名或更新进行中的摘要。若要结项请使用 finish_topic。",
    input = UpdateTopicArgs,
)]
pub struct UpdateTopic {
    pub topic_service: Arc<TopicService>,
}

#[async_trait]
impl ToolRuntime for UpdateTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: UpdateTopicArgs = serde_json::from_value(args)?;
        let mut results = Vec::new();

        if let Some(title) = &typed_args.title {
            self.topic_service
                .update_title(typed_args.topic_id, title)
                .await
                .map_err(toolcall_anyhow_err)?;
            results.push("标题已更新");
        }

        if let Some(summary) = &typed_args.summary {
            self.topic_service
                .update_summary(typed_args.topic_id, summary)
                .await
                .map_err(toolcall_anyhow_err)?;
            results.push("摘要已更新");
        }

        if results.is_empty() {
            Ok(Value::String("未执行任何更新".into()))
        } else {
            Ok(Value::String(results.join("，")))
        }
    }
}

// --- Finish Topic Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct FinishTopicArgs {
    #[input(description = "The UUID of the topic to close")]
    pub topic_id: Uuid,
    #[input(description = "Optional new title for the topic")]
    pub title: Option<String>,
    #[input(description = "Final summary of the topic")]
    pub summary: String,
}

#[tool(
    name = "finish_topic",
    description = "结项话题：写入最终摘要并将话题标记为 closed。",
    input = FinishTopicArgs,
)]
pub struct FinishTopic {
    pub topic_service: Arc<TopicService>,
}

#[async_trait]
impl ToolRuntime for FinishTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: FinishTopicArgs = serde_json::from_value(args)?;

        if let Some(title) = &typed_args.title {
            self.topic_service
                .update_title(typed_args.topic_id, title)
                .await
                .map_err(toolcall_anyhow_err)?;
        }

        self.topic_service
            .finish_topic(typed_args.topic_id, &typed_args.summary)
            .await
            .map_err(toolcall_anyhow_err)?;
        Ok(Value::String("话题已结项".into()))
    }
}

// --- Delete Topic Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct DeleteTopicArgs {
    #[input(description = "The UUID of the topic to delete")]
    pub topic_id: Uuid,
}

#[tool(
    name = "delete_topic",
    description = "删除错误的话题。关联的消息会被解除关联，笔记会被删除。",
    input = DeleteTopicArgs,
)]
pub struct DeleteTopic {
    pub topic_service: Arc<TopicService>,
}

#[async_trait]
impl ToolRuntime for DeleteTopic {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: DeleteTopicArgs = serde_json::from_value(args)?;
        let count = self
            .topic_service
            .delete_topic(typed_args.topic_id)
            .await
            .map_err(toolcall_anyhow_err)?;
        Ok(Value::String(format!("已删除话题 {} 条", count)))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordMemoryCategory {
    UserFact,
    Knowledge,
    Note,
    ChatRule,
}

// --- Manage Memory Tool ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOperation {
    Create {
        category: RecordMemoryCategory,
        content: String,
        account_id: Option<i64>,
        topic_id: Option<Uuid>,
        message_id: Option<i64>,
    },
    Update {
        id: Uuid,
        category: RecordMemoryCategory,
        content: Option<String>,
        importance: Option<i32>,
    },
}

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct ManageMemoryArgs {
    #[input(description = "The internal chat ID")]
    pub chat_id: i64,
    #[input(description = "The operation to perform")]
    pub operation: MemoryOperation,
}

#[tool(
    name = "manage_memory",
    description = "管理记忆：记录新信息（群友特征、知识、笔记）或更新现有记忆。",
    input = ManageMemoryArgs,
)]
pub struct ManageMemory {
    pub memory_service: Arc<MemoryService>,
}

#[async_trait]
impl ToolRuntime for ManageMemory {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: ManageMemoryArgs = serde_json::from_value(args)?;
        let input = match typed_args.operation {
            MemoryOperation::Create {
                category,
                content,
                account_id,
                topic_id,
                message_id,
            } => match category {
                RecordMemoryCategory::UserFact => {
                    let account_id = account_id
                        .ok_or_else(|| toolcall_err("account_id is required for 'user_fact'"))?;
                    MemoryInput::CreateUserFact {
                        account_id,
                        chat_id: typed_args.chat_id,
                        content,
                    }
                }
                RecordMemoryCategory::Knowledge => MemoryInput::CreateKnowledge {
                    chat_id: typed_args.chat_id,
                    content,
                },
                RecordMemoryCategory::Note => MemoryInput::CreateAgentNote {
                    chat_id: typed_args.chat_id,
                    topic_id,
                    message_id,
                    content,
                },
                RecordMemoryCategory::ChatRule => MemoryInput::UpsertChatRule {
                    chat_id: typed_args.chat_id,
                    content,
                },
            },
            MemoryOperation::Update {
                id,
                category,
                content,
                importance,
            } => match category {
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
                    chat_id: typed_args.chat_id,
                    content: content
                        .ok_or_else(|| toolcall_err("content is required for 'chat_rule'"))?,
                },
            },
        };

        self.memory_service
            .save_memory(input)
            .await
            .map_err(toolcall_anyhow_err)?;

        Ok(Value::String("记忆已保存".into()))
    }
}

// --- Search Memory Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SearchMemoryArgs {
    #[input(description = "The internal chat ID")]
    pub chat_id: i64,
    #[input(description = "The search query")]
    pub query: String,
    #[input(description = "Optional limit for the number of results (default 10)")]
    pub limit: Option<i64>,
}

#[tool(
    name = "search_memory",
    description = "搜索记忆：群友信息、知识或笔记。",
    input = SearchMemoryArgs,
)]
pub struct SearchMemory {
    pub memory_service: Arc<MemoryService>,
}

#[async_trait]
impl ToolRuntime for SearchMemory {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: SearchMemoryArgs = serde_json::from_value(args)?;
        let limit = typed_args.limit.unwrap_or(10);

        let memories = self
            .memory_service
            .search_knowledge(typed_args.chat_id, &typed_args.query, limit)
            .await
            .map_err(toolcall_anyhow_err)?;

        let results = render_related_memories_section(&memories);
        Ok(Value::String(results))
    }
}

// --- Delete Memory Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct DeleteMemoryArgs {
    #[input(description = "The UUID of the memory/note to delete")]
    pub id: Uuid,
}

#[tool(
    name = "delete_memory",
    description = "删除记忆或笔记。",
    input = DeleteMemoryArgs,
)]
pub struct DeleteMemory {
    pub memory_service: Arc<MemoryService>,
}

#[async_trait]
impl ToolRuntime for DeleteMemory {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: DeleteMemoryArgs = serde_json::from_value(args)?;
        let count = self.memory_service
            .delete(typed_args.id)
            .await
            .map_err(toolcall_anyhow_err)?;
        Ok(Value::String(format!("已删除记忆/笔记 {} 条", count)))
    }
}

// --- Get Account Info Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct GetAccountInfoArgs {
    #[input(description = "The internal account ID to query")]
    pub account_id: i64,
}

#[tool(
    name = "get_account_info",
    description = "获取群友的账号信息（用户名、平台、身份等）。",
    input = GetAccountInfoArgs,
)]
pub struct GetAccountInfo {
    pub platform_service: Arc<PlatformService>,
}

#[async_trait]
impl ToolRuntime for GetAccountInfo {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: GetAccountInfoArgs = serde_json::from_value(args)?;
        let account = self
            .platform_service
            .get_account_by_id(typed_args.account_id)
            .await
            .map_err(toolcall_anyhow_err)?
            .ok_or_else(|| toolcall_err("账号不存在"))?;

        Ok(Value::String(render_account_info(&account)))
    }
}

// --- Get History Messages Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct GetHistoryMessagesArgs {
    #[input(description = "The internal chat ID")]
    pub chat_id: i64,
    #[input(
        description = "Number of history messages to fetch (already-processed messages only, excluding pending)"
    )]
    pub limit: i64,
}

#[tool(
    name = "get_history_messages",
    description = "获取更多历史消息（已处理，不含当前 pending）。当需要了解话题背景或核实上文时使用。",
    input = GetHistoryMessagesArgs,
)]
pub struct GetHistoryMessages {
    pub message_service: Arc<MessageService>,
    pub bot_account_id: i64,
}

#[async_trait]
impl ToolRuntime for GetHistoryMessages {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: GetHistoryMessagesArgs = serde_json::from_value(args)?;
        let limit = typed_args.limit.clamp(1, 100);

        let messages = self
            .message_service
            .get_history_messages(typed_args.chat_id, limit)
            .await
            .map_err(toolcall_anyhow_err)?;

        if messages.is_empty() {
            return Ok(Value::String("没有更多历史消息".into()));
        }

        // 渲染时没有完整的账号/话题列表，传空 slice，渲染函数会回退到 ID 展示
        let rendered = render_conversation_log(&messages, &[], &[], self.bot_account_id);
        Ok(Value::String(rendered))
    }
}

pub fn get_main_agent_tools(
    services: Arc<ServiceContext>,
    chat_id: i64,
    bot_account_id: i64,
    signal_tx: mpsc::UnboundedSender<BotSignal>,
) -> Vec<Arc<dyn ToolT>> {
    vec![
        Arc::new(SendMessage {
            chat_id,
            signal_tx: signal_tx.clone(),
            topic_service: Arc::clone(&services.topic),
        }),
        Arc::new(OrganizeMessages {
            topic_service: Arc::clone(&services.topic),
        }),
        Arc::new(ListTopics {
            topic_service: Arc::clone(&services.topic),
        }),
        Arc::new(SearchTopics {
            topic_service: Arc::clone(&services.topic),
        }),
        Arc::new(UpdateTopic {
            topic_service: Arc::clone(&services.topic),
        }),
        Arc::new(FinishTopic {
            topic_service: Arc::clone(&services.topic),
        }),
        Arc::new(DeleteTopic {
            topic_service: Arc::clone(&services.topic),
        }),
        Arc::new(ManageMemory {
            memory_service: Arc::clone(&services.memory),
        }),
        Arc::new(SearchMemory {
            memory_service: Arc::clone(&services.memory),
        }),
        Arc::new(DeleteMemory {
            memory_service: Arc::clone(&services.memory),
        }),
        Arc::new(GetAccountInfo {
            platform_service: Arc::clone(&services.platform),
        }),
        Arc::new(GetHistoryMessages {
            message_service: Arc::clone(&services.message),
            bot_account_id,
        }),
    ]
}
