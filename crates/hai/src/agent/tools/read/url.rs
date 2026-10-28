use super::{Read, ReadArgs, ReadOutput, paging::paginate};
use crate::{
    agent::{
        multimodal::analysis::{ResolvedMedia, resolve_parser},
        web::{Fetched, WebUrl},
    },
    agentcore::tool::ToolError,
    domain::vo::Subject,
};

impl Read {
    pub(super) async fn read_url(
        &self,
        url: &WebUrl,
        args: &ReadArgs,
    ) -> Result<ReadOutput, ToolError> {
        match self.web.fetch(url).await? {
            Fetched::Text { text, engine } => Ok(ReadOutput::Paged(paginate(
                &text,
                args.offset,
                args.limit,
                Some(engine),
            ))),
            fetched => {
                let (payload, detected) = fetched
                    .into_payload()
                    .expect("a non-text fetch carries a payload");
                let parser = resolve_parser(detected, args.parser)?;
                let content = self
                    .analyzer
                    .analyze(
                        ResolvedMedia {
                            parser,
                            payload,
                            subject: Some(Subject::url(url.as_str())),
                        },
                        args.focus.as_deref(),
                    )
                    .await?;
                Ok(ReadOutput::Text { content })
            }
        }
    }
}
