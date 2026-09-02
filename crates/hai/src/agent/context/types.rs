use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    agent::context::render_context::ContentRenderer,
    domain::{
        model::{Message, Perception},
        service::memory::RelatedMemory,
        vo::TopicSearchResult,
    },
};

pub struct AttachmentPerceptionMap {
    pub by_attachment_id: HashMap<Uuid, Vec<Perception>>,
    pub same_resource_as: HashMap<Uuid, Uuid>,
}

pub struct Attachment {
    pub id: Uuid,
    pub file_id: String,
}

pub struct ParsedContent {
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub text_fragments: Vec<String>,
}

pub struct PerceptionResult {
    pub items: Vec<Perception>,
    pub map: AttachmentPerceptionMap,
}

pub struct SearchResult {
    pub memories: Vec<RelatedMemory>,
    pub topics: Vec<TopicSearchResult>,
}

/// 一次 turn 的上下文消息：对话流（窗口）+ 窗口外引用上下文。
#[derive(Debug)]
pub struct ContextMessages {
    /// 对话流（gather 增量窗口）
    window: Vec<Message>,
    window_by_id: HashMap<i64, usize>,
    /// 窗口外引用目标（reply_to_id 指向不在窗口的消息）
    reply: HashMap<i64, Message>,
}

/// 消息来源——决定 reference 渲染策略。
#[derive(Debug)]
pub enum MessageSource<'a> {
    /// 窗口内：已作为独立 `<msg>` 渲染，reference 截断 50 字符
    InWindow(&'a Message),
    /// 窗口外引用上下文：唯一呈现渠道，reference 保留全文
    Reply(&'a Message),
}

impl ContextMessages {
    pub fn new(window: Vec<Message>, reply: HashMap<i64, Message>) -> Self {
        let window_by_id = window.iter().enumerate().map(|(i, m)| (m.id, i)).collect();
        Self {
            window,
            window_by_id,
            reply,
        }
    }

    /// 对话流消息（渲染为独立 `<msg>`）
    pub fn window(&self) -> &[Message] {
        &self.window
    }

    /// 按 id 查消息（窗口优先），带来源标记。
    pub fn get(&self, id: i64) -> Option<MessageSource<'_>> {
        if let Some(&i) = self.window_by_id.get(&id) {
            return Some(MessageSource::InWindow(&self.window[i]));
        }
        self.reply.get(&id).map(MessageSource::Reply)
    }
}

impl<'a> MessageSource<'a> {
    pub fn message(&self) -> &'a Message {
        match self {
            MessageSource::InWindow(m) | MessageSource::Reply(m) => m,
        }
    }

    pub fn is_in_window(&self) -> bool {
        matches!(self, MessageSource::InWindow(_))
    }
}

/// 平台内容解析器
pub trait ContentParser: Send + Sync {
    fn parse(&self, value: &serde_json::Value) -> ParsedContent;
    fn create_renderer(&self, map: &AttachmentPerceptionMap) -> ContentRenderer;
}
