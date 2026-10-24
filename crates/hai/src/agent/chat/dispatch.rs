use std::collections::{HashMap, HashSet};

use tokio::time::Instant;

use super::{BusyPass, ConversationAgent, ConversationAgentState};
use crate::{
    agent::{
        events::{Doorbell, WakeEvent, WakeEvents, WakeReason},
        prompt::context,
        run::{TurnContext, TurnOutput, engine::TurnRun},
    },
    config::conversation::{ContentPolicy, Language},
    domain::{
        model::Message,
        vo::{
            AgentEvent, AgentEventPayload, BatchComposition, ChatEntry, ChatId, EntryKind,
            HostAction, MessageId, SessionRef, StartedBy, TurnNumber,
        },
    },
    error::Result,
};

impl ConversationAgent {
    /// Starts a turn over the batch as it stands: the batch is the input, so a failure before the
    /// turn starts leaves it untouched for the next attempt.
    pub async fn dispatch(&mut self, doorbell: &Doorbell, started_by: StartedBy) {
        let Some(origin) = self.origin_for_batch() else {
            tracing::debug!(%self.conversation_id, "no origin chat in this batch");
            return;
        };
        self.membership.activate(origin);

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
                tracing::warn!(%self.conversation_id, "settings lookup failed: {e}");
                return self.stand_down();
            }
        };
        let ctx = self.build_turn_context(
            origin,
            self.host_actions(),
            self.batch_reasons(origin),
            settings.language,
            settings.persona.clone(),
            settings.content_policy,
        );
        let model = match self.selected_model(settings.model).await {
            Ok(model) => model,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "model selection failed: {e}");
                return self.stand_down();
            }
        };
        let tools = self
            .chat(origin)
            .expect("the origin came from the chat map")
            .tools
            .clone();

        let messages = match self.gather_batch(origin).await {
            Ok(messages) => messages,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "gather_batch failed: {e}");
                return self.stand_down();
            }
        };

        let prompt_messages = messages.len();
        let is_first = self.conversation.is_first_render();
        let new_message_ids: HashSet<MessageId> = self
            .batch
            .iter()
            .filter(|entry| entry.chat_id == origin)
            .filter_map(|entry| entry.message_id())
            .collect();
        let built = match context::build_prompt(&ctx, &messages, is_first, &new_message_ids, origin)
            .await
        {
            Ok(built) => built,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "build_prompt failed: {e}");
                return self.stand_down();
            }
        };

        let messages = self.conversation.build_full_messages(built.messages);
        let turn_number = TurnNumber::from((self.conversation.turn_count() + 1) as usize);

        // A batch that stands for no reason at all is one the idle loop's own pass reads (a purge):
        // a Turn reads changes, and every change a Turn reads is a reason.
        let Some(reason) = ctx.events.strongest() else {
            tracing::debug!(
                %self.conversation_id,
                "no wake reason in this batch; the idle loop's own pass reads it",
            );
            return;
        };

        let composition = BatchComposition::of(
            &self
                .batch
                .iter()
                .filter(|entry| entry.chat_id == origin)
                .cloned()
                .collect::<Vec<_>>(),
        );
        let session = SessionRef::main(self.conversation.session());

        self.engine.runtime.event_bus().emit(AgentEvent {
            chat_id: origin,
            session: Some(session),
            payload: AgentEventPayload::TurnStarted {
                turn: turn_number,
                reason,
                prompt_messages,
                full_prompt: built.rendered_prompt.clone(),
                started_by: Some(started_by),
                composition: Some(composition),
            },
        });

        let result_rx = self.engine.spawn_turn(
            ctx,
            tools,
            TurnRun {
                messages,
                doorbell: doorbell.clone(),
                steering_since: doorbell.version(origin),
                turn_number,
                session,
                model: &model,
            },
        );

        self.state = ConversationAgentState::busy(result_rx, Instant::now(), BusyPass::Turn);
    }

    /// One origin per Turn: the Chat whose ready entries carry the strongest reason. Ties go to the
    /// oldest entry, then to the smaller Chat id, so the order is stable.
    fn origin_for_batch(&self) -> Option<ChatId> {
        select_origin(&self.batch, |chat_id| {
            self.chats.get(&chat_id).map(|chat| chat.roster.chat_type)
        })
    }

    /// Commits the turn: the changes it consumed leave the accumulation in the same step as the
    /// snapshot, so a turn that never reaches here leaves both untouched.
    pub async fn on_complete(&mut self, output: TurnOutput) {
        if output.has_spoken {
            self.schedule.on_spoke();
        }

        let last_prompt_tokens = output.steps.last().map(|t| t.prompt_tokens).unwrap_or(0);
        let origin = self.origin();
        let consumed = super::consumed_entries(&self.batch, origin);
        let snapshot = self.conversation.next_snapshot(
            output.messages,
            output.steps.len(),
            last_prompt_tokens,
        );
        if let Err(e) = self.persist_conversation(&snapshot, &consumed).await {
            tracing::warn!(%self.conversation_id, "completed Turn not persisted: {e}");
            self.pending_persist = Some((snapshot, consumed));
            self.stand_down();
            self.end_run();
            return;
        }

        self.conversation.apply_snapshot(snapshot);
        self.batch.retain(|entry| !consumed.contains(&entry.id));
        self.membership.clear_dirty();
        self.retry.reopen();
        self.end_run();
    }

    fn batch_reasons(&self, chat_id: ChatId) -> WakeEvents {
        let chat_type = self.chats.get(&chat_id).map(|chat| chat.roster.chat_type);
        WakeEvents::new(
            self.batch
                .iter()
                .filter(|entry| entry.chat_id == chat_id)
                .filter_map(|entry| WakeReason::of_entry(entry, chat_type?).map(WakeEvent::new))
                .collect(),
        )
    }

    /// The host actions the Conversation accumulated: context for this Turn, read and then spent
    /// like any other change it read.
    fn host_actions(&self) -> Vec<HostAction> {
        host_actions_in(&self.batch)
    }

    pub fn build_turn_context(
        &self,
        chat_id: ChatId,
        host_actions: Vec<HostAction>,
        events: WakeEvents,
        language: Language,
        persona: Option<String>,
        content_policy: ContentPolicy,
    ) -> TurnContext {
        let chat = self
            .chats
            .get(&chat_id)
            .expect("a turn context is built for a member chat");
        TurnContext {
            tool: chat.tool.clone(),
            app: self.engine.runtime.app.clone(),
            chat_type: chat.roster.chat_type,
            origin_chat: chat_id,
            chats: self.membership.roster(),
            roster_dirty: self.membership.dirty(),
            language,
            persona,
            content_policy,
            host_actions,
            events,
        }
    }

    /// The language the Conversation reads its persona and replies in.
    pub(crate) async fn language(&self) -> Result<Language> {
        self.engine
            .runtime
            .app
            .settings
            .language(self.conversation_id)
            .await
    }

    pub async fn gather_batch(&self, chat_id: ChatId) -> Result<Vec<Message>> {
        let services = self.engine.runtime.services();

        let mut ids: Vec<MessageId> = self
            .batch
            .iter()
            .filter(|entry| entry.chat_id == chat_id)
            .filter_map(|entry| entry.message_id())
            .collect();
        ids.sort_unstable();
        ids.dedup();

        let mut messages = services.message.get_messages_by_ids(&ids).await?;

        if self.conversation.is_first_render() {
            let cap = self.engine.runtime.cfg().agent.context.context_seed_cap;
            messages.extend(
                services
                    .message
                    .recent_for_conversation(self.conversation_id, cap)
                    .await?,
            );
            messages.sort_by_key(|message| message.id);
            messages.dedup_by_key(|message| message.id);
        }

        Ok(messages)
    }
}

/// Every host action in the batch, in arrival order. A setting change belongs to the Conversation,
/// not to the Chat whose command made it.
fn host_actions_in(batch: &[ChatEntry]) -> Vec<HostAction> {
    batch
        .iter()
        .filter_map(ChatEntry::host_action)
        .copied()
        .collect()
}

/// The Chat whose ready changes carry the strongest reason; ties go to the oldest entry, then to
/// the smaller Chat id, so the order does not depend on map iteration.
fn select_origin(
    batch: &[ChatEntry],
    chat_type: impl Fn(ChatId) -> Option<crate::domain::model::ChatType>,
) -> Option<ChatId> {
    let mut groups: HashMap<ChatId, Vec<&ChatEntry>> = HashMap::new();
    for entry in batch {
        if !matches!(entry.kind, EntryKind::Purge(_)) {
            groups.entry(entry.chat_id).or_default().push(entry);
        }
    }
    groups
        .into_iter()
        .filter_map(|(chat_id, entries)| {
            let chat_type = chat_type(chat_id)?;
            let events = WakeEvents::new(
                entries
                    .iter()
                    .filter_map(|entry| WakeReason::of_entry(entry, chat_type).map(WakeEvent::new))
                    .collect(),
            );
            let strongest = events.strongest()?;
            let oldest = entries.iter().map(|entry| entry.at).min()?;
            Some((chat_id, strongest.strength(), oldest))
        })
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| b.0.cmp(&a.0))
        })
        .map(|(chat_id, _, _)| chat_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        model::ChatType,
        vo::{EntryAudience, HostAction, MessageId},
    };

    fn entry(chat_id: ChatId, audience: EntryAudience, at: i64) -> ChatEntry {
        ChatEntry::new(
            chat_id,
            audience,
            EntryKind::MessageCreated {
                message_id: MessageId(at),
            },
            jiff::Timestamp::from_second(at).expect("timestamp"),
        )
    }

    fn host_action(chat_id: ChatId, action: HostAction, at: i64) -> ChatEntry {
        ChatEntry::new(
            chat_id,
            EntryAudience::Context,
            EntryKind::HostAction(action),
            jiff::Timestamp::from_second(at).expect("timestamp"),
        )
    }

    #[test]
    fn host_actions_from_every_chat_are_read_in_batch_order() {
        let batch = vec![
            host_action(ChatId(2), HostAction::PersonaChanged, 1),
            host_action(ChatId(1), HostAction::ModelChanged, 2),
        ];

        assert_eq!(
            host_actions_in(&batch),
            vec![HostAction::PersonaChanged, HostAction::ModelChanged]
        );
    }

    #[test]
    fn the_strongest_reason_selects_the_origin() {
        let batch = vec![
            entry(ChatId(1), EntryAudience::Agent, 1),
            entry(ChatId(2), EntryAudience::Bystanders, 0),
            entry(ChatId(3), EntryAudience::Agent, 3),
        ];
        let origin = select_origin(&batch, |id| {
            Some(if id == ChatId(2) {
                ChatType::Private
            } else {
                ChatType::Group
            })
        });
        assert_eq!(origin, Some(ChatId(2)), "a private chat is a direct reason");
    }

    #[test]
    fn a_tie_goes_to_the_oldest_entry_then_the_smaller_chat() {
        let batch = vec![
            entry(ChatId(9), EntryAudience::Agent, 5),
            entry(ChatId(4), EntryAudience::Agent, 5),
            entry(ChatId(7), EntryAudience::Agent, 6),
        ];
        let origin = select_origin(&batch, |_| Some(ChatType::Group));
        assert_eq!(origin, Some(ChatId(4)));
    }
}
