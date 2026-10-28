//! Where a tool's media argument comes from: the one place an address becomes bytes or a URL.
//!
//! A local or workspace path is read through the sandbox, an attachment through its platform
//! handle, and an HTTP URL stays a URL for the consumer to hand on. The resolver applies the
//! sandbox's access rule and the caller's byte ceiling; it never calls a model.

use std::sync::Arc;

#[cfg(test)]
use async_trait::async_trait;

use crate::{
    agent::{
        attachment::AttachmentSource, environment::sandbox::Sandbox, tools::addressing::Address,
        web::WebUrl,
    },
    agentcore::tool::{ToolError, tool_err},
};
#[cfg(test)]
use crate::{
    domain::{model::Attachment, vo::AttachmentId},
    error::Result,
};

/// Resolves the media addresses a tool accepts.
#[derive(Clone)]
pub(crate) struct SourceResolver {
    sandbox: Arc<Sandbox>,
    attachments: Arc<dyn AttachmentSource>,
}

/// What an address resolved to: inline bytes with a name, or a URL to hand on.
#[derive(Debug)]
pub(crate) enum ResolvedSource {
    Bytes { data: Vec<u8>, name: String },
    Url { url: WebUrl, name: String },
}

impl SourceResolver {
    pub(crate) fn new(sandbox: Arc<Sandbox>, attachments: Arc<dyn AttachmentSource>) -> Self {
        Self {
            sandbox,
            attachments,
        }
    }

    /// Resolves one address; `0` is an unbounded byte ceiling.
    pub(crate) async fn resolve(
        &self,
        address: &Address,
        limit: u64,
    ) -> Result<ResolvedSource, ToolError> {
        match address {
            Address::Local(_) | Address::Workspace(_) => {
                let path = address
                    .local_path(&self.sandbox)
                    .expect("the arm matched an address that names a local path");
                self.sandbox.ensure_readable(&path)?;
                let name = file_name_of(&path.to_string_lossy());
                let metadata = tokio::fs::metadata(&path)
                    .await
                    .map_err(|e| tool_err(format!("{}: {e}", path.display())))?;
                refuse(&name, metadata.len(), limit)?;
                let data = tokio::fs::read(&path)
                    .await
                    .map_err(|e| tool_err(format!("{}: {e}", path.display())))?;
                Ok(ResolvedSource::Bytes { data, name })
            }
            Address::Attachment(id) => {
                let attachment = self
                    .attachments
                    .attachment(*id)
                    .await?
                    .ok_or_else(|| tool_err(format!("no attachment has id {id}")))?;
                let data = self.attachments.download(&attachment.file_id).await?;
                let name = attachment
                    .meta
                    .file_name
                    .clone()
                    .unwrap_or_else(|| attachment.file_id.clone());
                refuse(&name, data.len() as u64, limit)?;
                Ok(ResolvedSource::Bytes { data, name })
            }
            Address::Url(url) => Ok(ResolvedSource::Url {
                url: url.clone(),
                name: file_name_of(url.path()),
            }),
            Address::Skill(_) | Address::Topic(_) | Address::Memory(_) => Err(tool_err(
                "only file://, workspace://, attachment:// and http(s):// can be used as a media source",
            )),
        }
    }
}

fn refuse(name: &str, len: u64, limit: u64) -> Result<(), ToolError> {
    if limit != 0 && len > limit {
        return Err(tool_err(format!(
            "{name} is larger than the {} MB this tool takes",
            limit / (1024 * 1024)
        )));
    }
    Ok(())
}

/// The last path segment, or `file` when there is none.
fn file_name_of(path: &str) -> String {
    path.rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("file")
        .to_owned()
}

#[cfg(test)]
pub(crate) struct NoAttachments;

#[cfg(test)]
#[async_trait]
impl AttachmentSource for NoAttachments {
    async fn attachment(&self, _id: AttachmentId) -> Result<Option<Attachment>> {
        Ok(None)
    }

    async fn download(&self, _file_id: &str) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }

    async fn file_url(&self, _file_id: &str) -> Result<String> {
        Ok(String::new())
    }
}

#[cfg(test)]
pub(crate) struct FixedAttachments {
    pub(crate) attachment: Attachment,
    pub(crate) bytes: Vec<u8>,
}

#[cfg(test)]
#[async_trait]
impl AttachmentSource for FixedAttachments {
    async fn attachment(&self, id: AttachmentId) -> Result<Option<Attachment>> {
        Ok((self.attachment.id == id).then(|| self.attachment.clone()))
    }

    async fn download(&self, _file_id: &str) -> Result<Vec<u8>> {
        Ok(self.bytes.clone())
    }

    async fn file_url(&self, _file_id: &str) -> Result<String> {
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{
            environment::sandbox::Sandbox, test_support::created_workspace,
            tools::addressing::parse,
        },
        domain::vo::AttachmentKind,
    };

    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    fn attachment() -> Attachment {
        let mut attachment = crate::domain::test_support::attachment(AttachmentKind::Photo);
        attachment.meta.file_name = Some("ref.png".into());
        attachment
    }

    #[tokio::test]
    async fn a_local_file_is_read_as_bytes() {
        let (workspace, _dir) = created_workspace().await;
        let sandbox = Arc::new(Sandbox::host(workspace, 30));
        tokio::fs::write(sandbox.host_workspace().join("a.png"), PNG_MAGIC)
            .await
            .expect("write");

        let resolved = SourceResolver::new(sandbox, Arc::new(NoAttachments))
            .resolve(&parse("workspace://a.png").expect("address"), 0)
            .await
            .expect("local source");

        match resolved {
            ResolvedSource::Bytes { data, name } => {
                assert_eq!(data, PNG_MAGIC);
                assert_eq!(name, "a.png");
            }
            other => panic!("expected bytes, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_url_stays_a_url() {
        let (workspace, _dir) = created_workspace().await;
        let sandbox = Arc::new(Sandbox::host(workspace, 30));
        let resolved = SourceResolver::new(sandbox, Arc::new(NoAttachments))
            .resolve(&parse("https://example.com/a.png?x=1").expect("address"), 0)
            .await
            .expect("URL source");

        match resolved {
            ResolvedSource::Url { url, name } => {
                assert_eq!(url.as_str(), "https://example.com/a.png?x=1");
                assert_eq!(name, "a.png");
            }
            other => panic!("expected a URL, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_attachment_is_downloaded() {
        let (workspace, _dir) = created_workspace().await;
        let sandbox = Arc::new(Sandbox::host(workspace, 30));
        let row = attachment();
        let resolver = SourceResolver::new(
            sandbox,
            Arc::new(FixedAttachments {
                attachment: row.clone(),
                bytes: PNG_MAGIC.to_vec(),
            }),
        );

        let resolved = resolver
            .resolve(
                &parse(&format!("attachment://{}", row.id)).expect("address"),
                0,
            )
            .await
            .expect("attachment");

        match resolved {
            ResolvedSource::Bytes { data, name } => {
                assert_eq!(data, PNG_MAGIC);
                assert_eq!(name, "ref.png");
            }
            other => panic!("expected bytes, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unsupported_address_is_refused() {
        let (workspace, _dir) = created_workspace().await;
        let sandbox = Arc::new(Sandbox::host(workspace, 30));
        let error = SourceResolver::new(sandbox, Arc::new(NoAttachments))
            .resolve(&parse("skill://demo").expect("address"), 0)
            .await
            .expect_err("not a media source");
        assert!(error.to_string().contains("only file://"), "{error}");
    }

    #[tokio::test]
    async fn a_source_over_the_limit_is_refused() {
        let (workspace, _dir) = created_workspace().await;
        let sandbox = Arc::new(Sandbox::host(workspace, 30));
        tokio::fs::write(sandbox.host_workspace().join("a.png"), PNG_MAGIC)
            .await
            .expect("write");

        let error = SourceResolver::new(sandbox, Arc::new(NoAttachments))
            .resolve(&parse("workspace://a.png").expect("address"), 3)
            .await
            .expect_err("over the limit");
        assert!(error.to_string().contains("larger than"), "{error}");
    }

    #[tokio::test]
    async fn an_unknown_attachment_is_refused() {
        let (workspace, _dir) = created_workspace().await;
        let sandbox = Arc::new(Sandbox::host(workspace, 30));
        let row = attachment();
        let error = SourceResolver::new(sandbox, Arc::new(NoAttachments))
            .resolve(
                &parse(&format!("attachment://{}", row.id)).expect("address"),
                0,
            )
            .await
            .expect_err("no attachment row");
        assert!(
            error.to_string().contains("no attachment has id"),
            "{error}"
        );
    }
}
