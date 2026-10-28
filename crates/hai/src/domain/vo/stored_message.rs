use serde::{Deserialize, Serialize};

use crate::domain::vo::ImageRef;

/// One persisted model-facing message; its parts are provider-neutral and never carry base64.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoredMessage {
    pub role: StoredRole,
    pub parts: Vec<StoredPart>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StoredRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StoredPart {
    Text {
        text: String,
    },
    Image {
        #[serde(alias = "media")]
        reference: ImageRef,
    },
    ToolCall {
        call: StoredToolCall,
    },
    ToolResult {
        call_id: String,
        fn_name: Option<String>,
        content: String,
    },
    Reasoning {
        text: String,
    },
    ThoughtSignature {
        signature: String,
    },
    Custom {
        data: serde_json::Value,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoredToolCall {
    pub call_id: String,
    pub fn_name: String,
    pub arguments: serde_json::Value,
    #[serde(default)]
    pub thought_signatures: Vec<String>,
}
