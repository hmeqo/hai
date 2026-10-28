//! The `read` tool: every source it can open, the pagination it applies to a long one, and the
//! surface it advertises so the model can choose among them.

mod local;
mod media;
mod paging;
mod record;
mod skill;
mod url;

use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use strum::IntoEnumIterator;

use self::paging::{Page, page_data};
use super::addressing::{self, Address};
use crate::{
    agent::{
        attachment::ToolAttachments,
        environment::sandbox::Sandbox,
        multimodal::{
            MultimodalService,
            analysis::{InlineLimit, MediaAnalyzer, MediaResolver},
        },
        process::AgentRuntime,
        run::turn::ToolContext,
        web::{WebService, WebUrl},
    },
    agentcore::{
        skills::{SkillManager, SkillName},
        tool::{AgentTool, ToolCallContext, ToolError, ToolMedia, ToolOutput, schema_of},
    },
    domain::{
        service::Services,
        vo::{AttachmentId, AttachmentParser, ConversationId, MemoryId, TopicId},
    },
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// file://<path> (a bare path means the same), workspace://<path>, http(s)://<url>, attachment://<uuid>, skill://<name>, topic://<id>, memory://<id>
    #[serde(deserialize_with = "addressing::deserialize_address")]
    #[schemars(with = "String")]
    pub path: Address,
    /// Starting position, 1-based, counted in the unit the result reports (`lines`, `chars` or `notes`)
    pub offset: Option<u64>,
    /// Maximum amount to read, counted in the unit the result reports (`lines`, `chars` or `notes`)
    pub limit: Option<u64>,
    /// Attachment parsers (image, video, audio): analyze against this instruction instead of the default full transcription
    pub focus: Option<String>,
    /// `ocr` reads an image as raw text instead of a description; any other value must match the file
    pub parser: Option<AttachmentParser>,
}

pub struct Read {
    description: String,
    conversation_id: ConversationId,
    web: WebService,
    analyzer: MediaAnalyzer,
    resolver: MediaResolver,
    services: Services,
    sandbox: Arc<Sandbox>,
    skill_manager: SkillManager,
}

enum ReadOutput {
    Text { content: String },
    Paged(Page),
    Skill { name: String, instructions: String },
    Image { media: ToolMedia },
}

impl ReadOutput {
    fn into_tool_output(self) -> ToolOutput {
        let data = match self {
            Self::Text { content } => json!({ "content": content }),
            Self::Paged(page) => page_data(page),
            Self::Skill { name, instructions } => {
                json!({ "skill": name, "instructions": instructions })
            }
            Self::Image { media } => {
                let mut output = ToolOutput::ok(json!({ "content": "Image" }));
                output.media = vec![media];
                return output;
            }
        };
        ToolOutput::ok(data)
    }
}

#[async_trait]
impl AgentTool for Read {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Value {
        schema_of::<ReadArgs>()
    }

    async fn execute(&self, args: Value, ctx: &ToolCallContext) -> Result<ToolOutput, ToolError> {
        let args: ReadArgs = serde_json::from_value(args)?;
        Ok(self.read(&args, ctx).await?.into_tool_output())
    }

    /// A read changes nothing: it opens what the chat already has.
    fn reads_only(&self) -> bool {
        true
    }
}

enum Target<'a> {
    Attachment(AttachmentId),
    Skill(&'a SkillName),
    Topic(TopicId),
    Memory(MemoryId),
    Url(&'a WebUrl),
    Local(PathBuf),
}

/// The address resolved against the sandbox: where a read actually comes from.
fn target<'a>(address: &'a Address, sandbox: &Sandbox) -> Target<'a> {
    match address {
        Address::Local(_) | Address::Workspace(_) => Target::Local(
            address
                .local_path(sandbox)
                .expect("the arm matched an address that names a local path"),
        ),
        Address::Attachment(id) => Target::Attachment(*id),
        Address::Skill(name) => Target::Skill(name),
        Address::Topic(id) => Target::Topic(*id),
        Address::Memory(id) => Target::Memory(*id),
        Address::Url(url) => Target::Url(url),
    }
}

impl Read {
    async fn read(&self, args: &ReadArgs, ctx: &ToolCallContext) -> Result<ReadOutput, ToolError> {
        let focus = args.focus.as_deref();
        match target(&args.path, &self.sandbox) {
            Target::Attachment(id) => {
                self.read_attachment(id, args.parser, focus, ctx.image_input)
                    .await
            }
            Target::Skill(name) => skill::skill_body(&self.skill_manager, name),
            Target::Topic(id) => {
                record::topic(
                    &self.services,
                    self.conversation_id,
                    id,
                    args.offset,
                    args.limit,
                )
                .await
            }
            Target::Memory(id) => record::memory(&self.services, self.conversation_id, id).await,
            Target::Url(url) => self.read_url(url, args).await,
            Target::Local(path) => self.read_local(&path, args, ctx.image_input).await,
        }
    }
}

pub fn tools(runtime: &AgentRuntime, ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    let parsers = enabled_parsers(
        &ctx.multimodal,
        runtime.model_catalog().any_chat_image_input(),
    );
    let inline_limit = InlineLimit::new(ctx.multimodal.inline_max_bytes());
    vec![Arc::new(Read {
        description: describe(&parsers, !ctx.skill_manager.is_empty()),
        conversation_id: ctx.conversation_id,
        web: runtime.web.clone(),
        analyzer: MediaAnalyzer::new(
            ctx.multimodal.clone(),
            ctx.services.clone(),
            ctx.conversation_id,
            inline_limit,
        ),
        resolver: MediaResolver::new(
            Arc::new(ToolAttachments::new(ctx.services.clone(), ctx.chat.clone())),
            inline_limit,
        ),
        services: ctx.services.clone(),
        sandbox: ctx.sandbox.clone(),
        skill_manager: ctx.skill_manager.clone(),
    })]
}

/// Lists only the sources this process can actually serve.
fn describe(parsers: &[AttachmentParser], skills: bool) -> String {
    let kinds: Vec<&str> = AttachmentParser::iter()
        .filter(|parser| parsers.contains(parser))
        .map(|parser| parser.name())
        .collect();
    let media = if kinds.is_empty() {
        String::new()
    } else {
        format!(
            "; media is analyzed ({}) and an image is loaded directly when the active model can see it",
            kinds.join("/")
        )
    };
    let mut lines = vec![
        "Reads content. `path` accepts:".to_owned(),
        format!(
            "- file://<path> (a bare path means the same; a directory is listed): text read line by line{media}"
        ),
        "- workspace://<path> (a relative path also resolves here): your workspace".to_owned(),
        format!("- http/https URL: the page converted to body text{media}"),
    ];
    if !parsers.is_empty() {
        lines.push(format!(
            "- attachment://<uuid>: an attachment in the chat (the id of `<attachment id=\"…\">`); parseable: {}",
            kinds.join("/")
        ));
    }
    if parsers.contains(&AttachmentParser::Image) {
        lines.push(
            "- `parser`: `ocr` reads an image as raw text instead of a description".to_owned(),
        );
    }
    if skills {
        lines.push("- skill://<name>: loads that skill's detailed instructions".to_owned());
    }
    lines.push(
        "- topic://<id>: topic archive (summary + message range + raw material; offset/limit count notes, everything by default)"
            .to_owned(),
    );
    lines.push("- memory://<id>: memory entry (content + status + source messages)".to_owned());
    lines.join("\n")
}

/// The attachment parsers the model is told about: only what the resolved service can actually read,
/// plus an image a native vision chat model can load without the analyzer.
fn enabled_parsers(
    multimodal: &MultimodalService,
    chat_image_input: bool,
) -> Vec<AttachmentParser> {
    let mut parsers = Vec::new();
    if multimodal.transcription_enabled() {
        parsers.push(AttachmentParser::Audio);
    }
    if multimodal.vision_enabled() {
        parsers.push(AttachmentParser::Video);
        parsers.push(AttachmentParser::Image);
    } else if chat_image_input {
        parsers.push(AttachmentParser::Image);
    }
    parsers
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{agent::environment::workspace::Workspace, domain::vo::ConversationId};

    /// The local path or the kind a `path` argument resolves to, as text so the borrow stays inside.
    fn resolved(path: &str, sandbox: &Sandbox) -> String {
        let address = addressing::parse(path).expect("address");
        match target(&address, sandbox) {
            Target::Local(path) => path.display().to_string(),
            Target::Attachment(_) => "attachment".to_owned(),
            Target::Skill(name) => format!("skill:{}", name.as_str()),
            Target::Topic(_) => "topic".to_owned(),
            Target::Memory(_) => "memory".to_owned(),
            Target::Url(url) => format!("url:{}", url.as_str()),
        }
    }

    #[test]
    fn a_target_resolves_local_addresses_against_the_workspace() {
        let workspace = Workspace::for_conversation(ConversationId(4242));
        let root = workspace.dir().display().to_string();
        let sandbox = Sandbox::host(workspace, 30);
        assert_eq!(resolved("a.md", &sandbox), format!("{root}/a.md"));
        assert_eq!(
            resolved("workspace://n/x.md", &sandbox),
            format!("{root}/n/x.md")
        );
        assert_eq!(resolved("file:///tmp/a.md", &sandbox), "/tmp/a.md");
        assert_eq!(
            resolved(
                "attachment://3f0c1a9e-1b2c-4d5e-8f90-112233445566",
                &sandbox
            ),
            "attachment"
        );
        assert_eq!(resolved("skill://demo", &sandbox), "skill:demo");
        assert_eq!(
            resolved("topic://3f0c1a9e-1b2c-4d5e-8f90-112233445566", &sandbox),
            "topic"
        );
        assert_eq!(
            resolved("memory://3f0c1a9e-1b2c-4d5e-8f90-112233445566", &sandbox),
            "memory"
        );
        assert_eq!(
            resolved("HTTP://Example.COM/a?b=1", &sandbox),
            "url:http://example.com/a?b=1"
        );
    }

    #[test]
    fn the_path_argument_arrives_typed() {
        let args: ReadArgs =
            serde_json::from_value(json!({ "path": "workspace://n/x.md" })).expect("args");
        assert!(matches!(args.path, Address::Workspace(path) if path == Path::new("n/x.md")));

        // The wire form is still one string, so a bad scheme or id fails where every argument does.
        let scheme = serde_json::from_value::<ReadArgs>(json!({ "path": "ftp://host/x" }))
            .expect_err("unknown scheme");
        assert!(scheme.to_string().contains("Unsupported scheme"));
        let id = serde_json::from_value::<ReadArgs>(json!({ "path": "topic://nope" }))
            .expect_err("bad id");
        assert!(id.to_string().contains("Invalid topic id"));
    }

    #[test]
    fn an_image_output_carries_bytes_in_media_not_text() {
        let output = ReadOutput::Image {
            media: ToolMedia::Image {
                mime: "image/png".into(),
                data: vec![1, 2, 3],
                focus: Some("what is this".into()),
            },
        }
        .into_tool_output();

        assert_eq!(output.media.len(), 1);
        assert!(!output.data.to_string().contains("base64"));
        assert_eq!(output.data["data"]["content"], "Image");
    }

    #[test]
    fn describe_lists_available_sources() {
        assert!(!describe(&[], false).contains("attachment://"));
        assert!(!describe(&[], false).contains("skill://"));
        assert!(describe(&[], true).contains("skill://<name>"));
        assert!(describe(&[], false).contains("file://<path>"));
        assert!(describe(&[], false).contains("workspace://"));
        assert!(
            describe(&[AttachmentParser::Image, AttachmentParser::Audio], false)
                .contains("image/audio")
        );
    }
}
