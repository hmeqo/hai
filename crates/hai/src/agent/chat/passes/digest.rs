//! The digest pass: its start and its report completion. It writes no chat data.

use std::sync::Arc;

use super::{super::ConversationAgent, BusyRun, BusySignal, DigestRequest};
use crate::{agent::command::digest, agentcore::tool::AgentTool, error::Result};

impl ConversationAgent {
    /// One digest: a pass of its own over a stretch of the target Chat's record, holding the
    /// Conversation Busy while it runs and writing no chat data. The report travels back over the
    /// request's own reply, because there is no row it could be read from.
    pub(crate) async fn start_digest(&mut self, request: DigestRequest) {
        let settings = match self
            .engine
            .runtime
            .app
            .settings
            .snapshot(self.conversation_id)
            .await
        {
            Ok(settings) => settings,
            Err(e) => {
                let _ = request.reply.send(Err(e.to_string()));
                return;
            }
        };
        let language = request.language.unwrap_or(settings.language);
        let selected = match self.selected_model(settings.model).await {
            Ok(model) => model,
            Err(e) => {
                let _ = request.reply.send(Err(e.to_string()));
                return;
            }
        };
        let content_policy = settings.content_policy;
        let chat = match self.chats.surface(request.chat_id) {
            Ok(chat) => chat,
            Err(e) => {
                let _ = request.reply.send(Err(e.to_string()));
                return;
            }
        };
        // The digest may only read, so it is handed the reads and nothing else: the schema it sees
        // is the surface it may use.
        let tools: Arc<[Arc<dyn AgentTool>]> = chat
            .tools()
            .iter()
            .filter(|tool| tool.reads_only())
            .cloned()
            .collect();
        let runtime = self.engine.runtime.clone();
        let handle = chat.tool().chat.clone();
        let shutdown = self.engine.shutdown();
        let (chat_id, chat_type, stretch) =
            (request.chat_id, chat.roster().chat_type, request.stretch);
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let signal = tokio::select! {
                biased;
                _ = shutdown.cancelled() => BusySignal::Cancelled,
                report = digest::run(digest::DigestPass {
                    runtime: &runtime,
                    chat_id,
                    chat_type,
                    language,
                    model: &selected,
                    content_policy,
                    handle: &handle,
                    stretch,
                    tools: tools.as_ref(),
                }) => BusySignal::Digested(report),
            };
            let _ = tx.send(signal);
        });
        self.pass.begin(BusyRun::digest(rx, request.reply));
    }

    /// A digest ends: its report goes back where the request came from, and no chat data is written.
    /// There is no retry to owe — the person on the page has a retry of their own.
    pub(crate) fn on_digest_done(
        &mut self,
        reply: Option<tokio::sync::oneshot::Sender<Result<digest::run::DigestReport, String>>>,
        result: Result<digest::run::DigestReport, String>,
    ) {
        if let Some(reply) = reply {
            let _ = reply.send(result);
        }
    }
}
