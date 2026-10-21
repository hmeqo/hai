//! Test helpers the agent layer's modules share.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    agent::{
        channel::{CallbackAnswer, ChannelAccess, ChoiceOption},
        environment::workspace::Workspace,
    },
    agentcore::tool::{AgentTool, ToolError},
    domain::vo::ChatId,
    error::{AppError, Result},
    test_support::TempDir,
};

/// One document the recording access received: chat, filename, bytes, caption.
pub(crate) type RecordedDocument = (String, String, Vec<u8>, String);

/// The inbound half of a channel that records what the host said, so a test can assert on the pages
/// it drew and the presses it answered. The two refusals a platform can report are switchable.
#[derive(Default)]
pub(crate) struct RecordingAccess {
    pub asked: Mutex<Vec<AskedPage>>,
    pub edited: Mutex<Vec<EditedPage>>,
    /// Each answer as `quiet`, `toast: …` or `alert: …`.
    pub answered: Mutex<Vec<String>>,
    /// Each page removal as `(chat, message)`.
    pub deleted: Mutex<Vec<(String, String)>>,
    pub documents: Mutex<Vec<RecordedDocument>>,
    /// Whether the platform refuses to rewrite a message, and whether it refuses to deliver a file.
    pub edit_fails: bool,
    pub document_fails: bool,
}

/// One question the host put out.
pub(crate) struct AskedPage {
    pub chat_external_id: String,
    pub prompt: String,
    pub options: Vec<ChoiceOption>,
}

/// One in-place rewrite of a question.
pub(crate) struct EditedPage {
    pub prompt: String,
    pub options: Vec<ChoiceOption>,
}

impl std::fmt::Debug for RecordingAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordingAccess").finish()
    }
}

#[async_trait]
impl ChannelAccess for RecordingAccess {
    fn admits(&self, _chat_external_id: &str) -> bool {
        true
    }

    fn command_prefix(&self) -> &'static str {
        "/"
    }

    async fn notice(&self, _chat_external_id: &str, _text: &str) -> Result<()> {
        Ok(())
    }

    async fn ask(
        &self,
        chat_external_id: &str,
        prompt: &str,
        options: &[ChoiceOption],
    ) -> Result<()> {
        self.asked
            .lock()
            .expect("test channel lock")
            .push(AskedPage {
                chat_external_id: chat_external_id.to_owned(),
                prompt: prompt.to_owned(),
                options: options.to_vec(),
            });
        Ok(())
    }

    async fn edit_choice(
        &self,
        _chat_external_id: &str,
        _message_external_id: &str,
        prompt: &str,
        options: &[ChoiceOption],
    ) -> Result<()> {
        self.edited
            .lock()
            .expect("test channel lock")
            .push(EditedPage {
                prompt: prompt.to_owned(),
                options: options.to_vec(),
            });
        if self.edit_fails {
            return Err(AppError::message("the message is gone"));
        }
        Ok(())
    }

    async fn answer_callback(&self, _query_id: &str, answer: CallbackAnswer<'_>) -> Result<()> {
        let recorded = match answer {
            CallbackAnswer::Quiet => "quiet".to_owned(),
            CallbackAnswer::Toast(text) => format!("toast: {text}"),
            CallbackAnswer::Alert(text) => format!("alert: {text}"),
        };
        self.answered
            .lock()
            .expect("test channel lock")
            .push(recorded);
        Ok(())
    }

    async fn delete_notice(&self, chat_external_id: &str, message_external_id: &str) -> Result<()> {
        self.deleted
            .lock()
            .expect("test channel lock")
            .push((chat_external_id.to_owned(), message_external_id.to_owned()));
        Ok(())
    }

    async fn notice_document(
        &self,
        chat_external_id: &str,
        filename: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<()> {
        if self.document_fails {
            return Err(AppError::message("nowhere to deliver it"));
        }
        self.documents.lock().expect("test channel lock").push((
            chat_external_id.to_owned(),
            filename.to_owned(),
            bytes,
            caption.to_owned(),
        ));
        Ok(())
    }
}

/// A tool whose metadata a test sets explicitly; every field left unset keeps the trait default
/// (silent, turn-continuing, non-producing) and the call returns `null`.
pub(crate) struct StubTool {
    name: String,
    ends_turn: bool,
    speaks: bool,
    produces: bool,
    reads_only: bool,
}

impl StubTool {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ends_turn: false,
            speaks: false,
            produces: false,
            reads_only: false,
        }
    }

    pub(crate) fn ends_turn(mut self, value: bool) -> Self {
        self.ends_turn = value;
        self
    }

    pub(crate) fn speaks(mut self, value: bool) -> Self {
        self.speaks = value;
        self
    }

    pub(crate) fn produces(mut self, value: bool) -> Self {
        self.produces = value;
        self
    }

    pub(crate) fn reads_only(mut self, value: bool) -> Self {
        self.reads_only = value;
        self
    }

    pub(crate) fn into_dyn(self) -> Arc<dyn AgentTool> {
        Arc::new(self)
    }
}

#[async_trait]
impl AgentTool for StubTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        ""
    }

    fn schema(&self) -> Option<Value> {
        None
    }

    async fn execute(&self, _args: Value) -> Result<Value, ToolError> {
        Ok(Value::Null)
    }

    fn ends_turn(&self) -> bool {
        self.ends_turn
    }

    fn speaks(&self) -> bool {
        self.speaks
    }

    fn produces(&self) -> bool {
        self.produces
    }

    fn reads_only(&self) -> bool {
        self.reads_only
    }
}

/// A workspace belonging to a chat that does not exist, plus the guard that removes its directory: a
/// test needs a real directory only when it writes into one.
pub(crate) async fn created_workspace() -> (Workspace, TempDir) {
    let workspace = Workspace::create(ChatId(uuid::Uuid::new_v4().as_u128() as i64))
        .await
        .expect("workspace directory");
    let dir = TempDir::adopt(workspace.dir().to_path_buf());
    (workspace, dir)
}
