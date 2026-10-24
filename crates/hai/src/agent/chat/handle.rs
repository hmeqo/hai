use tokio::sync::{mpsc, oneshot};

use super::{BusyPass, scheduler::SchedulerStatus};
use crate::{
    agent::{
        chat::types::DigestRequest,
        command::digest::run::DigestReport,
        events::Doorbell,
        prompt::text::{LanguageText, PerCatalog},
    },
    config::conversation::Language,
    domain::vo::{ChatId, ConversationId, Stretch},
    error::{AppError, Result},
};

pub(crate) struct ConversationAgentStatus {
    pub scheduler: SchedulerStatus,
    pub step_count: usize,
    pub context_tokens: u32,
    pub conversation_msgs: usize,
    pub turn_elapsed_secs: Option<f64>,
    /// What the chat is busy with; `None` when it is idle.
    pub busy: Option<BusyPass>,
    pub model: String,
}

impl ConversationAgentStatus {
    fn render(&self, language: Language) -> String {
        let labels = LABELS.get(language.text_catalog());
        let sched = &self.scheduler;
        let mut lines = vec![format!("{}  {}", labels.model, self.model)];

        let runs_line = if let Some(secs) = self.turn_elapsed_secs {
            format!(
                "{}  {} · {} {:.1}s",
                labels.steps, self.step_count, labels.active, secs
            )
        } else {
            format!("{}  {}", labels.steps, self.step_count)
        };
        lines.push(runs_line);

        if self.context_tokens > 0 {
            lines.push(format!(
                "{}  {}",
                labels.tokens,
                fmt_tokens(self.context_tokens)
            ));
        }

        lines.push(format!(
            "{}  {} {}",
            labels.conversation, self.conversation_msgs, labels.messages
        ));

        let heat = format!(
            "{}  {:.2} / {:.2}",
            labels.heat, sched.heat_value, sched.heat_base
        );
        let heat_line = if sched.window_active {
            let secs = sched.window_closes_in_secs.unwrap_or(0.0) as i64;
            format!("{heat} · {} {}s", labels.window, secs)
        } else {
            heat
        };
        lines.push(heat_line);

        // A purge and a digest are not Turns and have no steps of their own; naming them is what
        // keeps "busy" readable.
        match self.busy {
            Some(BusyPass::Purge) => lines.push(format!(
                "{}  {:.1}s",
                labels.purging,
                self.turn_elapsed_secs.unwrap_or(0.0)
            )),
            Some(BusyPass::Digest) => lines.push(format!(
                "{}  {:.1}s",
                labels.digesting,
                self.turn_elapsed_secs.unwrap_or(0.0)
            )),
            Some(BusyPass::Turn) | Some(BusyPass::WrapUp) | None => {}
        }

        lines.join("\n")
    }
}

struct StatusLabels {
    model: &'static str,
    steps: &'static str,
    active: &'static str,
    tokens: &'static str,
    conversation: &'static str,
    messages: &'static str,
    heat: &'static str,
    window: &'static str,
    purging: &'static str,
    digesting: &'static str,
}

const LABELS: PerCatalog<StatusLabels> = PerCatalog {
    zh: StatusLabels {
        model: "模型",
        steps: "步数",
        active: "进行中",
        tokens: "上下文",
        conversation: "对话",
        messages: "条",
        heat: "热度",
        window: "留意",
        purging: "清理中",
        digesting: "写报告中",
    },
    en: StatusLabels {
        model: "model",
        steps: "steps",
        active: "active",
        tokens: "tokens",
        conversation: "conv",
        messages: "msgs",
        heat: "heat",
        window: "watching",
        purging: "purging",
        digesting: "digesting",
    },
};

fn fmt_tokens(n: u32) -> String {
    if n >= 1000 {
        format!("{}.{}k", n / 1000, (n % 1000) / 100)
    } else {
        n.to_string()
    }
}

#[derive(Clone)]
pub struct ConversationAgentHandle {
    conversation_id: ConversationId,
    doorbell: Doorbell,
    status_tx: mpsc::UnboundedSender<oneshot::Sender<ConversationAgentStatus>>,
    digest_tx: mpsc::UnboundedSender<DigestRequest>,
}

impl ConversationAgentHandle {
    pub(crate) fn new(
        conversation_id: ConversationId,
        doorbell: Doorbell,
        status_tx: mpsc::UnboundedSender<oneshot::Sender<ConversationAgentStatus>>,
        digest_tx: mpsc::UnboundedSender<DigestRequest>,
    ) -> Self {
        Self {
            conversation_id,
            doorbell,
            status_tx,
            digest_tx,
        }
    }

    /// Rings the Conversation's doorbell; the changes themselves are already in the database.
    pub fn wake(&self) {
        self.doorbell.notify();
    }

    /// Wakes the agent and makes this Chat's arrival eligible to steer its running Turn.
    pub fn notify_chat(&self, chat_id: ChatId) {
        self.doorbell.notify_chat(chat_id);
    }

    /// Queues one digest and hands back where its report lands. A busy chat runs it when the slot is
    /// free; the request lives in memory only, so a restart drops it.
    pub fn request_digest(
        &self,
        chat_id: ChatId,
        stretch: Stretch,
        language: Option<Language>,
    ) -> Result<oneshot::Receiver<Result<DigestReport, String>>> {
        let (reply, report) = oneshot::channel();
        self.digest_tx
            .send(DigestRequest {
                chat_id,
                stretch,
                language,
                reply,
            })
            .map_err(|_| {
                tracing::warn!(
                    conversation_id = %self.conversation_id,
                    "Failed to queue a digest: the agent is gone"
                );
                AppError::message("the chat agent is not running")
            })?;
        Ok(report)
    }

    pub async fn status_text(&self, language: Language) -> Option<String> {
        Some(self.status().await?.render(language))
    }

    async fn status(&self) -> Option<ConversationAgentStatus> {
        let (tx, rx) = oneshot::channel();
        if self.status_tx.send(tx).is_err() {
            tracing::warn!(conversation_id = %self.conversation_id, "Failed to send status query: agent dead");
            return None;
        }
        match rx.await {
            Ok(s) => Some(s),
            Err(_) => {
                tracing::warn!(conversation_id = %self.conversation_id, "Status query cancelled: agent dropped");
                None
            }
        }
    }
}
