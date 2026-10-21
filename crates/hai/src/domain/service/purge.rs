//! The purge the agent asks for: one transaction over the agent surface, then the two deletions that
//! cannot share it.

use uuid::Uuid;

use crate::{
    domain::{
        repo::{PurgeWiped, Repos},
        vo::{ChapterWrite, ChatId, PurgeSurfaces, Subject, TimeRange},
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

    /// Wipes one chat's agent surface in a single transaction. `chapter` is the snapshot the
    /// conversation row is rewritten to; `also_consumed` names the changes to prune with the ones the
    /// deletes invalidated — the confirmed `Purge` entry this pass runs.
    pub async fn purge(
        &self,
        chat_id: ChatId,
        surfaces: PurgeSurfaces,
        range: TimeRange,
        chapter: ChapterWrite,
        also_consumed: &[Uuid],
    ) -> Result<PurgeWiped> {
        self.repos
            .purge
            .wipe(chat_id, surfaces, range, chapter, also_consumed)
            .await
    }

    /// Deletes this chat's audit trail inside the window. The caller flushes the event bus first: an
    /// event that was already queued when this ran would land after it and survive the purge.
    pub async fn delete_events(&self, chat_id: ChatId, range: TimeRange) -> Result<u64> {
        self.repos.event.delete_in_range(chat_id, range).await
    }

    /// Deletes the transcripts of these attachment subjects — the only scope a perception row has,
    /// it carries no chat.
    pub async fn delete_perceptions(&self, subjects: &[Subject]) -> Result<u64> {
        self.repos.perception.delete_by_subjects(subjects).await
    }
}
