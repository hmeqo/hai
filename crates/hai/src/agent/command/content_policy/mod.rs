//! `/content-policy`: fixed presets; the page collects one and the handler applies it.

pub(crate) mod copy;
mod dialog;

pub(crate) use dialog::ContentPolicyAction;

use crate::{
    agent::{
        channel::ChannelChoices,
        command::{
            Command, CommandContext, ContentPolicyArg, Reply, SystemCommand,
            dialog::{Audience, PageSite, Pages, Press, settle},
            execute,
        },
        inbound::Arrival,
    },
    config::conversation::{ContentPolicy, Language},
    domain::vo::ChatId,
    error::{AppError, Result},
};

/// Puts the picker out: the effective axes and every fixed preset beside them.
pub(crate) async fn open(
    pages: &Pages,
    choices: &dyn ChannelChoices,
    chat_id: ChatId,
    arrival: &Arrival,
    current: ContentPolicy,
    language: Language,
) -> Result<()> {
    let site = PageSite {
        chat_id,
        chat: &arrival.chat.address,
        chat_type: arrival.chat.kind,
        language,
    };
    let opener = arrival
        .sender
        .as_ref()
        .map(|sender| sender.external_id.as_str());
    let options = pages.draw(
        site,
        Audience::Opener(opener.map(str::to_owned)),
        &dialog::options(language),
    );
    choices
        .ask(site.chat, &copy::page(language, current), &options)
        .await
}

/// One button press: the command writes the preset through the same handler the command line
/// reaches, then the page turns into its confirmation.
pub(crate) async fn apply(
    ctx: &CommandContext,
    choices: &dyn ChannelChoices,
    action: ContentPolicyAction,
    press: &Press<'_>,
) -> Result<()> {
    let arg = match action {
        ContentPolicyAction::Set(preset) => ContentPolicyArg::Set(preset),
        ContentPolicyAction::Reset => ContentPolicyArg::Reset,
    };
    let command = Command::System(SystemCommand::ContentPolicy(arg));
    let outcome = execute(ctx, command).await?;
    let text = match outcome.reply {
        Reply::Said(said) => said.copy.render(said.language),
        _ => {
            return Err(AppError::message(
                "a content-policy page did not produce a spoken reply",
            ));
        }
    };
    settle(
        choices,
        &press.site,
        press.message_external_id,
        &text,
        &[],
        press.query_id,
    )
    .await
}
