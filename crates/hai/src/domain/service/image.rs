use std::{fmt::Write as _, path::PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::vo::{ConversationId, ImageId, ImageRef},
    error::{AppError, Result},
};

/// Content-addressed image bytes owned by one Conversation. The conversation boundary is also the
/// purge boundary; images never leak into another Conversation's directory.
#[derive(Clone, Debug)]
pub struct ImageStore {
    root: PathBuf,
}

impl ImageStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn put(
        &self,
        conversation_id: ConversationId,
        bytes: &[u8],
        mime: &str,
    ) -> Result<ImageRef> {
        if bytes.is_empty() {
            return Err(AppError::message("image bytes must not be empty"));
        }
        let mime = mime.trim();
        if mime.is_empty() {
            return Err(AppError::message("image mime must not be empty"));
        }

        let id = ImageId(hex_digest(bytes));
        let path = self.path(conversation_id, &id)?;
        let reference = ImageRef {
            id,
            mime: mime.to_owned(),
        };
        if tokio::fs::try_exists(&path).await? {
            return Ok(reference);
        }

        let dir = path
            .parent()
            .expect("an image path always has its Conversation directory");
        tokio::fs::create_dir_all(dir).await?;
        let temp = dir.join(format!(".tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&temp, bytes).await?;
        if let Err(error) = tokio::fs::rename(&temp, &path).await {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(error.into());
        }

        Ok(reference)
    }

    pub async fn get(
        &self,
        conversation_id: ConversationId,
        image: &ImageRef,
    ) -> Result<Option<Vec<u8>>> {
        let path = self.path(conversation_id, &image.id)?;
        match tokio::fs::read(path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Removes every file of this Conversation; returns the number removed.
    pub async fn clear(&self, conversation_id: ConversationId) -> Result<usize> {
        let dir = self.conversation_dir(conversation_id);
        let mut entries = match tokio::fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
        };

        let mut removed = 0;
        while let Some(entry) = entries.next_entry().await? {
            if tokio::fs::remove_file(entry.path()).await.is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub async fn purge(&self, conversation_id: ConversationId) -> Result<()> {
        match tokio::fs::remove_dir_all(self.conversation_dir(conversation_id)).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn conversation_dir(&self, conversation_id: ConversationId) -> PathBuf {
        self.root.join(conversation_id.0.to_string())
    }

    fn path(&self, conversation_id: ConversationId, id: &ImageId) -> Result<PathBuf> {
        if !valid_id(&id.0) {
            return Err(AppError::message(format!(
                "conversation {conversation_id}: invalid image id {:?}",
                id.0
            )));
        }
        Ok(self.conversation_dir(conversation_id).join(&id.0))
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("writing into a String cannot fail");
    }
    hex
}

fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::test_support::TempDir;

    const CONVERSATION: ConversationId = ConversationId(7);

    #[tokio::test]
    async fn one_content_hash_is_one_file() {
        let dir = TempDir::create("hai-image-store");
        let store = ImageStore::new(dir.path().to_path_buf());

        let first = store
            .put(CONVERSATION, b"same bytes", "image/png")
            .await
            .expect("image put");
        let second = store
            .put(CONVERSATION, b"same bytes", "image/png")
            .await
            .expect("image put");

        assert_eq!(first.id, second.id);
        assert_eq!(
            store.get(CONVERSATION, &first).await.expect("image get"),
            Some(b"same bytes".to_vec())
        );
        assert_eq!(entries(&store).await, 1);
    }

    #[tokio::test]
    async fn clear_removes_every_image_and_purge_removes_the_directory() {
        let dir = TempDir::create("hai-image-clear");
        let store = ImageStore::new(dir.path().to_path_buf());
        let image = store
            .put(CONVERSATION, b"bytes", "image/png")
            .await
            .expect("image put");

        assert_eq!(store.clear(CONVERSATION).await.expect("image clear"), 1);
        assert!(
            store
                .get(CONVERSATION, &image)
                .await
                .expect("image get")
                .is_none()
        );

        store
            .put(CONVERSATION, b"bytes", "image/png")
            .await
            .expect("image put");
        store.purge(CONVERSATION).await.expect("image purge");
        assert!(!store.conversation_dir(CONVERSATION).exists());
    }

    #[tokio::test]
    async fn a_malformed_id_is_refused_before_a_path_is_built() {
        let dir = TempDir::create("hai-image-id");
        let store = ImageStore::new(dir.path().to_path_buf());
        let image = ImageRef {
            id: ImageId("../escape".to_owned()),
            mime: "image/png".to_owned(),
        };

        assert!(store.get(CONVERSATION, &image).await.is_err());
    }

    async fn entries(store: &ImageStore) -> usize {
        let mut entries = tokio::fs::read_dir(store.conversation_dir(CONVERSATION))
            .await
            .expect("image directory");
        let mut count = 0;
        while entries.next_entry().await.expect("entry").is_some() {
            count += 1;
        }
        count
    }

    #[test]
    fn the_directory_is_named_by_the_conversation() {
        let store = ImageStore::new(Path::new("/images").to_path_buf());
        assert_eq!(
            store.conversation_dir(CONVERSATION),
            Path::new("/images").join("7")
        );
    }
}
