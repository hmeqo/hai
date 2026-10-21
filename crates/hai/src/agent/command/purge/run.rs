//! The purge pass itself: what one confirmed purge does, and in which order, because the platform
//! handles and the file keys are read out of rows the wipe is about to delete.

use uuid::Uuid;

use super::PurgeReport;
use crate::{
    agent::{
        channel::DeleteReport, environment::workspace::Workspace, events::AgentEventBus,
        run::turn::ToolContext,
    },
    domain::vo::{ChapterWrite, PurgeSpec, Subject},
};

/// Runs one purge to the end. `Err` carries the database's or the filesystem's own words: the wipe's
/// transaction rolled back, while the platform and the workspace — which are not part of it — may
/// have been done in part.
pub(crate) async fn run(
    tool: &ToolContext,
    events: &AgentEventBus,
    spec: PurgeSpec,
    entry_id: Uuid,
) -> Result<PurgeReport, String> {
    let range = spec.range.resolve(jiff::Timestamp::now());
    let chat_id = tool.chat.chat_id();
    let services = &tool.services;

    // The platform ids have to be read before the rows carrying them go, so this surface is first
    // whatever the others do.
    let telegram = if spec.surfaces.telegram {
        let ids = services
            .message
            .external_ids_in_range(chat_id, range)
            .await
            .map_err(|e| e.to_string())?;
        tool.chat
            .delete_messages(&ids)
            .await
            .map_err(|e| e.to_string())?
    } else {
        DeleteReport::default()
    };

    // Likewise the attachment handles: the transcript subjects and the cache keys come from them.
    let handles = if spec.surfaces.derived {
        services
            .message
            .attachment_handles_in_range(chat_id, range)
            .await
            .map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };

    // Purging the agent surface starts a new chapter; any other surface keeps the one the chat is in.
    let chapter = if spec.surfaces.agent {
        services
            .conversation
            .fresh_chapter()
            .map_err(|e| e.to_string())?
    } else {
        match services
            .conversation
            .get(chat_id)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(record) => ChapterWrite {
                messages: record.messages,
                context_meta: record.context_meta,
            },
            // A chat with no record yet is given the chapter it would have started with anyway.
            None => services
                .conversation
                .fresh_chapter()
                .map_err(|e| e.to_string())?,
        }
    };

    let mut wiped = services
        .purge
        .purge(chat_id, spec.surfaces, range, chapter, &[entry_id])
        .await
        .map_err(|e| e.to_string())?;

    let mut cached_removed = 0;
    if spec.surfaces.derived {
        // The event bus is the only other writer of event rows, so emptying it first is what keeps
        // an event that was already queued from landing after the delete.
        events.flush().await;
        wiped.counts.events = services
            .purge
            .delete_events(chat_id, range)
            .await
            .map_err(|e| e.to_string())?;

        let subjects: Vec<Subject> = handles.iter().map(|handle| handle.subject()).collect();
        wiped.counts.perceptions = services
            .purge
            .delete_perceptions(&subjects)
            .await
            .map_err(|e| e.to_string())?;

        let file_ids: Vec<String> = handles
            .iter()
            .map(|handle| handle.file_id.clone())
            .collect();
        cached_removed = tool
            .chat
            .forget_cached(&file_ids)
            .await
            .map_err(|e| e.to_string())?;
    }

    let files_removed = if spec.surfaces.files {
        Workspace::for_chat(chat_id)
            .remove_files_in(range)
            .await
            .map_err(|e| e.to_string())?
    } else {
        0
    };

    Ok(PurgeReport {
        spec,
        counts: wiped.counts,
        telegram,
        files_removed,
        cached_removed,
        consumed: wiped.consumed,
    })
}
