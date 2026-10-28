//! Conversion between the persisted provider-neutral message log and genai's wire messages. Images
//! live only as references on disk; this module is the one place that encodes them for a provider
//! and the one place that stores their bytes.

use base64::{Engine as _, prelude::BASE64_STANDARD};
use genai::chat::{
    BinarySource, ChatMessage, ChatRole, ContentPart, CustomPart, MessageContent, ToolCall,
    ToolResponse,
};

use super::Messages;
use crate::{
    domain::{
        service::ImageStore,
        vo::{ConversationId, ImageRef, StoredMessage, StoredPart, StoredRole, StoredToolCall},
    },
    error::{AppError, Result},
};

pub(crate) async fn store_messages(
    messages: Messages,
    conversation_id: ConversationId,
    images: &ImageStore,
) -> Result<Vec<StoredMessage>> {
    let mut stored = Vec::with_capacity(messages.len());
    for message in messages.into_vec() {
        stored.push(store_message(message, conversation_id, images).await?);
    }
    Ok(stored)
}

async fn store_message(
    message: ChatMessage,
    conversation_id: ConversationId,
    images: &ImageStore,
) -> Result<StoredMessage> {
    let role = match message.role {
        ChatRole::System => StoredRole::System,
        ChatRole::User => StoredRole::User,
        ChatRole::Assistant => StoredRole::Assistant,
        ChatRole::Tool => StoredRole::Tool,
    };
    let mut parts = Vec::new();
    for part in message.content {
        parts.push(store_part(part, conversation_id, images).await?);
    }
    Ok(StoredMessage { role, parts })
}

async fn store_part(
    part: ContentPart,
    conversation_id: ConversationId,
    images: &ImageStore,
) -> Result<StoredPart> {
    match part {
        ContentPart::Text(text) => Ok(StoredPart::Text { text }),
        ContentPart::Binary(binary) => match binary.source {
            BinarySource::Base64(data) => {
                let bytes = BASE64_STANDARD.decode(data.as_bytes()).map_err(|error| {
                    AppError::message(format!("image base64 unreadable: {error}"))
                })?;
                let reference = images
                    .put(conversation_id, &bytes, &binary.content_type)
                    .await?;
                Ok(StoredPart::Image { reference })
            }
            BinarySource::Url(_) => Err(AppError::message("binary URL has no bytes to store")),
        },
        ContentPart::ToolCall(call) => Ok(StoredPart::ToolCall {
            call: StoredToolCall {
                call_id: call.call_id,
                fn_name: call.fn_name,
                arguments: call.fn_arguments,
                thought_signatures: call.thought_signatures.unwrap_or_default(),
            },
        }),
        ContentPart::ToolResponse(response) => Ok(StoredPart::ToolResult {
            call_id: response.call_id,
            fn_name: response.fn_name,
            content: response.content,
        }),
        ContentPart::ThoughtSignature(signature) => Ok(StoredPart::ThoughtSignature { signature }),
        ContentPart::ReasoningContent(text) => Ok(StoredPart::Reasoning { text }),
        ContentPart::Custom(custom) => Ok(StoredPart::Custom { data: custom.data }),
    }
}

pub(crate) async fn render_messages(
    messages: &[StoredMessage],
    conversation_id: ConversationId,
    images: &ImageStore,
    image_input: bool,
) -> Result<Messages> {
    let mut rendered = Vec::with_capacity(messages.len());
    for message in messages {
        rendered.push(render_message(message, conversation_id, images, image_input).await?);
    }
    Ok(Messages::new(rendered))
}

async fn render_message(
    message: &StoredMessage,
    conversation_id: ConversationId,
    images: &ImageStore,
    image_input: bool,
) -> Result<ChatMessage> {
    let mut parts = Vec::with_capacity(message.parts.len());
    for part in &message.parts {
        parts.push(render_part(part, conversation_id, images, image_input).await?);
    }
    Ok(ChatMessage::new(
        role(message.role),
        MessageContent::from_parts(parts),
    ))
}

async fn render_part(
    part: &StoredPart,
    conversation_id: ConversationId,
    images: &ImageStore,
    image_input: bool,
) -> Result<ContentPart> {
    match part {
        StoredPart::Text { text } => Ok(ContentPart::Text(text.clone())),
        StoredPart::Image { reference } => {
            render_image(reference, conversation_id, images, image_input).await
        }
        StoredPart::ToolCall { call } => Ok(ContentPart::ToolCall(tool_call_to_wire(call))),
        StoredPart::ToolResult {
            call_id,
            fn_name,
            content,
        } => Ok(ContentPart::ToolResponse(ToolResponse {
            call_id: call_id.clone(),
            fn_name: fn_name.clone(),
            content: content.clone(),
        })),
        StoredPart::Reasoning { text } => Ok(ContentPart::ReasoningContent(text.clone())),
        StoredPart::ThoughtSignature { signature } => {
            Ok(ContentPart::ThoughtSignature(signature.clone()))
        }
        StoredPart::Custom { data } => Ok(ContentPart::Custom(CustomPart {
            model_iden: None,
            data: data.clone(),
        })),
    }
}

async fn render_image(
    reference: &ImageRef,
    conversation_id: ConversationId,
    images: &ImageStore,
    image_input: bool,
) -> Result<ContentPart> {
    if !image_input {
        return Ok(ContentPart::Text(
            "[image omitted: model does not support vision]".to_owned(),
        ));
    }
    match images.get(conversation_id, reference).await? {
        Some(bytes) => Ok(ContentPart::from_binary_base64(
            reference.mime.clone(),
            BASE64_STANDARD.encode(bytes),
            None,
        )),
        None => {
            tracing::warn!(
                conversation = %conversation_id,
                image = %reference.id.0,
                "image blob missing"
            );
            Ok(ContentPart::Text(format!(
                "[image unavailable: {}]",
                reference.id.0
            )))
        }
    }
}

fn role(role: StoredRole) -> ChatRole {
    match role {
        StoredRole::System => ChatRole::System,
        StoredRole::User => ChatRole::User,
        StoredRole::Assistant => ChatRole::Assistant,
        StoredRole::Tool => ChatRole::Tool,
    }
}

fn tool_call_to_wire(call: &StoredToolCall) -> ToolCall {
    ToolCall {
        call_id: call.call_id.clone(),
        fn_name: call.fn_name.clone(),
        fn_arguments: call.arguments.clone(),
        thought_signatures: (!call.thought_signatures.is_empty())
            .then(|| call.thought_signatures.clone()),
    }
}

#[cfg(test)]
mod tests {
    use genai::chat::{ContentPart, MessageContent};

    use super::*;
    use crate::{
        domain::{service::ImageStore, vo::ImageId},
        test_support::TempDir,
    };

    const CONVERSATION: ConversationId = ConversationId(7);
    const BYTES: &[u8] = b"pixels";

    fn image_message() -> ChatMessage {
        ChatMessage::user(MessageContent::from_parts(vec![
            ContentPart::from_text("Image from read"),
            ContentPart::from_binary_base64("image/png", BASE64_STANDARD.encode(BYTES), None),
        ]))
    }

    #[tokio::test]
    async fn an_image_is_stored_as_a_reference_and_rendered_back() {
        let dir = TempDir::create("hai-wire-image");
        let store = ImageStore::new(dir.path().to_path_buf());
        let messages = Messages::new(vec![image_message()]);

        let stored = store_messages(messages, CONVERSATION, &store)
            .await
            .expect("store image");
        let StoredPart::Image { reference } = &stored[0].parts[1] else {
            panic!("expected an image part");
        };
        assert_eq!(
            store.get(CONVERSATION, reference).await.expect("image get"),
            Some(BYTES.to_vec())
        );

        let rendered = render_messages(&stored, CONVERSATION, &store, true)
            .await
            .expect("render image");
        let parts: Vec<ContentPart> = rendered.into_vec().remove(0).content.into_iter().collect();
        assert!(matches!(parts[1], ContentPart::Binary(_)));

        let text_only = render_messages(&stored, CONVERSATION, &store, false)
            .await
            .expect("render text");
        assert!(
            text_only.into_vec()[0].content.iter().any(
                |part| matches!(part, ContentPart::Text(text) if text.contains("image omitted"))
            )
        );
    }

    #[tokio::test]
    async fn a_missing_blob_becomes_text_not_an_error() {
        let dir = TempDir::create("hai-wire-missing");
        let store = ImageStore::new(dir.path().to_path_buf());
        let stored = vec![StoredMessage {
            role: StoredRole::User,
            parts: vec![StoredPart::Image {
                reference: ImageRef {
                    id: ImageId("0".repeat(64)),
                    mime: "image/png".to_owned(),
                },
            }],
        }];

        let rendered = render_messages(&stored, CONVERSATION, &store, true)
            .await
            .expect("missing image falls back");
        assert!(rendered.into_vec()[0].content.iter().any(
            |part| matches!(part, ContentPart::Text(text) if text.contains("image unavailable"))
        ));
    }
}
