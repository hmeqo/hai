use serde::{Deserialize, Serialize};

/// Content-addressed identity of one stored image: the lowercase SHA-256 hex of its bytes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ImageId(pub String);

/// One image in the Conversation's image store: its content id and MIME type.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ImageRef {
    pub id: ImageId,
    pub mime: String,
}
