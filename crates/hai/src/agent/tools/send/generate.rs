//! The image producer: making a picture is not sending one, so this tool only writes it down.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::{
        environment::sandbox::Sandbox,
        multimodal::{ImageSource, MultimodalService},
        tools::{
            addressing::{Address, deserialize_optional_addresses},
            source::{ResolvedSource, SourceResolver},
        },
    },
    agentcore::{
        api::ImageUrl,
        tool::{AgentTool, ToolCallContext, ToolError, ToolOutput, schema_of, tool_data, tool_err},
    },
    domain::vo::ImageFormat,
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateImageArgs {
    /// Image generation prompt; describe the scene, style and composition you want in detail
    pub prompt: String,
    /// Images that guide or edit the generation; shared source addresses (`attachment://`, `workspace://`, `file://`, http(s)); local images are inlined
    #[serde(default, deserialize_with = "deserialize_optional_addresses")]
    #[schemars(with = "Option<Vec<String>>")]
    pub reference_images: Option<Vec<Address>>,
}

pub struct GenerateImage {
    pub(super) sandbox: Arc<Sandbox>,
    pub(super) multimodal: MultimodalService,
    pub(super) resolver: SourceResolver,
    pub(super) max_reference_bytes: u64,
}

#[async_trait]
impl AgentTool for GenerateImage {
    fn name(&self) -> &str {
        "generate_image"
    }

    fn description(&self) -> &str {
        "Generate one image into the workspace and return its path. `reference_images` takes the shared \
         source addresses (`attachment://`, `workspace://`, `file://`, http(s)); local images are \
         inlined. Nothing is sent: pass that path to send_message with kind \"file\" if the image should \
         reach the chat."
    }

    fn schema(&self) -> Value {
        schema_of::<GenerateImageArgs>()
    }

    fn produces(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value, _ctx: &ToolCallContext) -> Result<ToolOutput, ToolError> {
        let args: GenerateImageArgs = serde_json::from_value(args)?;
        let references = args.reference_images.unwrap_or_default();
        let mut urls = Vec::with_capacity(references.len());
        for reference in references {
            urls.push(self.reference_url(&reference).await?);
        }
        let bytes = self.multimodal.generate_image(&args.prompt, &urls).await?;
        let path = write_image(&self.sandbox, &bytes, &args.prompt).await?;

        tool_data(json!({ "path": path })).map(ToolOutput::data)
    }
}

impl GenerateImage {
    async fn reference_url(&self, address: &Address) -> Result<ImageUrl, ToolError> {
        render_reference(
            self.resolver
                .resolve(address, self.max_reference_bytes)
                .await?,
        )
    }
}

fn render_reference(resolved: ResolvedSource) -> Result<ImageUrl, ToolError> {
    match resolved {
        ResolvedSource::Bytes { data, .. } => {
            let format = ImageFormat::sniff(&data).ok_or_else(|| {
                tool_err("a reference image is not an image format the model can read")
            })?;
            Ok(ImageUrl(ImageSource::bytes(data, format).into_image_url()))
        }
        ResolvedSource::Url { url, .. } => Ok(ImageUrl(url.as_str().to_owned())),
    }
}

async fn write_image(sandbox: &Sandbox, bytes: &[u8], prompt: &str) -> Result<String, ToolError> {
    let dir = sandbox.host_workspace().join("generated");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| tool_err(format!("{}: {e}", dir.display())))?;

    let stem = format!(
        "{}-{}",
        jiff::Timestamp::now().as_millisecond(),
        slug(prompt)
    );
    let mut name = format!("{stem}.png");
    let mut attempt = 1;
    while dir.join(&name).exists() {
        name = format!("{stem}-{attempt}.png");
        attempt += 1;
    }
    let path = dir.join(&name);
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| tool_err(format!("{}: {e}", path.display())))?;

    Ok(format!("workspace://generated/{name}"))
}

fn slug(prompt: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in prompt.chars() {
        if slug.len() >= 24 {
            break;
        }
        if character.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            separator = false;
            slug.push(character.to_ascii_lowercase());
        } else if character == '_' {
            separator = false;
            slug.push('_');
        } else {
            separator = true;
        }
    }
    if slug.is_empty() {
        "image".to_owned()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{GenerateImageArgs, ResolvedSource, Sandbox, render_reference, write_image};
    use crate::{
        agent::{tools::addressing::Address, web::WebUrl},
        agentcore::tool::schema_of,
    };

    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn a_prompt_becomes_a_file_name_front() {
        assert_eq!(super::slug("A Cat, sitting!"), "a-cat-sitting");
        assert_eq!(super::slug("!!!"), "image");
        assert_eq!(super::slug(""), "image");
        assert!(super::slug(&"x".repeat(50)).len() <= 24);
    }

    #[test]
    fn reference_arguments_arrive_typed() {
        let args: GenerateImageArgs = serde_json::from_value(json!({
            "prompt": "x",
            "reference_images": ["workspace://a.png", "https://example.com/a.png"]
        }))
        .expect("args");
        let references = args.reference_images.expect("references");
        assert!(
            matches!(references[0], Address::Workspace(ref path) if path == Path::new("a.png"))
        );
        assert!(
            matches!(references[1], Address::Url(ref url) if url.as_str() == "https://example.com/a.png")
        );
    }

    #[test]
    fn the_schema_keeps_reference_images_as_strings() {
        let schema = schema_of::<GenerateImageArgs>();
        assert_eq!(
            schema["properties"]["reference_images"]["items"]["type"], "string",
            "{schema}"
        );
    }

    #[test]
    fn a_local_reference_becomes_a_data_url() {
        let url = render_reference(ResolvedSource::Bytes {
            data: PNG_MAGIC.to_vec(),
            name: "a.png".into(),
        })
        .expect("data URL");
        assert!(url.0.starts_with("data:image/png;base64,"), "{}", url.0);
    }

    #[test]
    fn a_remote_reference_stays_a_url() {
        let target = "https://example.com/a.png";
        let url = render_reference(ResolvedSource::Url {
            url: WebUrl::parse(target).expect("URL"),
            name: "a.png".into(),
        })
        .expect("URL");
        assert_eq!(url.0, target);
    }

    #[test]
    fn a_non_image_reference_is_refused() {
        let error = render_reference(ResolvedSource::Bytes {
            data: b"not an image".to_vec(),
            name: "a.txt".into(),
        })
        .expect_err("not an image");
        assert!(error.to_string().contains("not an image format"), "{error}");
    }

    #[tokio::test]
    async fn a_generated_image_lands_in_the_workspace() {
        let (workspace, _dir) = crate::agent::test_support::created_workspace().await;
        let sandbox = Sandbox::host(workspace, 30);

        let first = write_image(&sandbox, b"png", "A Cat, sitting!")
            .await
            .expect("write");
        assert!(
            first.starts_with("workspace://generated/") && first.ends_with("-a-cat-sitting.png"),
            "{first}"
        );
        let name = first.trim_start_matches("workspace://");
        assert!(
            tokio::fs::metadata(sandbox.host_workspace().join(name))
                .await
                .is_ok(),
            "the file is on disk at {first}"
        );

        let second = write_image(&sandbox, b"png", "A Cat, sitting!")
            .await
            .expect("write");
        assert_ne!(first, second, "a second image does not overwrite the first");
    }
}
