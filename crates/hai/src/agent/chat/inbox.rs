//! The changes a ConversationAgent has read: the durable accumulation, the entries the dispatch
//! policy has already seen, and the policy change that asks for a wrap-up.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::{
    agent::events::{WakeEvent, WakeEvents, WakeReason},
    domain::{
        model::{ChatRoster, ChatType},
        vo::{ChatEntry, ChatId, EntryAudience, EntryKind, HostAction, MessageId, PurgeSpec},
    },
};

/// The purge a batch carries, with the change that asked for it. A Conversation change, never a
/// reason to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PendingPurge {
    pub(super) entry_id: Uuid,
    pub(super) chat_id: ChatId,
    pub(super) spec: PurgeSpec,
}

/// What one `replace` told the caller: whether the retry budget must reopen, and the audiences of
/// entries the scheduler has not seen before.
pub(super) struct InboxRefresh {
    pub(super) reopened: bool,
    pub(super) audiences: Vec<EntryAudience>,
}

/// The accumulated changes and the dispatch facts derived from them.
pub(super) struct Inbox {
    batch: Vec<ChatEntry>,
    fed: HashSet<Uuid>,
    policy_due: bool,
}

impl Inbox {
    pub(super) fn new(entries: Vec<ChatEntry>) -> Self {
        let policy_due = entries
            .iter()
            .any(|entry| matches!(entry.kind, EntryKind::PolicyChanged));
        Self {
            batch: entries,
            fed: HashSet::new(),
            policy_due,
        }
    }

    /// Replaces the batch and reports what is new. `fed` starts empty, so the first replace after
    /// construction reports every entry; later replaces report only entries appended since.
    pub(super) fn replace(&mut self, entries: Vec<ChatEntry>) -> InboxRefresh {
        let fresh: Vec<&ChatEntry> = entries
            .iter()
            .filter(|entry| !self.fed.contains(&entry.id))
            .collect();
        let audiences = fresh
            .iter()
            .filter(|entry| !matches!(entry.kind, EntryKind::Purge(_) | EntryKind::PolicyChanged))
            .map(|entry| entry.audience)
            .collect();
        let reopened = !fresh.is_empty();
        if entries
            .iter()
            .any(|entry| matches!(entry.kind, EntryKind::PolicyChanged))
        {
            self.policy_due = true;
        }
        self.fed = entries.iter().map(|entry| entry.id).collect();
        self.batch = entries;
        InboxRefresh {
            reopened,
            audiences,
        }
    }

    pub(super) fn entries(&self) -> &[ChatEntry] {
        &self.batch
    }

    pub(super) fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    pub(super) fn consume(&mut self, ids: &[Uuid]) {
        self.batch.retain(|entry| !ids.contains(&entry.id));
    }

    pub(super) fn policy_due(&self) -> bool {
        self.policy_due
    }

    pub(super) fn policy_ids(&self) -> Vec<Uuid> {
        if !self.policy_due {
            return Vec::new();
        }
        self.batch
            .iter()
            .filter(|entry| matches!(entry.kind, EntryKind::PolicyChanged))
            .map(|entry| entry.id)
            .collect()
    }

    pub(super) fn clear_policy(&mut self) {
        self.policy_due = false;
    }

    pub(super) fn pending_purge(&self) -> Option<PendingPurge> {
        pending_purge(&self.batch)
    }

    pub(super) fn has_wake_reason(&self, roster: &[ChatRoster]) -> bool {
        batch_has_wake_reason(&self.batch, roster)
    }

    pub(super) fn origin_for_turn(&self, roster: &[ChatRoster]) -> Option<ChatId> {
        select_origin(&self.batch, roster)
    }

    pub(super) fn host_actions(&self) -> Vec<HostAction> {
        host_actions_in(&self.batch)
    }

    pub(super) fn reasons(&self, chat_id: ChatId, roster: &[ChatRoster]) -> WakeEvents {
        let Some(chat_type) = chat_type(roster, chat_id) else {
            return WakeEvents::new(Vec::new());
        };
        let events = self
            .batch
            .iter()
            .filter(|entry| entry.chat_id == chat_id)
            .filter_map(|entry| WakeReason::of_entry(entry, chat_type).map(WakeEvent::new))
            .collect();
        WakeEvents::new(events)
    }

    pub(super) fn consumed_for_turn(&self, origin: ChatId) -> Vec<Uuid> {
        consumed_entries(&self.batch, origin)
    }

    pub(super) fn message_ids(&self, chat_id: ChatId) -> Vec<MessageId> {
        self.batch
            .iter()
            .filter(|entry| entry.chat_id == chat_id)
            .filter_map(ChatEntry::message_id)
            .collect()
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
fn pending_purge(batch: &[ChatEntry]) -> Option<PendingPurge> {
    batch.iter().find_map(|entry| match &entry.kind {
        EntryKind::Purge(spec) => Some(PendingPurge {
            entry_id: entry.id,
            chat_id: entry.chat_id,
            spec: *spec,
        }),
        EntryKind::MessageCreated { .. }
        | EntryKind::MessageEdited { .. }
        | EntryKind::Command(_)
        | EntryKind::PolicyChanged
        | EntryKind::HostAction(_)
        | EntryKind::Scheduled { .. } => None,
    })
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
fn select_origin(batch: &[ChatEntry], roster: &[ChatRoster]) -> Option<ChatId> {
    let mut groups: HashMap<ChatId, Vec<&ChatEntry>> = HashMap::new();
    for entry in batch {
        if !matches!(entry.kind, EntryKind::Purge(_)) {
            groups.entry(entry.chat_id).or_default().push(entry);
        }
    }
    groups
        .into_iter()
        .filter_map(|(chat_id, entries)| {
            let chat_type = chat_type(roster, chat_id)?;
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

fn batch_has_wake_reason(batch: &[ChatEntry], roster: &[ChatRoster]) -> bool {
    batch.iter().any(|entry| {
        chat_type(roster, entry.chat_id)
            .is_some_and(|chat_type| WakeReason::of_entry(entry, chat_type).is_some())
    })
}

fn chat_type(roster: &[ChatRoster], chat_id: ChatId) -> Option<ChatType> {
    roster
        .iter()
        .find(|chat| chat.id == chat_id)
        .map(|chat| chat.chat_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        model::Platform,
        vo::{
            AgentCommand, ChatId, EntryAudience, EntryKind, HostAction, MessageId, PurgeSpec,
            PurgeSurfaces, Stretch,
        },
    };

    fn roster(chat_id: ChatId, chat_type: ChatType) -> ChatRoster {
        ChatRoster {
            id: chat_id,
            alias: format!("chat-{}", chat_id.0),
            platform: Platform::Telegram,
            chat_type,
            name: None,
        }
    }

    fn entry(kind: EntryKind) -> ChatEntry {
        ChatEntry::new(
            ChatId(1),
            EntryAudience::Agent,
            kind,
            jiff::Timestamp::now(),
        )
    }

    fn entry_at(chat_id: ChatId, audience: EntryAudience, at: i64) -> ChatEntry {
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
    fn a_refresh_reports_only_unseen_entries() {
        let command = entry(EntryKind::Command(AgentCommand::Explain(None)));
        let mut inbox = Inbox::new(vec![command.clone()]);

        let first = inbox.replace(vec![command.clone()]);
        assert!(first.reopened);
        assert_eq!(first.audiences, vec![EntryAudience::Agent]);

        let second = inbox.replace(vec![command]);
        assert!(!second.reopened);
        assert!(second.audiences.is_empty());
    }

    /// A Turn consumes what it read; a purge is consumed by the pass that runs it, not by a Turn
    /// that happened to carry it.
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
            Some(PendingPurge {
                entry_id: purge.id,
                chat_id: purge.chat_id,
                spec
            })
        );
        assert!(pending_purge(&[command]).is_none());
    }

    #[test]
    fn a_policy_change_requests_a_wrap_up() {
        let inbox = Inbox::new(vec![entry(EntryKind::PolicyChanged)]);
        assert!(inbox.policy_due());
        assert_eq!(inbox.policy_ids().len(), 1);
    }

    #[test]
    fn host_actions_from_every_chat_are_read_in_batch_order() {
        let batch = vec![
            host_action(ChatId(2), HostAction::PersonaChanged, 1),
            host_action(ChatId(1), HostAction::LanguageChanged, 2),
        ];

        assert_eq!(
            host_actions_in(&batch),
            vec![HostAction::PersonaChanged, HostAction::LanguageChanged]
        );
    }

    #[test]
    fn the_strongest_reason_selects_the_origin() {
        let batch = vec![
            entry_at(ChatId(1), EntryAudience::Agent, 1),
            entry_at(ChatId(2), EntryAudience::Bystanders, 0),
            entry_at(ChatId(3), EntryAudience::Agent, 3),
        ];
        let roster = [
            roster(ChatId(1), ChatType::Group),
            roster(ChatId(2), ChatType::Private),
            roster(ChatId(3), ChatType::Group),
        ];
        let origin = select_origin(&batch, &roster);
        assert_eq!(origin, Some(ChatId(2)), "a private chat is a direct reason");
    }

    #[test]
    fn a_tie_goes_to_the_oldest_entry_then_the_smaller_chat() {
        let batch = vec![
            entry_at(ChatId(9), EntryAudience::Agent, 5),
            entry_at(ChatId(4), EntryAudience::Agent, 5),
            entry_at(ChatId(7), EntryAudience::Agent, 6),
        ];
        let roster = [
            roster(ChatId(9), ChatType::Group),
            roster(ChatId(4), ChatType::Group),
            roster(ChatId(7), ChatType::Group),
        ];
        let origin = select_origin(&batch, &roster);
        assert_eq!(origin, Some(ChatId(4)));
    }

    /// Context-only entries never make a batch dispatchable; a real wake reason does.
    #[test]
    fn a_host_action_alone_is_not_a_wake_reason() {
        let host = ChatEntry::new(
            ChatId(1),
            EntryAudience::Context,
            EntryKind::HostAction(HostAction::PersonaChanged),
            jiff::Timestamp::now(),
        );
        let roster = [roster(ChatId(1), ChatType::Group)];
        assert!(!batch_has_wake_reason(std::slice::from_ref(&host), &roster));
    }
}
