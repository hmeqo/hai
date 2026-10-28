use genai::chat::ChatMessage;
use uuid::Uuid;

use crate::{
    agent::run::{Messages, wire_messages},
    domain::{
        service::ImageStore,
        vo::{
            ContextMeta, ConversationId, ConversationSnapshot, SessionId, StoredMessage,
            StoredPart, StoredRole,
        },
    },
    error::{AppError, Result},
};

/// Conversation state: the provider-neutral message log and the chapter scalars, persisted as
/// `ConversationSnapshot`. An image part holds an `ImageRef`, so history renders without a new read.
pub(crate) struct Conversation {
    conversation_id: ConversationId,
    messages: Vec<StoredMessage>,
    chapter: ContextMeta,
    restored_last_active: Option<jiff::Timestamp>,
}

/// A completed Turn whose snapshot could not be written yet; the next idle tick retries it before
/// anything else.
pub(super) struct PendingWrite {
    pub(super) snapshot: ConversationSnapshot,
    pub(super) consumed: Vec<Uuid>,
}

impl Conversation {
    pub fn new(conversation_id: ConversationId) -> Self {
        Self {
            conversation_id,
            messages: Vec::new(),
            chapter: ContextMeta::new(),
            restored_last_active: None,
        }
    }

    pub fn from_snapshot(
        conversation_id: ConversationId,
        snap: ConversationSnapshot,
        restored_last_active: Option<jiff::Timestamp>,
    ) -> Self {
        Self {
            conversation_id,
            messages: snap.messages,
            chapter: snap.context_meta,
            restored_last_active,
        }
    }

    pub fn snapshot(&self) -> ConversationSnapshot {
        ConversationSnapshot {
            messages: self.messages.clone(),
            context_meta: self.chapter,
        }
    }

    /// The snapshot a completed Turn would persist, without changing this value.
    ///
    /// `messages` is the rendered history followed by the Turn's new messages, one wire message per
    /// stored message; the split below is what makes that invariant load-bearing.
    pub async fn next_snapshot(
        &self,
        messages: Messages,
        step_count: usize,
        last_prompt_tokens: u32,
        images: &ImageStore,
    ) -> Result<ConversationSnapshot> {
        let history_len = self.messages.len();
        let mut messages = messages.into_vec();
        if messages.len() < history_len {
            return Err(AppError::message(format!(
                "conversation {}: completed Turn lost its rendered history",
                self.conversation_id
            )));
        }
        let new_messages = messages.split_off(history_len);
        let new_messages = wire_messages::store_messages(
            Messages::new(new_messages),
            self.conversation_id,
            images,
        )
        .await?;
        let mut stored = self.messages.clone();
        stored.extend(new_messages);
        let mut chapter = self.chapter;
        chapter.advance(step_count, last_prompt_tokens);
        Ok(ConversationSnapshot {
            messages: stored,
            context_meta: chapter,
        })
    }

    pub fn apply_snapshot(&mut self, snap: ConversationSnapshot) {
        self.messages = snap.messages;
        self.chapter = snap.context_meta;
        self.restored_last_active = None;
    }

    /// Rebuilds the wire message list for one model. Images are decoded from the image store only when
    /// the model can consume them.
    pub async fn build_full_messages(
        &self,
        chunk: Vec<ChatMessage>,
        images: &ImageStore,
        image_input: bool,
    ) -> Result<Messages> {
        let mut full = wire_messages::render_messages(
            &self.messages,
            self.conversation_id,
            images,
            image_input,
        )
        .await?;
        full.extend(chunk);
        Ok(full)
    }

    pub async fn messages_for_wrap_up(
        &self,
        images: &ImageStore,
        image_input: bool,
    ) -> Result<Messages> {
        wire_messages::render_messages(&self.messages, self.conversation_id, images, image_input)
            .await
    }

    /// The chapter's initial state: a chapter with no completed Turn is built whole — backfill
    /// included — while one that has run a Turn continues from the context it holds. A chapter
    /// opened by a wrap-up summary holds that message and is still its first render.
    pub fn is_first_render(&self) -> bool {
        self.turn_count() == 0
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn context_tokens(&self) -> u32 {
        self.chapter.tokens
    }

    pub fn step_count(&self) -> u64 {
        self.chapter.step_count
    }

    pub fn turn_count(&self) -> u64 {
        self.chapter.turn_count
    }

    pub fn has_unwrapped_content(&self) -> bool {
        self.chapter.is_non_empty()
    }

    pub fn take_restored_last_active(&mut self) -> Option<jiff::Timestamp> {
        self.restored_last_active.take()
    }

    pub fn session(&self) -> SessionId {
        self.chapter.session
    }

    pub fn start_new_chapter(&mut self, summary: Option<String>) {
        self.messages = summary_messages(summary);
        self.chapter = ContextMeta::new();
    }

    pub fn next_chapter_snapshot(&self, summary: Option<String>) -> ConversationSnapshot {
        ConversationSnapshot {
            messages: summary_messages(summary),
            context_meta: ContextMeta::new(),
        }
    }
}

fn summary_messages(summary: Option<String>) -> Vec<StoredMessage> {
    summary
        .map(|text| StoredMessage {
            role: StoredRole::User,
            parts: vec![StoredPart::Text { text }],
        })
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::vo::{StoredMessage, StoredPart, StoredRole},
        test_support::TempDir,
    };

    const CONVERSATION: ConversationId = ConversationId(9);

    #[tokio::test]
    async fn an_image_in_history_renders_without_a_new_read() {
        let dir = TempDir::create("hai-conversation-image");
        let images = ImageStore::new(dir.path().to_path_buf());
        let reference = images
            .put(CONVERSATION, b"pixels", "image/png")
            .await
            .expect("image put");
        let conversation = Conversation::from_snapshot(
            CONVERSATION,
            ConversationSnapshot {
                messages: vec![StoredMessage {
                    role: StoredRole::User,
                    parts: vec![
                        StoredPart::Text {
                            text: "Image from read".to_owned(),
                        },
                        StoredPart::Image { reference },
                    ],
                }],
                context_meta: ContextMeta::new(),
            },
            None,
        );

        let rendered = conversation
            .build_full_messages(Vec::new(), &images, true)
            .await
            .expect("render history");
        assert!(
            rendered.into_vec()[0]
                .content
                .iter()
                .any(|part| matches!(part, genai::chat::ContentPart::Binary(_)))
        );
    }

    #[tokio::test]
    async fn a_text_only_render_keeps_the_canonical_image_reference() {
        let dir = TempDir::create("hai-conversation-text-only");
        let images = ImageStore::new(dir.path().to_path_buf());
        let reference = images
            .put(CONVERSATION, b"pixels", "image/png")
            .await
            .expect("image put");
        let conversation = Conversation::from_snapshot(
            CONVERSATION,
            ConversationSnapshot {
                messages: vec![StoredMessage {
                    role: StoredRole::User,
                    parts: vec![StoredPart::Image { reference }],
                }],
                context_meta: ContextMeta::new(),
            },
            None,
        );

        let rendered = conversation
            .build_full_messages(vec![ChatMessage::user("next")], &images, false)
            .await
            .expect("render text-only history");
        let snapshot = conversation
            .next_snapshot(rendered, 1, 10, &images)
            .await
            .expect("store next Turn");

        assert!(matches!(
            &snapshot.messages[0].parts[0],
            StoredPart::Image { .. }
        ));
        assert!(matches!(
            &snapshot.messages[1].parts[0],
            StoredPart::Text { text } if text == "next"
        ));
    }

    #[test]
    fn a_chapter_summary_has_no_image() {
        let snapshot =
            Conversation::new(CONVERSATION).next_chapter_snapshot(Some("summary".into()));
        assert!(matches!(
            &snapshot.messages[0].parts[0],
            StoredPart::Text { text } if text == "summary"
        ));
        assert!(snapshot.messages.iter().all(|message| {
            message
                .parts
                .iter()
                .all(|part| !matches!(part, StoredPart::Image { .. }))
        }));
    }
}
