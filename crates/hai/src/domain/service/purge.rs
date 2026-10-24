//! The purge the agent asks for: one transaction over the agent surface, then the two deletions that
//! cannot share it.

use uuid::Uuid;

use crate::{
    domain::{
        repo::{PurgeWiped, Repos},
        vo::{ChapterWrite, ConversationId, PurgeSurfaces, TimeRange},
    },
    error::Result,
};

#[derive(Debug)]
pub struct PurgeService {
    repos: Repos,
}

impl PurgeService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    /// Wipes one Conversation's agent surface in a single transaction. `chapter` is the snapshot the
    /// conversation row is rewritten to; `also_consumed` names the changes to prune with the ones the
    /// deletes invalidated — the confirmed `Purge` entry this pass runs.
    pub async fn purge(
        &self,
        conversation_id: ConversationId,
        surfaces: PurgeSurfaces,
        range: TimeRange,
        chapter: ChapterWrite,
        also_consumed: &[Uuid],
    ) -> Result<PurgeWiped> {
        self.repos
            .purge
            .wipe(conversation_id, surfaces, range, chapter, also_consumed)
            .await
    }

    /// Deletes the audit trail of every member Chat inside the window.
    pub async fn delete_events(
        &self,
        conversation_id: ConversationId,
        range: TimeRange,
    ) -> Result<u64> {
        self.repos
            .event
            .delete_in_range_for_conversation(conversation_id, range)
            .await
    }

    /// Deletes this Conversation's cached analyses inside the window, whatever subject wrote them.
    pub async fn delete_perceptions(
        &self,
        conversation_id: ConversationId,
        range: TimeRange,
    ) -> Result<u64> {
        self.repos
            .perception
            .delete_in_range_for_conversation(conversation_id, range)
            .await
    }
}
