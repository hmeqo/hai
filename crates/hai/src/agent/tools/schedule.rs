use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    agent::{link::PlatformHandler, runtime::context::ToolContext},
    agentcore::tool::{AgentTool, MapToolErr, ToolError, tool_data},
    domain::{service::DbServices, vo::ChatId},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScheduleTaskArgs {
    /// ISO 8601 绝对时刻（如 2026-09-01T09:00:00Z）
    pub at: String,
    /// 重复间隔秒数；缺省 = 一次性任务
    pub every_secs: Option<i64>,
    /// 到点时执行的行为描述
    pub description: String,
}

/// 安排一个计划任务：到点后以「计划任务」事件唤醒并执行 description。
#[hai_macros::tool]
pub struct ScheduleTask {
    pub chat_id: ChatId,
    pub handler: Arc<dyn PlatformHandler>,
    pub db: DbServices,
}

impl ScheduleTask {
    async fn exec(&self, args: ScheduleTaskArgs) -> Result<Value, ToolError> {
        let fire_at = args
            .at
            .parse::<jiff::Timestamp>()
            .map_err(|e| ToolError::Msg(format!("无效时间: {e}")))?;
        if fire_at <= jiff::Timestamp::now() {
            return Err(ToolError::Msg("时间必须晚于当前时刻".into()));
        }
        if args.every_secs.is_some_and(|s| s <= 0) {
            return Err(ToolError::Msg("every_secs 必须为正整数".into()));
        }

        let bot_id = self.handler.bot_id().to_string();
        let task = self
            .db
            .scheduled_task
            .create(
                &bot_id,
                self.chat_id,
                &args.description,
                fire_at,
                args.every_secs,
            )
            .await
            .into_tool_err()?;

        tool_data(json!({
            "task_id": task.id,
            "fire_at": task.fire_at.to_string(),
            "every_secs": task.every_secs,
            "description": task.description,
        }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListScheduledTasksArgs {
    /// 是否包含已停用任务；缺省只看激活
    #[serde(default)]
    pub include_inactive: bool,
}

/// 列出当前对话的计划任务。
#[hai_macros::tool]
pub struct ListScheduledTasks {
    pub chat_id: ChatId,
    pub handler: Arc<dyn PlatformHandler>,
    pub db: DbServices,
}

impl ListScheduledTasks {
    async fn exec(&self, args: ListScheduledTasksArgs) -> Result<Value, ToolError> {
        let bot_id = self.handler.bot_id().to_string();
        let tasks = if args.include_inactive {
            self.db
                .scheduled_task
                .list_all(&bot_id, self.chat_id)
                .await
                .into_tool_err()?
        } else {
            self.db
                .scheduled_task
                .list_active(&bot_id, self.chat_id)
                .await
                .into_tool_err()?
        };
        let items: Vec<_> = tasks
            .into_iter()
            .map(|t| {
                json!({
                    "task_id": t.id,
                    "description": t.description,
                    "fire_at": t.fire_at.to_string(),
                    "every_secs": t.every_secs,
                    "is_active": t.is_active,
                })
            })
            .collect();
        tool_data(json!({ "tasks": items }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CancelScheduledTaskArgs {
    /// 任务 ID（schedule_task / list_scheduled_tasks 返回的 task_id）
    pub task_id: Uuid,
}

/// 取消一个计划任务。
#[hai_macros::tool]
pub struct CancelScheduledTask {
    pub chat_id: ChatId,
    pub handler: Arc<dyn PlatformHandler>,
    pub db: DbServices,
}

impl CancelScheduledTask {
    async fn exec(&self, args: CancelScheduledTaskArgs) -> Result<Value, ToolError> {
        let bot_id = self.handler.bot_id().to_string();
        self.db
            .scheduled_task
            .cancel(&bot_id, self.chat_id, args.task_id)
            .await
            .into_tool_err()?;
        tool_data(json!({ "cancelled": args.task_id }))
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    let common = (ctx.chat_id, ctx.handler.clone(), ctx.db.clone());
    vec![
        Arc::new(ScheduleTask {
            chat_id: common.0,
            handler: common.1.clone(),
            db: common.2.clone(),
        }),
        Arc::new(ListScheduledTasks {
            chat_id: common.0,
            handler: common.1.clone(),
            db: common.2.clone(),
        }),
        Arc::new(CancelScheduledTask {
            chat_id: common.0,
            handler: common.1,
            db: common.2,
        }),
    ]
}
