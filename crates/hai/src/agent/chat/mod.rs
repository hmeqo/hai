mod attention;
mod conversation;
mod dispatch;
mod event_loop;
mod handle;
mod scheduler;
pub(crate) mod types;

use std::{collections::HashSet, sync::Arc};

pub use handle::ChatAgentRef;
use tokio::{
    sync::mpsc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) use self::conversation::Conversation;
use self::{
    scheduler::EventScheduler,
    types::{BusySignal, DigestRequest},
};
use super::{
    environment::{sandbox::Sandbox, workspace::Workspace},
    process::AgentRuntime,
    run::{
        ToolContext,
        engine::{self, AgentEngine},
    },
};
use crate::{
    agent::{
        channel::ChannelHandle,
        command::{digest, digest::run::DigestReport, purge},
    },
    agentcore::tool::AgentTool,
    domain::{
        model::ChatType,
        vo::{ChatEntry, ChatId, EntryKind, PurgeSpec},
    },
    error::{AppError, Result},
};

/// What the work behind a Busy state is: a Turn, the chapter wrap-up, a purge, or a digest. A
/// status query and the state's own round trip both carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BusyPass {
    Turn,
    WrapUp,
    Purge,
    Digest,
}

enum ChatAgentState {
    Idle,
    Busy {
        result_rx: tokio::sync::oneshot::Receiver<crate::agent::chat::types::BusySignal>,
        started_at: Instant,
        pass: BusyPass,
    },
}

impl ChatAgentState {
    fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Idle)
    }

    fn busy(
        result_rx: tokio::sync::oneshot::Receiver<crate::agent::chat::types::BusySignal>,
        started_at: Instant,
        pass: BusyPass,
    ) -> Self {
        Self::Busy {
            result_rx,
            started_at,
            pass,
        }
    }
}

/// Re-dispatches allowed per batch: three attempts in total, the first one included.
const MAX_TURN_RETRIES: u8 = 2;

/// Wait before the attempt that follows a failure that is not the model's own turn.
const RETRY_BACKOFF: Duration = Duration::from_secs(1);

struct RetryBudget {
    attempts_left: u8,
    /// When the owed attempt may go; `None` when nothing is owed. Reading it does not spend it — it
    /// is cleared where the attempt is dispatched, so a wait that is interrupted keeps the debt.
    owed_at: Option<Instant>,
}

impl RetryBudget {
    fn new() -> Self {
        Self {
            attempts_left: MAX_TURN_RETRIES,
            owed_at: None,
        }
    }

    /// A new reason to spend a turn leaves the budget whole.
    fn reopen(&mut self) {
        *self = Self::new();
    }

    /// A failed attempt: spends one attempt and returns whether the budget still owes another one,
    /// which may go at once — the model's own turn failed.
    fn fail_now(&mut self) -> bool {
        let owed = self.attempts_left > 0;
        self.attempts_left = self.attempts_left.saturating_sub(1);
        if owed {
            self.owed_at = Some(Instant::now());
        }
        owed
    }

    /// A failed attempt that waits out the backoff before the one it owes — the failure happened
    /// before the turn started.
    fn fail_after_backoff(&mut self) -> bool {
        let owed = self.fail_now();
        if owed {
            self.owed_at = Some(Instant::now() + RETRY_BACKOFF);
        }
        owed
    }

    fn remaining(&self) -> u8 {
        self.attempts_left
    }

    /// When the owed attempt may go.
    fn owed_at(&self) -> Option<Instant> {
        self.owed_at
    }

    /// The owed attempt went out; nothing is owed until another failure.
    fn dispatched(&mut self) {
        self.owed_at = None;
    }
}

pub(super) struct ChatAgent {
    engine: AgentEngine,
    /// The token this chat's channel stops on.
    shutdown: CancellationToken,
    /// The tool surface of this chat, built once: its inputs are per-chat constants.
    tools: Arc<[Arc<dyn AgentTool>]>,
    tool: ToolContext,
    schedule: EventScheduler,
    conversation: Conversation,
    batch: Vec<ChatEntry>,
    /// Entry ids the dispatch policy has already been told about.
    fed_entries: HashSet<Uuid>,
    state: ChatAgentState,
    /// The chat this agent runs in: everything that addresses it by id reads this.
    chat_id: ChatId,
    chat_type: ChatType,
    retry: RetryBudget,
    /// The digests people asked for, in the order they asked; the chat runs one when it is free.
    digest_rx: mpsc::UnboundedReceiver<DigestRequest>,
    /// Where the digest now running sends its report, if one is.
    digest_reply: Option<tokio::sync::oneshot::Sender<Result<DigestReport, String>>>,
    /// Deadline before which a wrap-up start that failed its precondition must not be retried.
    wrap_up_retry_at: Option<Instant>,
    /// Deadline before which a purge that failed must not be tried again: one broken purge must not
    /// spin on every tick.
    purge_retry_at: Option<Instant>,
    /// The change the purge now running answers for; a failure spends this one and nothing else.
    purge_entry: Option<Uuid>,
}

impl ChatAgent {
    pub async fn new(
        runtime: AgentRuntime,
        chat: ChannelHandle,
        shutdown: CancellationToken,
        digest_rx: mpsc::UnboundedReceiver<DigestRequest>,
    ) -> Result<Self> {
        let chat_id = chat.chat_id();
        let workspace = Workspace::create(chat_id).await?;
        let cfg = &runtime.cfg().sandbox;
        let sandbox = Arc::new(if cfg.enabled {
            Sandbox::container(cfg, runtime.skill_manager.roots().to_vec(), workspace)
        } else {
            Sandbox::host(workspace, cfg.timeout_secs)
        });
        let engine = AgentEngine::new(&runtime, shutdown.clone());

        let (record, row) = tokio::try_join!(
            runtime.services().conversation.get(chat_id),
            runtime.services().platform.get_chat_by_id(chat_id),
        )?;
        let conversation = match record {
            Some(r) => Conversation::from_snapshot(
                runtime.services().conversation.restore(&r)?,
                Some(r.updated_at),
            ),
            None => Conversation::new(),
        };
        let row = row.ok_or_else(|| AppError::message(format!("Chat {chat_id} not found")))?;
        let chat_type = row.chat_type;
        let batch = runtime
            .app
            .services
            .conversation
            .pending_entries(chat_id)
            .await?;

        let tool = ToolContext {
            chat,
            services: runtime.services().clone(),
            sandbox,
            skill_manager: runtime.skill_manager.clone(),
            multimodal: runtime.multimodal().clone(),
        };
        let tools = engine::mounted_tools(&runtime, &tool);

        let schedule = EventScheduler::new(&runtime.cfg().agent.attention);
        Ok(Self {
            engine,
            shutdown,
            tools,
            tool,
            schedule,
            conversation,
            batch,
            fed_entries: HashSet::new(),
            state: ChatAgentState::Idle,
            chat_id,
            chat_type,
            retry: RetryBudget::new(),
            digest_rx,
            digest_reply: None,
            wrap_up_retry_at: None,
            purge_retry_at: None,
            purge_entry: None,
        })
    }

    /// Writes the snapshot and consumes the batch in one statement; a failure leaves both alone, so
    /// the changes are read again rather than lost.
    async fn persist_conversation(&self, consumed: &[Uuid]) -> Result<()> {
        let snap = self.conversation.snapshot();
        self.engine
            .runtime
            .app
            .services
            .conversation
            .save(&snap, self.chat_id, consumed)
            .await
            .map_err(|e| {
                tracing::warn!(%self.chat_id, "Failed to persist conversation: {e}");
                e
            })
    }

    /// Leaves Busy: the idle clock restarts here.
    fn end_run(&mut self) {
        self.schedule.mark_run_end();
        // A digest answers its own reply where it lands; one that never got there drops the sender,
        // which is what tells the page the pass is gone.
        self.digest_reply = None;
        self.state = ChatAgentState::Idle;
    }

    /// One purge: not a Turn and not a role, a background task of its own that owns the chat while it
    /// runs — an arrival rings the doorbell and waits its turn. The change that asked for it is
    /// consumed by the pass itself, failure included, so a broken purge is not read again at once;
    /// nothing else in the batch is spent.
    fn start_purge(&mut self, spec: PurgeSpec, entry_id: Uuid) {
        self.purge_entry = Some(entry_id);
        let tool = self.tool.clone();
        let events = self.engine.runtime.event_bus().clone();
        let shutdown = self.shutdown.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let signal = tokio::select! {
                biased;
                _ = shutdown.cancelled() => BusySignal::Cancelled,
                report = purge::run(&tool, &events, spec, entry_id) => BusySignal::Purged(report),
            };
            let _ = tx.send(signal);
        });
        self.state = ChatAgentState::busy(rx, Instant::now(), BusyPass::Purge);
    }

    /// One digest: a pass of its own over a stretch of the chat's own record, holding the chat Busy
    /// while it runs and writing no chat data. The report travels back over the request's own
    /// reply, because there is no row it could be read from.
    async fn start_digest(&mut self, request: DigestRequest) {
        let language = match request.language {
            Some(language) => language,
            None => match self.language().await {
                Ok(language) => language,
                Err(e) => {
                    tracing::warn!(%self.chat_id, "language lookup failed: {e}");
                    let _ = request.reply.send(Err(e.to_string()));
                    return;
                }
            },
        };
        // The digest may only read, so it is handed the reads and nothing else: the schema it sees
        // is the surface it may use.
        let tools: Arc<[Arc<dyn AgentTool>]> = self
            .tools
            .iter()
            .filter(|tool| tool.reads_only())
            .cloned()
            .collect();
        let runtime = self.engine.runtime.clone();
        let handle = self.tool.chat.clone();
        let shutdown = self.shutdown.clone();
        let (chat_id, chat_type, stretch) = (self.chat_id, self.chat_type, request.stretch);
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
                    handle: &handle,
                    stretch,
                    tools: tools.as_ref(),
                }) => BusySignal::Digested(report),
            };
            let _ = tx.send(signal);
        });
        self.digest_reply = Some(request.reply);
        self.state = ChatAgentState::busy(rx, Instant::now(), BusyPass::Digest);
    }

    /// A digest ends: its report goes back where the request came from, and no chat data is written.
    /// There is no retry to owe — the person on the page has a retry of their own.
    fn on_digest_done(&mut self, result: Result<DigestReport, String>) {
        if let Some(reply) = self.digest_reply.take() {
            let _ = reply.send(result);
        }
        self.end_run();
    }

    /// A failure before the Turn started: an owed attempt waits out the backoff instead of going at
    /// once. A spent budget leaves the batch waiting for a new change (`docs/topics/chat-agent.md`).
    fn stand_down(&mut self) {
        if self.retry.fail_after_backoff() {
            tracing::warn!(%self.chat_id, "Re-dispatching after a failure before the turn started");
        } else {
            tracing::warn!(%self.chat_id, "Retry budget spent; the batch waits for a new change");
        }
        self.state = ChatAgentState::Idle;
    }

    /// Reads the accumulation and tells the dispatch policy about the arrivals it has not seen yet:
    /// one feed per entry, so a quiet period already running is not re-armed on every tick; a fresh
    /// change also reopens the retry budget.
    async fn refresh_batch(&mut self) -> Result<()> {
        let entries = self
            .engine
            .runtime
            .app
            .services
            .conversation
            .pending_entries(self.chat_id)
            .await?;

        let fresh: Vec<&ChatEntry> = entries
            .iter()
            .filter(|entry| !self.fed_entries.contains(&entry.id))
            .collect();
        if !fresh.is_empty() {
            self.retry.reopen();
        }
        for entry in fresh
            .iter()
            .filter(|entry| !matches!(entry.kind, EntryKind::Purge(_)))
        {
            self.schedule.on_event(entry.audience);
        }

        self.fed_entries = entries.iter().map(|entry| entry.id).collect();
        self.batch = entries;
        Ok(())
    }

    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(
            self.engine
                .runtime
                .app
                .cfg
                .agent
                .context
                .session_idle_timeout_secs,
        )
    }
}

/// The changes a Turn consumes: what it read, minus a purge, which the pass that runs it consumes
/// itself. A Turn that happens to carry one leaves it for that pass.
fn consumed_entries(batch: &[ChatEntry]) -> Vec<Uuid> {
    batch
        .iter()
        .filter(|entry| !matches!(entry.kind, EntryKind::Purge(_)))
        .map(|entry| entry.id)
        .collect()
}

/// The purge this batch carries, if any: a change of the chat's own, never a reason to read.
fn pending_purge(batch: &[ChatEntry]) -> Option<(Uuid, PurgeSpec)> {
    batch.iter().find_map(|entry| match &entry.kind {
        EntryKind::Purge(spec) => Some((entry.id, *spec)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ChatEntry, Instant, MAX_TURN_RETRIES, RetryBudget, consumed_entries, pending_purge,
    };
    use crate::domain::vo::{
        AgentCommand, EntryAudience, EntryKind, PurgeSpec, PurgeSurfaces, Stretch,
    };

    fn entry(kind: EntryKind) -> ChatEntry {
        ChatEntry::new(EntryAudience::Agent, kind, jiff::Timestamp::now())
    }

    /// A Turn consumes what it read; a purge is consumed by the pass that runs it, not by a Turn that
    /// happened to carry it.
    #[test]
    fn a_turn_does_not_consume_a_purge() {
        let command = entry(EntryKind::Command(AgentCommand::OrganizeMemory));
        let purge = entry(EntryKind::Purge(PurgeSpec {
            range: Stretch::All,
            surfaces: PurgeSurfaces::all(),
        }));

        assert_eq!(
            consumed_entries(&[command.clone(), purge]),
            vec![command.id]
        );
    }

    /// The purge is found wherever it sits in the batch, and a batch without one has none.
    #[test]
    fn a_pending_purge_is_found_in_a_mixed_batch() {
        let spec = PurgeSpec {
            range: Stretch::All,
            surfaces: PurgeSurfaces::all(),
        };
        let purge = entry(EntryKind::Purge(spec));
        let command = entry(EntryKind::Command(AgentCommand::OrganizeMemory));

        assert_eq!(
            pending_purge(&[purge.clone(), command.clone()]),
            Some((purge.id, spec))
        );
        assert_eq!(pending_purge(&[command]), None);
    }

    #[test]
    fn a_model_failure_retries_at_once() {
        let mut retry = RetryBudget::new();
        assert!(retry.fail_now(), "the first failure still owes an attempt");
        assert!(retry.owed_at().is_some(), "the attempt is owed at once");
        retry.dispatched();
        assert!(retry.owed_at().is_none(), "a dispatched attempt settles it");
        assert_eq!(retry.remaining(), MAX_TURN_RETRIES - 1);
    }

    #[test]
    fn a_failure_before_the_turn_waits_and_spends_the_budget() {
        let mut retry = RetryBudget::new();
        assert!(retry.fail_after_backoff());
        let at = retry.owed_at().expect("the owed attempt waits");
        assert!(at > Instant::now(), "it waits out the backoff");
        retry.dispatched();
        assert!(retry.owed_at().is_none());
        assert_eq!(retry.remaining(), MAX_TURN_RETRIES - 1);
    }

    /// The debt survives being read: only a dispatched attempt settles it, so a wait that a status
    /// query interrupts keeps the retry owed.
    #[test]
    fn an_owed_attempt_survives_being_read() {
        let mut retry = RetryBudget::new();
        assert!(retry.fail_after_backoff());
        assert!(retry.owed_at().is_some());
        assert!(retry.owed_at().is_some(), "reading it does not spend it");
        assert_eq!(retry.remaining(), MAX_TURN_RETRIES - 1);
    }

    #[test]
    fn a_failure_before_the_turn_stops_owing_once_the_budget_is_spent() {
        let mut retry = RetryBudget::new();
        assert!(retry.fail_after_backoff());
        assert!(retry.fail_after_backoff());
        assert!(!retry.fail_after_backoff(), "a spent budget owes nothing");
        assert_eq!(retry.remaining(), 0, "the budget never goes below zero");
    }

    #[test]
    fn a_new_reason_leaves_the_budget_whole() {
        let mut retry = RetryBudget::new();
        retry.fail_after_backoff();
        retry.fail_now();
        retry.reopen();
        assert_eq!(retry.remaining(), MAX_TURN_RETRIES);
        assert!(retry.owed_at().is_none());
    }
}
