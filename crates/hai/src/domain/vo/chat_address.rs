use std::fmt;

use serde::{Deserialize, Serialize};

/// The remote authority a chat id is local to: a TeamSpeak virtual server, a Matrix homeserver, or
/// another federated platform's server. A centralized platform has none; its external id is already
/// global within the provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatOrigin(String);

impl ChatOrigin {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChatOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl sqlx::Type<sqlx::Postgres> for ChatOrigin {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <String as sqlx::Type<sqlx::Postgres>>::type_info()
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        <String as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for ChatOrigin {
    fn decode(
        value: sqlx::postgres::PgValueRef<'r>,
    ) -> std::result::Result<Self, sqlx::error::BoxDynError> {
        <String as sqlx::Decode<'r, sqlx::Postgres>>::decode(value).map(Self)
    }
}

impl sqlx::Encode<'_, sqlx::Postgres> for ChatOrigin {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> std::result::Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <String as sqlx::Encode<'_, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
    }
}

/// The address of one chat inside a platform: the remote origin when the platform's own ids are
/// server-local, plus the platform's own id. The HAI channel that owns the chat is separate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatAddress {
    pub origin: Option<ChatOrigin>,
    pub external_id: String,
}

impl ChatAddress {
    pub fn new(origin: Option<ChatOrigin>, external_id: impl Into<String>) -> Self {
        Self {
            origin,
            external_id: external_id.into(),
        }
    }
}

impl fmt::Display for ChatAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.origin {
            Some(origin) => write!(f, "{origin}/{}", self.external_id),
            None => f.write_str(&self.external_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_central_provider_address_has_no_origin() {
        let address = ChatAddress::new(None, "123");
        assert_eq!(address.to_string(), "123");
    }

    #[test]
    fn a_federated_address_names_its_server() {
        let address = ChatAddress::new(Some(ChatOrigin::new("server-uid")), "channel:3");
        assert_eq!(address.to_string(), "server-uid/channel:3");
        assert_eq!(address.external_id, "channel:3");
    }
}
