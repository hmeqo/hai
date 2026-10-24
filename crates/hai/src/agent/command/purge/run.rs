//! The purge pass itself: every member Chat's platform and workspace surfaces are read before the
//! rows that name them go, then one conversation-wide transaction removes the agent surface.

use uuid::Uuid;

use super::PurgeReport;
use crate::{
    agent::{
        channel::DeleteReport, environment::workspace::Workspace, events::AgentEventBus,
        run::turn::ToolContext,
    },
    domain::vo::{ChapterWrite, PurgeSpec},
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
    let conversation_id = tool.conversation_id;
    let services = &tool.services;
    let chats = services
        .platform
        .chats_in_conversation(conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut telegram = DeleteReport::default();
    let mut handles = Vec::new();
    let mut cached_removed = 0;
    for chat in &chats {
        let channel = services
            .platform
            .platform_channel(chat.channel_id)
            .await
            .map_err(|e| e.to_string())?;
        let Some(handle) = tool.router.handle(&channel.channel_key, chat.id).await else {
            return Err(format!("channel {} is not running", channel.channel_key));
        };
        if spec.surfaces.telegram {
            let ids = services
                .message
                .external_ids_in_range(conversation_id, chat.id, range)
                .await
                .map_err(|e| e.to_string())?;
            let report = handle
                .delete_messages(&ids)
                .await
                .map_err(|e| e.to_string())?;
            telegram.deleted += report.deleted;
            telegram.failed.extend(report.failed);
        }
        if spec.surfaces.derived {
            let chat_handles = services
                .message
                .attachment_handles_in_range(conversation_id, chat.id, range)
                .await
                .map_err(|e| e.to_string())?;
            let file_ids: Vec<String> = chat_handles
                .iter()
                .map(|handle| handle.file_id.clone())
                .collect();
            cached_removed += handle
                .forget_cached(&file_ids)
                .await
                .map_err(|e| e.to_string())?;
            handles.extend(chat_handles);
        }
    }

    let chapter = if spec.surfaces.agent {
        services
            .conversation
            .fresh_chapter()
            .map_err(|e| e.to_string())?
    } else {
        match services
            .conversation
            .get(conversation_id)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(record) => ChapterWrite {
                messages: record.messages,
                context_meta: record.context_meta,
            },
            None => services
                .conversation
                .fresh_chapter()
                .map_err(|e| e.to_string())?,
        }
    };

    let mut wiped = services
        .purge
        .purge(conversation_id, spec.surfaces, range, chapter, &[entry_id])
        .await
        .map_err(|e| e.to_string())?;

    if spec.surfaces.derived {
        events.flush().await;
        wiped.counts.events = services
            .purge
            .delete_events(conversation_id, range)
            .await
            .map_err(|e| e.to_string())?;

        wiped.counts.perceptions = services
            .purge
            .delete_perceptions(conversation_id, range)
            .await
            .map_err(|e| e.to_string())?;
    }

    let files_removed = if spec.surfaces.files {
        Workspace::for_conversation(conversation_id)
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
