use std::{collections::HashMap, path::PathBuf, str::FromStr};

use serde::Deserialize;

use super::inbound::ChatId;
use crate::{
    config::Paths,
    error::{AppError, Result, bail},
};

#[derive(Clone, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct TeamspeakConfig {
    pub address: String,
    pub name: String,
    pub identity: String,
    pub channel: String,
    pub channel_id: u64,
    pub server_password: String,
    pub channel_password: String,
    pub allowed_chat_ids: Option<Vec<String>>,
}

impl TeamspeakConfig {
    /// Reads this platform's keys out of one `[channel.<name>]` section; an unknown key fails here, at
    /// startup, rather than at the first connection attempt.
    pub(super) fn parse(key: &str, settings: &HashMap<String, serde_json::Value>) -> Result<Self> {
        if key.is_empty() || key.contains(':') {
            bail!(AppError::message(format!(
                "channel section '{key}' must not be empty or contain ':'"
            )));
        }
        let value = serde_json::Value::Object(settings.clone().into_iter().collect());
        let cfg: Self = serde_json::from_value(value)
            .map_err(|e| AppError::message(format!("channel '{key}': {e}")))?;
        if cfg.address.trim().is_empty() {
            bail!(AppError::message(format!(
                "channel '{key}': address is required"
            )));
        }
        if cfg.name.trim().is_empty() {
            bail!(AppError::message(format!(
                "channel '{key}': name is required"
            )));
        }
        if let Some(entries) = &cfg.allowed_chat_ids {
            if entries.is_empty() {
                tracing::warn!(
                    channel = key,
                    "allowed-chat-ids is empty; every chat is rejected"
                );
            }
            for entry in entries {
                ChatId::from_str(entry).map_err(|reason| {
                    AppError::message(format!(
                        "channel '{key}': invalid allowed-chat-ids entry '{entry}': {reason}"
                    ))
                })?;
            }
        }
        Ok(cfg)
    }

    /// The identity file this channel uses: the configured path, or one file per section so two
    /// TeamSpeak channels never share a client identity.
    pub(super) fn identity_path(&self, key: &str) -> Result<PathBuf> {
        if !self.identity.is_empty() {
            return Ok(PathBuf::from(&self.identity));
        }
        let name = sanitize_filename::sanitize(key);
        if name.is_empty() {
            bail!(AppError::message(format!(
                "channel '{key}': identity file name is empty"
            )));
        }
        Ok(Paths::inferred()
            .data_dir()
            .join("teamspeak")
            .join(format!("{name}.json")))
    }
}

impl Default for TeamspeakConfig {
    fn default() -> Self {
        Self {
            address: String::new(),
            name: "hai".to_owned(),
            identity: String::new(),
            channel: String::new(),
            channel_id: 0,
            server_password: String::new(),
            channel_password: String::new(),
            allowed_chat_ids: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn section(entries: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn a_section_name_that_cannot_be_stored_is_refused() {
        let settings = section(&[("address", json!("127.0.0.1:9987"))]);
        for bad in ["", "a:b", "main:"] {
            assert!(
                TeamspeakConfig::parse(bad, &settings).is_err(),
                "{bad:?} must not pass"
            );
        }
    }

    #[test]
    fn this_platforms_own_keys_are_read() {
        let cfg = TeamspeakConfig::parse(
            "ts3",
            &section(&[
                ("address", json!("127.0.0.1:9987")),
                ("name", json!("Hai")),
                ("channel-id", json!(3)),
                (
                    "allowed-chat-ids",
                    json!(["channel:3", "server", "client:abc="]),
                ),
            ]),
        )
        .expect("a readable section");

        assert_eq!(cfg.address, "127.0.0.1:9987");
        assert_eq!(cfg.name, "Hai");
        assert_eq!(cfg.channel_id, 3);
        assert_eq!(
            cfg.allowed_chat_ids,
            Some(vec![
                "channel:3".to_owned(),
                "server".to_owned(),
                "client:abc=".to_owned()
            ])
        );
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let err = TeamspeakConfig::parse(
            "ts3",
            &section(&[
                ("address", json!("127.0.0.1:9987")),
                ("bot-token", json!("a-token")),
            ]),
        )
        .err()
        .expect("a failing section")
        .to_string();
        assert!(err.contains("unknown field"), "{err}");
    }

    #[test]
    fn a_missing_address_fails_startup() {
        let err = TeamspeakConfig::parse("ts3", &section(&[("name", json!("hai"))]))
            .err()
            .expect("a failing section")
            .to_string();
        assert!(err.contains("address is required"), "{err}");
    }

    #[test]
    fn a_blank_name_fails_startup() {
        let err = TeamspeakConfig::parse(
            "ts3",
            &section(&[("address", json!("127.0.0.1:9987")), ("name", json!("   "))]),
        )
        .err()
        .expect("a failing section")
        .to_string();
        assert!(err.contains("name is required"), "{err}");
    }

    #[test]
    fn a_malformed_allowed_chat_id_fails_startup() {
        for bad in ["channel:x", "client:", "foo"] {
            let err = TeamspeakConfig::parse(
                "ts3",
                &section(&[
                    ("address", json!("127.0.0.1:9987")),
                    ("allowed-chat-ids", json!([bad])),
                ]),
            )
            .err()
            .expect("a failing section")
            .to_string();
            assert!(err.contains(bad), "{err}");
        }
    }

    #[test]
    fn an_empty_allowlist_parses_as_present_and_empty() {
        let cfg = TeamspeakConfig::parse(
            "ts3",
            &section(&[
                ("address", json!("127.0.0.1:9987")),
                ("allowed-chat-ids", json!([])),
            ]),
        )
        .expect("an empty list is a valid setting");
        assert_eq!(cfg.allowed_chat_ids, Some(Vec::new()));
    }

    #[test]
    fn the_identity_default_is_one_file_per_section() {
        let cfg = TeamspeakConfig::parse("ts3", &section(&[("address", json!("127.0.0.1:9987"))]))
            .expect("a readable section");
        let path = cfg.identity_path("ts3").expect("a path");
        assert!(path.ends_with("teamspeak/ts3.json"), "{path:?}");
        assert_eq!(cfg.name, "hai");
    }

    #[test]
    fn a_configured_identity_is_used_as_written() {
        let cfg = TeamspeakConfig::parse(
            "ts3",
            &section(&[
                ("address", json!("127.0.0.1:9987")),
                ("identity", json!("/tmp/hai-ts3.json")),
            ]),
        )
        .expect("a readable section");
        assert_eq!(
            cfg.identity_path("ts3").expect("a path"),
            PathBuf::from("/tmp/hai-ts3.json")
        );
    }
}
