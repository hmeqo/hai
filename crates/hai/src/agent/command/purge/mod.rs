//! `/purge`: the confirmation a person goes through, and what a confirmed purge leaves behind. The
//! dialog is host copy held in memory; a confirmed purge is one entry in the accumulation, which the
//! chat's own agent carries out in a pass of its own.

pub(crate) mod copy;
mod dialog;
pub(crate) mod run;

pub(crate) use dialog::{PurgeAction, PurgeDialog};
pub(crate) use run::run;
use uuid::Uuid;

use crate::{
    agent::{
        channel::{CallbackAnswer, ChannelAccess, ChoiceOption, DeleteReport},
        command::{
            Command, CommandContext, PurgeArg, Reply, Started, SystemCommand,
            dialog::{Audience, DialogPage, PageSite, Pages, Press, settle},
            execute,
        },
        host_copy::HostCopy,
        inbound::Arrival,
    },
    agentcore::Language,
    domain::vo::{ChatEntry, ChatId, EntryAudience, EntryKind, PurgeCounts, PurgeSpec},
    error::Result,
};

/// One purge's outcome: what the receipt is written from and what the audit trail records.
pub(crate) struct PurgeReport {
    pub spec: PurgeSpec,
    pub counts: PurgeCounts,
    pub telegram: DeleteReport,
    pub files_removed: u64,
    pub cached_removed: u64,
    /// The changes the purge took out of the accumulation, the one that asked for it included.
    pub consumed: Vec<Uuid>,
}

/// Puts the first page out. A command that already named a stretch goes straight to the
/// confirmation; a bare `/purge` starts at the menu.
pub(crate) async fn open(
    pages: &Pages,
    access: &dyn ChannelAccess,
    chat_id: ChatId,
    arrival: &Arrival,
    arg: PurgeArg,
    language: Language,
) -> Result<()> {
    let state = match arg.range {
        Some(range) => PurgeDialog::Confirm {
            range,
            surfaces: arg.surfaces,
        },
        None => PurgeDialog::Menu {
            surfaces: arg.surfaces,
        },
    };
    let sender_id = arrival
        .sender
        .as_ref()
        .map(|sender| sender.external_id.as_str());
    let site = PageSite {
        chat_id,
        chat_external_id: &arrival.chat.external_id,
        chat_type: arrival.chat.kind,
        language,
    };
    let options = page(pages, site, sender_id, state);
    access
        .ask(
            site.chat_external_id,
            &copy::stage_text(language, &state),
            &options,
        )
        .await
}

/// One button press: the page it came from, the button it was, and what that means. Everything it
/// changes it rewrites in place.
pub(crate) async fn apply(
    ctx: &CommandContext,
    pages: &Pages,
    access: &dyn ChannelAccess,
    state: PurgeDialog,
    action: PurgeAction,
    press: &Press<'_>,
) -> Result<()> {
    let site = press.site;
    let language = site.language;
    let next = match action {
        PurgeAction::Cancel => {
            return settle(
                access,
                &site,
                press.message_external_id,
                &copy::cancelled(language),
                &[],
                press.query_id,
            )
            .await;
        }
        PurgeAction::Stretch => PurgeDialog::Stretch {
            surfaces: state.surfaces(),
        },
        PurgeAction::Back => PurgeDialog::Menu {
            surfaces: state.surfaces(),
        },
        PurgeAction::Surfaces => PurgeDialog::Surfaces {
            surfaces: state.surfaces(),
        },
        PurgeAction::Toggle(surface) => state.toggled(surface),
        PurgeAction::Take(range) => state.confirm(range),
        PurgeAction::Confirm => {
            return confirm(ctx, access, state, press).await;
        }
    };

    let options = page(pages, site, press.sender_id, next);
    settle(
        access,
        &site,
        press.message_external_id,
        &copy::stage_text(language, &next),
        &options,
        press.query_id,
    )
    .await
}

/// A confirmed purge becomes one change in the accumulation and rings the chat: the wipe itself is
/// the chat's own work, so a restart before it runs reads the change and runs it then. The
/// confirmation page and a complete `--yes` command line submit the same `PurgeSpec` here.
pub(crate) async fn queue(ctx: &CommandContext, spec: PurgeSpec) -> Result<()> {
    let entry = ChatEntry::new(
        EntryAudience::Agent,
        EntryKind::Purge(spec),
        jiff::Timestamp::now(),
    );
    ctx.services
        .conversation
        .append_entry(ctx.chat_id, &entry)
        .await?;
    ctx.agents.get_or_create(ctx.chat_id).await.notify();
    Ok(())
}

/// One button press that confirmed the page: the same spec the command line submits, then the page
/// is settled with the receipt.
async fn confirm(
    ctx: &CommandContext,
    access: &dyn ChannelAccess,
    state: PurgeDialog,
    press: &Press<'_>,
) -> Result<()> {
    let site = press.site;
    let PurgeDialog::Confirm { range, surfaces } = state else {
        // Only the confirmation page carries a stretch to run; a page that is not it has nothing
        // to confirm, which means this press outlived its dialog.
        return access
            .answer_callback(
                press.query_id,
                CallbackAnswer::Toast(&HostCopy::DialogStale.render(site.language)),
            )
            .await;
    };
    let command = Command::System(SystemCommand::Purge(PurgeArg {
        range: Some(range),
        surfaces,
        yes: true,
    }));
    let Reply::Started(Started::Purge(spec)) = execute(ctx, command).await? else {
        return access
            .answer_callback(
                press.query_id,
                CallbackAnswer::Toast(&HostCopy::DialogStale.render(site.language)),
            )
            .await;
    };
    if let Err(e) = queue(ctx, spec).await {
        tracing::warn!(chat_id = %site.chat_id, error = %e, "a confirmed purge could not be queued");
        return access
            .answer_callback(
                press.query_id,
                CallbackAnswer::Alert(&copy::not_queued(site.language)),
            )
            .await;
    }

    settle(
        access,
        &site,
        press.message_external_id,
        &copy::started(site.language, &spec),
        &[],
        press.query_id,
    )
    .await
}

/// One page's buttons, each with a token of its own bound to this chat and this state. A purge acts on
/// the person who asked, so only they may press it.
fn page(
    pages: &Pages,
    site: PageSite<'_>,
    opener: Option<&str>,
    state: PurgeDialog,
) -> Vec<ChoiceOption> {
    let buttons: Vec<(String, DialogPage)> = dialog::options(site.language, &state)
        .into_iter()
        .map(|(label, action)| (label, DialogPage::Purge { state, action }))
        .collect();
    pages.draw(site, Audience::Opener(opener.map(str::to_owned)), &buttons)
}
