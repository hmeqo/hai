use std::{fmt, str::FromStr};

use tsclientlib::{ChannelId, ClientId, Invoker, MessageTarget, data, events::Event};
use uuid::Uuid;

use crate::{
    agent::inbound::{Arrival, ArrivalChat, ArrivalSender, ArrivalText},
    domain::{
        model::Platform,
        repo::ChangeKind,
        vo::{ChatAddress, ChatOrigin, EntryAudience, PlatformAccountMeta, TeamspeakAccountMeta},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ChatId {
    Server,
    Room(ChannelId),
    Client(String),
}

impl fmt::Display for ChatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Server => f.write_str("server"),
            Self::Room(id) => write!(f, "channel:{}", id.0),
            Self::Client(uid) => write!(f, "client:{uid}"),
        }
    }
}

impl FromStr for ChatId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "server" => Ok(Self::Server),
            _ => {
                if let Some(id) = value.strip_prefix("channel:") {
                    return id
                        .parse::<u64>()
                        .map(|id| Self::Room(ChannelId(id)))
                        .map_err(|_| invalid(value));
                }
                if let Some(uid) = value.strip_prefix("client:")
                    && !uid.is_empty()
                {
                    return Ok(Self::Client(uid.to_owned()));
                }
                Err(invalid(value))
            }
        }
    }
}

fn invalid(value: &str) -> String {
    format!(
        "invalid TeamSpeak chat id '{value}': expected 'server', 'channel:<id>' or 'client:<uid>'"
    )
}

pub(super) fn classify(text: &str) -> ArrivalText {
    let rest = text.trim_start();
    let Some(rest) = rest.strip_prefix('!') else {
        return ArrivalText::Ordinary;
    };
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty() {
        return ArrivalText::Ordinary;
    }
    ArrivalText::Command {
        name: name.to_owned(),
        payload: rest[name_end..].trim_start().to_owned(),
    }
}

/// Whether the bot was addressed by name in a room or the server chat. A mention is a standalone
/// token: `hai`, `@hai` and `say hai` count, while `haiku` and a CJK continuation of `hai` are other
/// words.
pub(super) fn mentions(text: &str, names: &[&str]) -> bool {
    let text = text.to_lowercase();
    names.iter().any(|name| {
        if name.is_empty() {
            return false;
        }
        let name = name.to_lowercase();
        text.match_indices(&name).any(|(start, _)| {
            let before = text[..start].chars().next_back();
            let after = text[start + name.len()..].chars().next();
            before.is_none_or(|c| !c.is_alphanumeric())
                && after.is_none_or(|c| !c.is_alphanumeric())
        })
    })
}

fn from_other(invoker: &Invoker, own_client: ClientId) -> bool {
    invoker.id != own_client
}

/// One `Event::Message` read into the shape the tail consumes; `None` for a message that cannot be
/// attributed to a stable sender or chat, and for a poke.
pub(super) fn arrival_from_message(
    state: &data::Connection,
    config_name: &str,
    event: Event,
) -> Option<Arrival> {
    let Event::Message {
        target,
        invoker,
        message,
    } = event
    else {
        return None;
    };
    if matches!(target, MessageTarget::Poke(_)) || !from_other(&invoker, state.own_client) {
        return None;
    }
    let Some(uid) = invoker.uid.as_ref() else {
        tracing::debug!(
            client = %invoker.id,
            "TeamSpeak message without a UID; skipping"
        );
        return None;
    };
    let uid = uid.to_string();

    let (chat, kind, title) = match target {
        MessageTarget::Server => (
            ChatId::Server,
            crate::domain::model::ChatType::Group,
            Some(state.server.name.clone()),
        ),
        MessageTarget::Channel => {
            let channel = state
                .clients
                .get(&invoker.id)
                .map(|client| client.channel)
                .filter(|id| id.0 != 0)?;
            let title = state
                .channels
                .get(&channel)
                .map(|channel| channel.name.clone())
                .unwrap_or_else(|| format!("channel {}", channel.0));
            (
                ChatId::Room(channel),
                crate::domain::model::ChatType::Group,
                Some(title),
            )
        }
        MessageTarget::Client(id) if id == state.own_client => (
            ChatId::Client(uid.clone()),
            crate::domain::model::ChatType::Private,
            Some(invoker.name.clone()),
        ),
        MessageTarget::Client(_) | MessageTarget::Poke(_) => return None,
    };

    let mut names = vec![config_name];
    if let Some(own) = state.clients.get(&state.own_client) {
        names.push(&own.name);
    }
    let audience = match kind {
        crate::domain::model::ChatType::Private => EntryAudience::Agent,
        _ if mentions(&message, &names) => EntryAudience::Agent,
        _ => EntryAudience::Bystanders,
    };
    let text_kind = classify(&message);

    Some(Arrival {
        platform: Platform::Teamspeak,
        chat: ArrivalChat {
            address: ChatAddress::new(
                Some(ChatOrigin::new(state.server.public_key.get_uid())),
                chat.to_string(),
            ),
            kind,
            title,
        },
        sender: Some(ArrivalSender {
            external_id: uid,
            meta: Some(
                serde_json::to_value(PlatformAccountMeta::Teamspeak(TeamspeakAccountMeta {
                    nickname: invoker.name.clone(),
                }))
                .expect("a TeamSpeak account meta serializes"),
            ),
        }),
        text: message,
        attachments: Vec::new(),
        external_id: Uuid::now_v7().to_string(),
        reply_external_id: None,
        change: ChangeKind::Created { edited_at: None },
        sent_at: None,
        audience,
        meta: serde_json::Value::Null,
        text_kind,
    })
}

#[cfg(test)]
mod tests {
    use tsclientlib::UidBuf;

    use super::*;

    #[test]
    fn chat_ids_round_trip() {
        for id in ["server", "channel:3", "client:abc="] {
            let parsed: ChatId = id.parse().expect("a valid chat id");
            assert_eq!(parsed.to_string(), id);
        }
        assert_eq!(
            "channel:3".parse::<ChatId>().expect("a room"),
            ChatId::Room(ChannelId(3))
        );
        assert_eq!(
            "client:abc=".parse::<ChatId>().expect("a client"),
            ChatId::Client("abc=".to_owned())
        );
    }

    #[test]
    fn malformed_chat_ids_are_refused() {
        for bad in ["", "channel", "channel:", "channel:x", "client:", "foo"] {
            let err = bad.parse::<ChatId>().unwrap_err();
            assert!(err.contains("invalid TeamSpeak chat id"), "{err}");
        }
    }

    #[test]
    fn commands_keep_their_payload() {
        assert!(matches!(
            classify("!status"),
            ArrivalText::Command { ref name, ref payload }
                if name == "status" && payload.is_empty()
        ));
        assert!(matches!(
            classify("  !purge 7d --yes"),
            ArrivalText::Command { ref name, ref payload }
                if name == "purge" && payload == "7d --yes"
        ));
        assert!(matches!(
            classify("!unknown"),
            ArrivalText::Command { ref name, .. } if name == "unknown"
        ));
    }

    #[test]
    fn ordinary_text_is_not_a_command() {
        for text in ["!", "hello", "[b]!status[/b]", "   ", ""] {
            assert!(matches!(classify(text), ArrivalText::Ordinary), "{text:?}");
        }
    }

    #[test]
    fn a_mention_is_a_standalone_word() {
        for text in ["hai", "@hai", "Hai's", "say hai", "hai, hello"] {
            assert!(mentions(text, &["hai"]), "{text:?} addresses the bot");
        }
        for text in ["haiku", "hai你好"] {
            assert!(!mentions(text, &["hai"]), "{text:?} is another word");
        }
        assert!(!mentions("Hai", &["other"]));
        assert!(mentions("hey Hai, look", &["Hai"]));
        assert!(mentions("the bot is hai", &["hai", "bot"]));
    }

    #[test]
    fn the_bot_does_not_read_its_own_message() {
        let invoker = Invoker {
            name: "hai".to_owned(),
            id: ClientId(7),
            uid: Some(UidBuf(vec![1, 2, 3])),
        };
        assert!(!from_other(&invoker, ClientId(7)));
        assert!(from_other(&invoker, ClientId(8)));
    }
}
