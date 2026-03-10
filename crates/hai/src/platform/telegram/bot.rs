use std::sync::Arc;

use anyhow::Result;
use tap::Tap;
use teloxide::{
    Bot,
    dispatching::dialogue::InMemStorage,
    dptree,
    net::Download,
    payloads::SendMessageSetters,
    prelude::*,
    types::{Me, MessageEntityKind, ReplyParameters},
    utils::command::BotCommands,
};
use tokio::sync::mpsc;

use crate::{
    agent::{AgentHandler, ImageOptions},
    config::AppConfig,
    app::{
        domain::{
            entity::{ChatType, Platform},
            model::{
                MessageMeta, PlatformAccountMeta, PlatformMessageMeta, TelegramAccountMeta,
                TelegramContentPart, TelegramMessageMeta,
            },
        },
        service::ServiceContext,
    },
    trigger::{AgentEvent, GroupTrigger, TriggerStatus},
};

const MAJOR_HELP_TEXT: &str = r#"
/image <消息内容> - 生成图像
  示例: /image 一只可爱的小猫在花园里玩耍
  说明: 根据描述或引用生成图像
"#;

#[derive(Debug, Clone, Default)]
pub enum State {
    #[default]
    Start,
}

#[derive(Debug, BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub enum Command {
    #[command(description = "Start")]
    Start,
    #[command(aliases = ["h", "?"], description = "显示帮助信息")]
    Help,
    #[command(description = "图像生成")]
    ToImage(#[command(description = "消息内容")] String),
    #[command(aliases = ["t"], description = "解析为文字")]
    ToText(#[command(description = "消息内容")] String),
    #[command(description = "切换模型")]
    Model(#[command(description = "模型名称")] String),
    #[command(description = "显示当前模型")]
    CurrentModel,
    #[command(description = "显示可用模型")]
    AvailableModels,
    #[command(description = "当前会话状态")]
    Status,
}

pub struct BotHandler {
    bot: Bot,
    agent_handler: Arc<AgentHandler>,
    agent_event_tx: mpsc::UnboundedSender<AgentEvent>,
    services: Arc<ServiceContext>,
    group_trigger: Arc<GroupTrigger>,
    allowed_chat_ids: Vec<i64>,
}

impl BotHandler {
    pub async fn new(
        config: &AppConfig,
        bot: Bot,
        agent_handler: Arc<AgentHandler>,
        agent_event_tx: mpsc::UnboundedSender<AgentEvent>,
        services: Arc<ServiceContext>,
        group_trigger: Arc<GroupTrigger>,
    ) -> Result<Self> {
        bot.set_my_commands(Command::bot_commands()).await?;

        Ok(Self {
            bot,
            agent_handler,
            agent_event_tx,
            services,
            group_trigger,
            allowed_chat_ids: config.telegram.allowed_chat_ids.clone(),
        })
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        let handler = Update::filter_message()
            .branch(
                dptree::entry()
                    .filter(|msg: Message, bh: Arc<BotHandler>| {
                        bh.is_allowed_chat(msg.chat.id).tap(|allowed| {
                            if !allowed {
                                tracing::warn!(
                                    "Unauthorized message attempt from chat_id: {}, user: {:?}",
                                    msg.chat.id,
                                    msg.from.as_ref().map(|u| &u.username)
                                );
                            }
                        })
                    })
                    .branch(dptree::entry().filter_command::<Command>().endpoint(
                        |bot: Bot, msg: Message, cmd: Command, bh: Arc<BotHandler>| async {
                            tokio::spawn(Self::handle_command(bot, msg, cmd, bh));
                            Ok::<(), anyhow::Error>(())
                        },
                    ))
                    .endpoint(|msg: Message, bh: Arc<BotHandler>, me: Me| async move {
                        let _ = bh.handle_message(msg, me).await;
                        Ok::<(), anyhow::Error>(())
                    }),
            )
            .branch(dptree::entry().enter_dialogue::<Message, InMemStorage<State>, State>())
            .endpoint(|_: Bot, _: Message, _: Arc<BotHandler>| async { Ok(()) });

        Dispatcher::builder(self.bot.clone(), handler)
            .dependencies(dptree::deps![self, InMemStorage::<State>::new()])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .tap(|_| tracing::info!("Started"))
            .await;

        Ok(())
    }

    async fn handle_message(&self, msg: Message, me: Me) -> Result<()> {
        let Some(from) = msg.from.as_ref() else {
            return Ok(());
        };
        let chat_type = msg_chat_type(&msg);

        // 1. 平台身份解析 & 消息持久化
        let (chat, account) = self.resolve_chat_and_account(&msg, from, chat_type).await?;
        self.persist_user_message(&msg, chat.id, account.id).await?;

        // 2. Agent 触发决策
        self.dispatch_agent_event(chat.id, chat_type, &msg, &me);

        Ok(())
    }

    /// 获取消息对应的内部 chat_id（通过 upsert 确保记录存在）
    async fn get_internal_chat_id(&self, msg: &Message) -> Result<i64> {
        let Some(from) = msg.from.as_ref() else {
            anyhow::bail!("No sender");
        };
        let (chat, _) = self
            .resolve_chat_and_account(msg, from, msg_chat_type(msg))
            .await?;
        Ok(chat.id)
    }

    /// 解析 Telegram 消息中的 chat 和 account，确保内部记录存在
    async fn resolve_chat_and_account(
        &self,
        msg: &Message,
        from: &teloxide::types::User,
        chat_type: ChatType,
    ) -> Result<(
        crate::app::domain::entity::Chat,
        crate::app::domain::entity::Account,
    )> {
        let account_meta = PlatformAccountMeta::Telegram(TelegramAccountMeta {
            first_name: from.first_name.clone(),
            last_name: from.last_name.clone(),
            username: from.username.clone(),
        });
        self.services
            .platform
            .ensure_chat_and_account(
                Platform::Telegram,
                &msg.chat.id.to_string(),
                chat_type,
                msg.chat.title(),
                &from.id.to_string(),
                Some(serde_json::to_value(account_meta)?),
            )
            .await
    }

    /// 将用户消息持久化到数据库
    async fn persist_user_message(
        &self,
        msg: &Message,
        chat_id: i64,
        account_id: i64,
    ) -> Result<()> {
        let reply_to_id = if let Some(reply) = msg.reply_to_message() {
            self.services
                .message
                .find_id_by_external_id(chat_id, &reply.id.0.to_string())
                .await?
        } else {
            None
        };

        let (content, meta) = extract_message_content(msg);
        self.services
            .message
            .save_user_message(
                chat_id,
                account_id,
                serde_json::to_value(content)?,
                &msg.id.0.to_string(),
                reply_to_id,
                meta,
                Some(jiff::Timestamp::from_second(msg.date.timestamp())?.into()),
            )
            .await?;
        Ok(())
    }

    /// 根据消息类型和内容决定是否触发 Agent，并发送对应事件
    fn dispatch_agent_event(&self, chat_id: i64, chat_type: ChatType, msg: &Message, me: &Me) {
        use crate::trigger::TriggerReason;

        if chat_type == ChatType::Private {
            let _ = self.agent_event_tx.send(AgentEvent::Message {
                chat_id,
                reason: TriggerReason::Private,
            });
            return;
        }

        let is_mention = is_mentioning_user(msg, me.user.username.as_deref().unwrap_or(""));
        if is_mention {
            self.group_trigger.on_mention(chat_id);
            if let Err(e) = self.agent_event_tx.send(AgentEvent::Message {
                chat_id,
                reason: TriggerReason::Mention,
            }) {
                tracing::error!("Failed to send mention event: {}", e);
            }
        } else if let Some(reason) = self.group_trigger.on_message(chat_id) {
            let _ = self
                .agent_event_tx
                .send(AgentEvent::Message { chat_id, reason });
        }
    }

    async fn handle_command(
        bot: Bot,
        msg: Message,
        cmd: Command,
        bh: Arc<BotHandler>,
    ) -> Result<()> {
        let chat_id = msg.chat.id;
        match cmd {
            Command::Start => {
                bot.send_message(chat_id, "Hello!").await?;
            }
            Command::Help => {
                bot.send_message(
                    chat_id,
                    format!("{}\n{MAJOR_HELP_TEXT}", Command::descriptions()),
                )
                .await?;
            }
            Command::ToImage(prompt) => {
                Self::handle_image(&bot, &msg, &bh, prompt).await?;
            }
            Command::ToText(prompt) => {
                Self::handle_to_text(&bot, &msg, &bh, prompt).await?;
            }
            Command::Model(model) => {
                bh.agent_handler.switch_model(&model)?;
                bot.send_message(chat_id, format!("已切换到模型: {}", model))
                    .await?;
            }
            Command::CurrentModel => {
                let current_model = bh.agent_handler.current_model();
                bot.send_message(chat_id, format!("当前模型: {}", current_model))
                    .await?;
            }
            Command::AvailableModels => {
                bot.send_message(chat_id, "https://openrouter.ai/models")
                    .await?;
            }
            Command::Status => {
                let inner_chat_id = bh.get_internal_chat_id(&msg).await?;
                let s = bh.group_trigger.status(inner_chat_id);
                let text = format_trigger_status(&s);
                bot.send_message(chat_id, text).await?;
            }
        }
        Ok(())
    }

    async fn extract_reply_media(reply: &Message, bot: &Bot) -> Result<Option<(String, String)>> {
        if let Some(text) = reply.text() {
            Ok(Some((String::new(), text.to_string())))
        } else {
            let file_id = if let Some(photos) = reply.photo() {
                photos.last().unwrap().file.id.clone()
            } else if let Some(document) = reply.document() {
                document.file.id.clone()
            } else {
                return Ok(None);
            };
            let url = get_file_url(bot, &file_id).await?;
            let content = reply.caption().unwrap_or("").to_string();
            Ok(Some((url, content)))
        }
    }

    async fn handle_image(
        bot: &Bot,
        msg: &Message,
        bh: &Arc<BotHandler>,
        prompt: String,
    ) -> Result<()> {
        let chat_id = msg.chat.id;
        let mut image_url = None;
        if let Some(reply) = msg.reply_to_message() {
            if let Some((url, _)) = Self::extract_reply_media(reply, bot).await? {
                image_url = Some(url);
            }
        }
        let data = bh
            .agent_handler
            .image(ImageOptions { prompt, image_url })
            .await?;
        let file = data.tmp_file().await?;
        bot.send_photo(chat_id, teloxide::types::InputFile::file(file.path()))
            .reply_parameters(ReplyParameters::new(msg.id))
            .await?;
        Ok(())
    }

    async fn handle_to_text(
        bot: &Bot,
        msg: &Message,
        bh: &Arc<BotHandler>,
        prompt: String,
    ) -> Result<()> {
        if let Some(reply) = msg.reply_to_message() {
            let (file_id, format) = if let Some(audio) = reply.audio() {
                (&audio.file.id, audio.mime_type.as_ref().unwrap().as_ref())
            } else if let Some(voice) = reply.voice() {
                (&voice.file.id, voice.mime_type.as_ref().unwrap().as_ref())
            } else {
                return Ok(());
            };
            let audio_data = get_file_content(bot, file_id).await?;
            let resp = bh
                .agent_handler
                .analyze_audio(&prompt, &audio_data, format)
                .await?;
            bot.send_message(msg.chat.id, &resp)
                .reply_parameters(ReplyParameters::new(msg.id))
                .await?;
        }
        Ok(())
    }

    fn is_allowed_chat(&self, chat_id: ChatId) -> bool {
        self.allowed_chat_ids.contains(&chat_id.0)
    }
}

/// 从 Telegram Message 中提取 ChatType
fn msg_chat_type(msg: &Message) -> ChatType {
    if msg.chat.is_private() {
        ChatType::Private
    } else {
        ChatType::Group
    }
}

/// 格式化触发状态信息（用于 /status 命令）
fn format_trigger_status(s: &TriggerStatus) -> String {
    format!(
        "📊 Agent 触发状态\n\
         \n\
         上次回复间隔: {} 秒\n\
         窗口状态: {}\n\
         \n\
         触发概率: {} {:.1}%",
        s.window_elapsed_secs,
        if s.is_in_window {
            "✅ 窗口期内"
        } else {
            "⚪ 窗口期外"
        },
        render_bar(s.trigger_probability),
        s.trigger_probability * 100.0,
    )
}

/// 渲染一个简单的进度条，长度 10 格
fn render_bar(value: f64) -> String {
    let filled = (value.clamp(0.0, 1.0) * 10.0).round() as usize;
    let empty = 10usize.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

/// 检查消息是否提及了用户
fn is_mentioning_user(msg: &Message, username: &str) -> bool {
    let Some(entities) = msg.entities() else {
        return false;
    };
    let username = format!("@{}", username);

    entities.iter().any(|e| {
        if !matches!(e.kind, MessageEntityKind::Mention) {
            return false;
        }
        // Telegram entity offset/length 以 UTF-16 code unit 计算，
        // 需转换为 char 边界后再提取，避免多字节字符导致 panic
        let mention_text = msg.text().and_then(|text| {
            // 用 UTF-16 编码后按 code unit 切片，再解码回 String
            let utf16: Vec<u16> = text.encode_utf16().collect();
            let end = e.offset + e.length;
            let slice = utf16.get(e.offset..end)?;
            String::from_utf16(slice).ok()
        });
        mention_text.as_deref() == Some(&username)
    })
}

/// 提取消息内容和元数据
///
/// 返回：(Vec<TelegramContentPart>, 可选的 platform metadata)
fn extract_message_content(msg: &Message) -> (Vec<TelegramContentPart>, Option<serde_json::Value>) {
    let mut parts = Vec::new();
    let caption = msg.caption().map(|c| c.to_string());

    // 1. 处理文本
    if let Some(text) = msg.text() {
        parts.push(TelegramContentPart::Text {
            text: text.to_string(),
        });
    }

    // 2. 处理媒体
    if let Some(photos) = msg.photo() {
        if let Some(photo) = photos.last() {
            parts.push(TelegramContentPart::Photo {
                file_id: photo.file.id.clone(),
                width: photo.width,
                height: photo.height,
                caption: caption.clone(),
            });
        }
    } else if let Some(video) = msg.video() {
        parts.push(TelegramContentPart::Video {
            file_id: video.file.id.clone(),
            caption: caption.clone(),
        });
    } else if let Some(audio) = msg.audio() {
        parts.push(TelegramContentPart::Audio {
            file_id: audio.file.id.clone(),
            caption: caption.clone(),
        });
    } else if let Some(voice) = msg.voice() {
        parts.push(TelegramContentPart::Voice {
            file_id: voice.file.id.clone(),
        });
    } else if let Some(document) = msg.document() {
        parts.push(TelegramContentPart::Document {
            file_id: document.file.id.clone(),
            file_name: document.file_name.clone(),
            caption: caption.clone(),
        });
    } else if let Some(sticker) = msg.sticker() {
        parts.push(TelegramContentPart::Sticker {
            file_id: sticker.file.id.clone(),
            emoji: sticker.emoji.clone(),
        });
    } else if let Some(animation) = msg.animation() {
        parts.push(TelegramContentPart::Animation {
            file_id: animation.file.id.clone(),
        });
    } else if let Some(video_note) = msg.video_note() {
        parts.push(TelegramContentPart::VideoNote {
            file_id: video_note.file.id.clone(),
        });
    }

    // 3. 提取元数据 (如 thread_id)
    let tg_meta = TelegramMessageMeta {
        message_thread_id: msg.thread_id.map(|id| id.0.0),
    };
    let meta = MessageMeta {
        platform: Some(PlatformMessageMeta::Telegram(tg_meta)),
        llm: None,
    };

    (parts, serde_json::to_value(meta).ok())
}

async fn get_file_url(bot: &teloxide::Bot, file_id: &str) -> Result<String> {
    let file = bot.get_file(file_id).await?;
    Ok(format!(
        "https://api.telegram.org/file/bot{}/{}",
        bot.token(),
        file.path
    ))
}

async fn get_file_content(bot: &teloxide::Bot, file_id: &str) -> Result<Vec<u8>> {
    let file = bot.get_file(file_id).await?;
    let mut data = Vec::new();
    bot.download_file(&file.path, &mut data).await?;
    Ok(data)
}
