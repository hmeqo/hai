mod attention;
mod conversation;
mod dispatch;
mod event_loop;
mod handle;
mod scheduler;
pub(crate) mod types;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub use handle::ConversationAgentHandle;
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
        SelectedModel, ToolContext,
        engine::{self, AgentEngine},
    },
};
use crate::{
    agent::{
        channel::ChannelRouter,
        command::{
            digest::{self, run::DigestReport},
            purge,
        },
    },
    agentcore::tool::AgentTool,
    domain::{
        model::ChatRoster,
        vo::{
            ChatEntry, ChatId, ConversationId, ConversationSnapshot, EntryKind, ModelAlias,
            PurgeSpec,
        },
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

enum ConversationAgentState {
    Idle,
    Busy {
        result_rx: tokio::sync::oneshot::Receiver<BusySignal>,
        started_at: Instant,
        pass: BusyPass,
    },
}

impl ConversationAgentState {
    fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Idle)
    }

    fn busy(
        result_rx: tokio::sync::oneshot::Receiver<BusySignal>,
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

/// The prompt/dispatch projection of the member Chats: sorted roster, the last Chat this agent used,
/// the origin of the Turn now running, and whether the roster changed after the chapter opened.
/// `ChatRuntime` keeps each Chat's tool surface in `ConversationAgent::chats`.
struct Membership {
    roster: Arc<[ChatRoster]>,
    last: ChatId,
    active: Option<ChatId>,
    dirty: bool,
}

impl Membership {
    fn new(mut roster: Vec<ChatRoster>, dirty: bool) -> Result<Self> {
        roster.sort_by_key(|chat| chat.id);
        let first = roster
            .first()
            .map(|chat| chat.id)
            .ok_or_else(|| AppError::message("Conversation has no Chat"))?;
        Ok(Self {
            last: first,
            roster: roster.into(),
            active: None,
            dirty,
        })
    }

    /// A restored agent cannot know whether membership changed while it was gone, so its first
    /// roster is dirty.
    fn restored(roster: Vec<ChatRoster>) -> Result<Self> {
        Self::new(roster, true)
    }

    /// Replaces the roster when it changed; an unchanged roster leaves the dirty flag as it is.
    fn replace(&mut self, mut roster: Vec<ChatRoster>) -> Result<bool> {
        roster.sort_by_key(|chat| chat.id);
        let first = roster
            .first()
            .map(|chat| chat.id)
            .ok_or_else(|| AppError::message("Conversation has no Chat"))?;
        if roster.as_slice() == self.roster.as_ref() {
            return Ok(false);
        }
        self.roster = roster.into();
        self.dirty = true;
        if !self.contains(self.last) {
            self.last = first;
        }
        if self.active.is_some_and(|chat_id| !self.contains(chat_id)) {
            self.active = None;
        }
        Ok(true)
    }

    fn roster(&self) -> Arc<[ChatRoster]> {
        self.roster.clone()
    }

    fn contains(&self, chat_id: ChatId) -> bool {
        self.roster.iter().any(|chat| chat.id == chat_id)
    }

    fn origin(&self) -> ChatId {
        self.active.unwrap_or(self.last)
    }

    #[cfg(test)]
    fn active(&self) -> Option<ChatId> {
        self.active
    }

    fn dirty(&self) -> bool {
        self.dirty
    }

    fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn activate(&mut self, chat_id: ChatId) {
        assert!(self.contains(chat_id), "the origin is always a member");
        self.active = Some(chat_id);
        self.last = chat_id;
    }

    fn deactivate(&mut self) {
        self.active = None;
    }

    fn remember(&mut self, chat_id: ChatId) {
        assert!(self.contains(chat_id), "the last Chat is always a member");
        self.last = chat_id;
    }
}

pub(super) struct ConversationAgent {
    engine: AgentEngine,
    /// The process-wide stop for this agent.
    shutdown: CancellationToken,
    /// The Conversation this agent owns; every member Chat is a surface under it.
    conversation_id: ConversationId,
    /// One entry per member Chat: its handle, its tool surface and the context the tool layer reads.
    chats: HashMap<ChatId, ChatRuntime>,
    membership: Membership,
    /// Rebuilds a Chat surface when a Chat is attached while this agent is live.
    router: ChannelRouter,
    sandbox: Arc<Sandbox>,
    /// The effective model label the next status query reports.
    model_label: String,
    schedule: EventScheduler,
    conversation: Conversation,
    batch: Vec<ChatEntry>,
    /// Entry ids the dispatch policy has already been told about.
    fed_entries: HashSet<Uuid>,
    state: ConversationAgentState,
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
    /// A stricter content policy waits for the chapter wrap-up before the next Turn.
    force_wrap_up: bool,
    /// The policy changes the current wrap-up consumes when it lands.
    wrap_up_consumed: Vec<Uuid>,
    /// A completed Turn whose snapshot could not be written; applied before the next run.
    pending_persist: Option<(ConversationSnapshot, Vec<Uuid>)>,
}

/// One member Chat's execution surface: its channel handle and the tools built for that Chat.
struct ChatRuntime {
    roster: ChatRoster,
    tool: ToolContext,
    tools: Arc<[Arc<dyn AgentTool>]>,
}

async fn mount_chat(
    runtime: &AgentRuntime,
    conversation_id: ConversationId,
    router: &ChannelRouter,
    sandbox: Arc<Sandbox>,
    row: crate::domain::model::Chat,
) -> Result<ChatRuntime> {
    let channel = runtime
        .services()
        .platform
        .platform_channel(row.channel_id)
        .await?;
    let handle = router
        .handle(&channel.channel_key, row.id)
        .await
        .ok_or_else(|| {
            AppError::message(format!(
                "channel {} is not running; Chat {} cannot mount its tools",
                channel.channel_key, row.id
            ))
        })?;
    let tool = ToolContext {
        chat: handle,
        conversation_id,
        router: router.clone(),
        services: runtime.services().clone(),
        sandbox,
        skill_manager: runtime.skill_manager.clone(),
        multimodal: runtime.multimodal().clone(),
    };
    let tools = engine::mounted_tools(runtime, &tool);
    Ok(ChatRuntime {
        roster: row.roster(),
        tool,
        tools,
    })
}

impl ConversationAgent {
    pub async fn new(
        runtime: AgentRuntime,
        conversation_id: ConversationId,
        router: ChannelRouter,
        shutdown: CancellationToken,
        digest_rx: mpsc::UnboundedReceiver<DigestRequest>,
    ) -> Result<Self> {
        let workspace = Workspace::create(conversation_id).await?;
        let cfg = &runtime.cfg().sandbox;
        let sandbox = Arc::new(if cfg.enabled {
            Sandbox::container(cfg, runtime.skill_manager.roots().to_vec(), workspace)
        } else {
            Sandbox::host(workspace, cfg.timeout_secs)
        });
        let engine = AgentEngine::new(&runtime, shutdown.clone());

        let rows = runtime
            .services()
            .platform
            .chats_in_conversation(conversation_id)
            .await?;
        if rows.is_empty() {
            return Err(AppError::message(format!(
                "Conversation {conversation_id} has no Chat"
            )));
        }

        let mut chats = HashMap::with_capacity(rows.len());
        for row in rows {
            let chat = mount_chat(&runtime, conversation_id, &router, sandbox.clone(), row).await?;
            chats.insert(chat.roster.id, chat);
        }

        let membership =
            Membership::restored(chats.values().map(|chat| chat.roster.clone()).collect())?;

        let record = runtime.services().conversation.get(conversation_id).await?;
        let conversation = match record {
            Some(r) => Conversation::from_snapshot(
                runtime.services().conversation.restore(&r)?,
                Some(r.updated_at),
            ),
            None => Conversation::new(),
        };
        let batch = runtime
            .app
            .services
            .conversation
            .pending_entries(conversation_id)
            .await?;

        let force_wrap_up = batch
            .iter()
            .any(|entry| matches!(entry.kind, EntryKind::PolicyChanged));
        let schedule = EventScheduler::new(&runtime.cfg().agent.attention);
        let stored_model = runtime.app.settings.model(conversation_id).await?;
        let resolved = runtime.resolve_model(
            stored_model
                .as_ref()
                .filter(|alias| runtime.model_exists(alias)),
        );
        let model_label = resolved
            .alias
            .as_ref()
            .map(|alias| format!("{alias} ({})", resolved.model))
            .unwrap_or_else(|| resolved.model.clone());
        Ok(Self {
            engine,
            shutdown,
            conversation_id,
            chats,
            membership,
            router,
            sandbox,
            model_label,
            schedule,
            conversation,
            batch,
            fed_entries: HashSet::new(),
            state: ConversationAgentState::Idle,
            retry: RetryBudget::new(),
            digest_rx,
            digest_reply: None,
            wrap_up_retry_at: None,
            purge_retry_at: None,
            purge_entry: None,
            force_wrap_up,
            wrap_up_consumed: Vec::new(),
            pending_persist: None,
        })
    }

    fn chat(&self, chat_id: ChatId) -> Result<&ChatRuntime> {
        self.chats.get(&chat_id).ok_or_else(|| {
            AppError::message(format!(
                "Chat {chat_id} is not a member of Conversation {}",
                self.conversation_id
            ))
        })
    }

    fn origin(&self) -> ChatId {
        self.membership.origin()
    }

    /// The model this Conversation's next pass runs on. A stored alias the catalog no longer has is
    /// cleared and the configured default is used.
    async fn selected_model(&mut self, stored: Option<ModelAlias>) -> Result<SelectedModel> {
        let settings = &self.engine.runtime.app.settings;
        if let Some(alias) = &stored
            && !self.engine.runtime.model_exists(alias)
        {
            tracing::warn!(
                %self.conversation_id,
                alias = %alias,
                "stored model alias is no longer configured; falling back to the default"
            );
            settings.reset_model(self.conversation_id).await?;
            let fallback = settings.configured_model();
            let model = self.engine.runtime.resolve_model(fallback.as_ref());
            self.model_label = model
                .alias
                .as_ref()
                .map(|alias| format!("{alias} ({})", model.model))
                .unwrap_or_else(|| model.model.clone());
            return Ok(model);
        }
        let model = self.engine.runtime.resolve_model(stored.as_ref());
        self.model_label = model
            .alias
            .as_ref()
            .map(|alias| format!("{alias} ({})", model.model))
            .unwrap_or_else(|| model.model.clone());
        Ok(model)
    }

    /// Writes one snapshot and consumes the batch in one statement; the caller applies the
    /// snapshot to memory only after this succeeds.
    async fn persist_conversation(
        &self,
        snap: &ConversationSnapshot,
        consumed: &[Uuid],
    ) -> Result<()> {
        self.engine
            .runtime
            .app
            .services
            .conversation
            .save(snap, self.conversation_id, consumed)
            .await
            .map_err(|e| {
                tracing::warn!(%self.conversation_id, "Failed to persist conversation: {e}");
                e
            })
    }

    /// Leaves Busy: the idle clock restarts here.
    fn end_run(&mut self) {
        self.schedule.mark_run_end();
        // A digest answers its own reply where it lands; one that never got there drops the sender,
        // which is what tells the page the pass is gone.
        self.digest_reply = None;
        self.membership.deactivate();
        self.state = ConversationAgentState::Idle;
    }

    /// One purge: not a Turn and not a role, a background task of its own that owns the Conversation
    /// while it runs — an arrival rings the doorbell and waits its turn. The change that asked for it is
    /// consumed by the pass itself, failure included, so a broken purge is not read again at once;
    /// nothing else in the batch is spent.
    fn start_purge(&mut self, spec: PurgeSpec, entry_id: Uuid, origin: ChatId) {
        self.purge_entry = Some(entry_id);
        if self.membership.contains(origin) {
            self.membership.deactivate();
            self.membership.remember(origin);
        }
        let chat_id = self.origin();
        let tool = self
            .chat(chat_id)
            .expect("the origin is always a member")
            .tool
            .clone();
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
        self.state = ConversationAgentState::busy(rx, Instant::now(), BusyPass::Purge);
    }

    /// One digest: a pass of its own over a stretch of the target Chat's record, holding the
    /// Conversation Busy while it runs and writing no chat data. The report travels back over the
    /// request's own reply, because there is no row it could be read from.
    async fn start_digest(&mut self, request: DigestRequest) {
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
        let chat = match self.chat(request.chat_id) {
            Ok(chat) => chat,
            Err(e) => {
                let _ = request.reply.send(Err(e.to_string()));
                return;
            }
        };
        // The digest may only read, so it is handed the reads and nothing else: the schema it sees
        // is the surface it may use.
        let tools: Arc<[Arc<dyn AgentTool>]> = chat
            .tools
            .iter()
            .filter(|tool| tool.reads_only())
            .cloned()
            .collect();
        let runtime = self.engine.runtime.clone();
        let handle = chat.tool.chat.clone();
        let shutdown = self.shutdown.clone();
        let (chat_id, chat_type, stretch) =
            (request.chat_id, chat.roster.chat_type, request.stretch);
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
        self.digest_reply = Some(request.reply);
        self.state = ConversationAgentState::busy(rx, Instant::now(), BusyPass::Digest);
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
            tracing::warn!(%self.conversation_id, "Re-dispatching after a failure before the turn started");
        } else {
            tracing::warn!(%self.conversation_id, "Retry budget spent; the batch waits for a new change");
        }
        self.state = ConversationAgentState::Idle;
    }

    /// Mounts Chats attached while this agent was live and drops Chats no longer in the
    /// Conversation; the next Turn re-renders the roster when membership changed.
    async fn sync_membership(&mut self) -> Result<()> {
        let rows = self
            .engine
            .runtime
            .app
            .services
            .platform
            .chats_in_conversation(self.conversation_id)
            .await?;
        if rows.is_empty() {
            return Err(AppError::message(format!(
                "Conversation {} has no Chat",
                self.conversation_id
            )));
        }
        let ids: HashSet<ChatId> = rows.iter().map(|row| row.id).collect();
        for row in &rows {
            if !self.chats.contains_key(&row.id) {
                let chat = mount_chat(
                    &self.engine.runtime,
                    self.conversation_id,
                    &self.router,
                    self.sandbox.clone(),
                    row.clone(),
                )
                .await?;
                self.chats.insert(row.id, chat);
            }
        }
        self.chats.retain(|id, _| ids.contains(id));
        self.membership.replace(
            self.chats
                .values()
                .map(|chat| chat.roster.clone())
                .collect(),
        )?;
        Ok(())
    }

    /// Reads the accumulation and tells the dispatch policy about the arrivals it has not seen yet:
    /// one feed per entry, so a quiet period already running is not re-armed on every tick; a fresh
    /// change also reopens the retry budget.
    async fn refresh_batch(&mut self) -> Result<()> {
        self.sync_membership().await?;
        let entries = self
            .engine
            .runtime
            .app
            .services
            .conversation
            .pending_entries(self.conversation_id)
            .await?;
        if let Some(entry) = entries
            .iter()
            .find(|entry| !self.membership.contains(entry.chat_id))
        {
            return Err(AppError::message(format!(
                "pending entry {} names unknown Chat {}",
                entry.id, entry.chat_id
            )));
        }

        let fresh: Vec<&ChatEntry> = entries
            .iter()
            .filter(|entry| !self.fed_entries.contains(&entry.id))
            .collect();
        if !fresh.is_empty() {
            self.retry.reopen();
        }
        for entry in fresh
            .iter()
            .filter(|entry| !matches!(entry.kind, EntryKind::Purge(_) | EntryKind::PolicyChanged))
        {
            self.schedule.on_event(entry.audience);
        }
        if entries
            .iter()
            .any(|entry| matches!(entry.kind, EntryKind::PolicyChanged))
        {
            self.force_wrap_up = true;
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

/// The changes a Turn consumes: its origin Chat's entries plus any host action, which belongs to the
/// Conversation. A purge and a `PolicyChanged` are spent by the pass that runs them, never by a Turn
/// that happens to carry one.
fn consumed_entries(batch: &[ChatEntry], origin: ChatId) -> Vec<Uuid> {
    batch
        .iter()
        .filter(|entry| entry.chat_id == origin || matches!(entry.kind, EntryKind::HostAction(_)))
        .filter(|entry| !matches!(entry.kind, EntryKind::Purge(_) | EntryKind::PolicyChanged))
        .map(|entry| entry.id)
        .collect()
}

/// The purge this batch carries, if any: a Conversation change, never a reason to read.
fn pending_purge(batch: &[ChatEntry]) -> Option<(Uuid, ChatId, PurgeSpec)> {
    batch.iter().find_map(|entry| match &entry.kind {
        EntryKind::Purge(spec) => Some((entry.id, entry.chat_id, *spec)),
        EntryKind::MessageCreated { .. }
        | EntryKind::MessageEdited { .. }
        | EntryKind::Command(_)
        | EntryKind::PolicyChanged
        | EntryKind::HostAction(_)
        | EntryKind::Scheduled { .. } => None,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ChatEntry, Instant, MAX_TURN_RETRIES, RetryBudget, consumed_entries, pending_purge,
    };
    use crate::domain::vo::{
        AgentCommand, ChatId, EntryAudience, EntryKind, HostAction, PurgeSpec, PurgeSurfaces,
        Stretch,
    };

    fn entry(kind: EntryKind) -> ChatEntry {
        ChatEntry::new(
            ChatId(1),
            EntryAudience::Agent,
            kind,
            jiff::Timestamp::now(),
        )
    }

    /// A Turn consumes what it read; a purge is consumed by the pass that runs it, not by a Turn that
    /// happened to carry it.
    #[test]
    fn a_turn_does_not_consume_a_purge() {
        let command = entry(EntryKind::Command(AgentCommand::Explain(None)));
        let purge = entry(EntryKind::Purge(PurgeSpec {
            range: Stretch::All,
            surfaces: PurgeSurfaces::all(),
        }));

        assert_eq!(
            consumed_entries(&[command.clone(), purge], ChatId(1)),
            vec![command.id]
        );
    }

    /// A host action belongs to the Conversation: a Turn consumes one written through another Chat,
    /// because the next Turn reads it. A purge and a policy change stay for their own passes.
    #[test]
    fn a_turn_consumes_a_host_action_from_another_chat() {
        let command = entry(EntryKind::Command(AgentCommand::Explain(None)));
        let action = ChatEntry::new(
            ChatId(2),
            EntryAudience::Context,
            EntryKind::HostAction(HostAction::PersonaChanged),
            jiff::Timestamp::now(),
        );
        let purge = entry(EntryKind::Purge(PurgeSpec {
            range: Stretch::All,
            surfaces: PurgeSurfaces::all(),
        }));
        let policy = entry(EntryKind::PolicyChanged);

        assert_eq!(
            consumed_entries(&[command.clone(), action.clone(), purge, policy], ChatId(1)),
            vec![command.id, action.id]
        );
    }

    /// A stricter policy is spent by the wrap-up it requests, never by an ordinary Turn.
    #[test]
    fn a_turn_does_not_consume_a_policy_change() {
        let command = entry(EntryKind::Command(AgentCommand::Explain(None)));
        let policy = entry(EntryKind::PolicyChanged);

        assert_eq!(
            consumed_entries(&[command.clone(), policy], ChatId(1)),
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
        let command = entry(EntryKind::Command(AgentCommand::Explain(None)));

        assert_eq!(
            pending_purge(&[purge.clone(), command.clone()]),
            Some((purge.id, purge.chat_id, spec))
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

#[cfg(test)]
mod membership_tests {
    use super::Membership;
    use crate::domain::{
        model::{ChatRoster, ChatType, Platform},
        vo::ChatId,
    };

    fn roster(id: i64) -> ChatRoster {
        ChatRoster {
            id: ChatId(id),
            alias: format!("chat-{id}"),
            platform: Platform::Telegram,
            chat_type: ChatType::Group,
            name: None,
        }
    }

    fn ids(membership: &Membership) -> Vec<ChatId> {
        membership.roster().iter().map(|chat| chat.id).collect()
    }

    #[test]
    fn replacing_membership_keeps_the_invariants() {
        let mut membership = Membership::new(vec![roster(2), roster(1)], true).unwrap();
        assert_eq!(ids(&membership), vec![ChatId(1), ChatId(2)]);
        assert_eq!(membership.origin(), ChatId(1));

        membership.activate(ChatId(2));
        assert_eq!(membership.origin(), ChatId(2));

        assert!(membership.replace(vec![roster(1)]).unwrap());
        assert_eq!(ids(&membership), vec![ChatId(1)]);
        assert_eq!(membership.origin(), ChatId(1));
        assert_eq!(membership.active(), None);
        assert!(membership.dirty());

        membership.clear_dirty();
        assert!(!membership.replace(vec![roster(1)]).unwrap());
        assert!(!membership.dirty());
    }

    #[test]
    fn an_empty_membership_is_rejected() {
        assert!(Membership::new(Vec::new(), true).is_err());
        let mut membership = Membership::new(vec![roster(1)], false).unwrap();
        assert!(membership.replace(Vec::new()).is_err());
        assert_eq!(membership.origin(), ChatId(1));
    }
}
