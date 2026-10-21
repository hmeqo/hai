//! The chat's own directory on the host.

use std::path::{Path, PathBuf};

use crate::{
    config::Paths,
    domain::vo::{ChatId, TimeRange},
    error::Result,
};

/// The directory a chat's tools work in.
#[derive(Debug)]
pub struct Workspace {
    dir: PathBuf,
}

impl Workspace {
    /// A chat's workspace is named here and nowhere else.
    pub fn for_chat(chat_id: ChatId) -> Self {
        Self {
            dir: Paths::inferred().workspaces_dir().join(chat_id.to_string()),
        }
    }

    pub async fn create(chat_id: ChatId) -> Result<Self> {
        let workspace = Self::for_chat(chat_id);
        tokio::fs::create_dir_all(workspace.dir()).await?;
        Ok(workspace)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Deletes the regular files under this directory whose modification time falls inside `range`,
    /// deepest last, and then the directories they leave empty. Symlinks are neither followed nor
    /// deleted, and one entry that cannot be removed is logged and skipped — the purge is best
    /// effort about a directory anything on the host may be writing to. The count is of files that
    /// were actually removed; a workspace that does not exist is nothing to remove.
    pub async fn remove_files_in(&self, range: TimeRange) -> Result<u64> {
        let mut removed = 0;
        let mut directories = Vec::new();
        let mut pending = vec![self.dir.clone()];
        while let Some(dir) = pending.pop() {
            let mut entries = match tokio::fs::read_dir(&dir).await {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    tracing::warn!(dir = %dir.display(), error = %e, "workspace unreadable");
                    continue;
                }
            };
            loop {
                let entry = match entries.next_entry().await {
                    Ok(Some(entry)) => entry,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(dir = %dir.display(), error = %e, "workspace unreadable");
                        break;
                    }
                };
                let path = entry.path();
                let Ok(metadata) = tokio::fs::symlink_metadata(&path).await else {
                    continue;
                };
                if metadata.is_dir() {
                    directories.push(path.clone());
                    pending.push(path);
                    continue;
                }
                if !metadata.is_file() {
                    // A symlink, a socket, a fifo: not something a chat's own tools wrote as content.
                    continue;
                }
                let modified = match metadata.modified() {
                    Ok(modified) => modified,
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "modification time unreadable; file left in place",
                        );
                        continue;
                    }
                };
                let Ok(at) = jiff::Timestamp::try_from(modified) else {
                    tracing::warn!(
                        path = %path.display(),
                        "modification time out of range; file left in place",
                    );
                    continue;
                };
                if !in_window(range, at) {
                    continue;
                }
                match tokio::fs::remove_file(&path).await {
                    Ok(()) => removed += 1,
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "file not removed")
                    }
                }
            }
        }

        // A directory the purge emptied goes too; one that still holds something is left alone.
        directories.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
        for dir in directories {
            if let Err(e) = tokio::fs::remove_dir(&dir).await
                && e.kind() != std::io::ErrorKind::DirectoryNotEmpty
            {
                tracing::warn!(dir = %dir.display(), error = %e, "directory not removed");
            }
        }
        Ok(removed)
    }
}

/// Whether a timestamp falls inside a window; a missing bound is unbounded on that side.
fn in_window(range: TimeRange, at: jiff::Timestamp) -> bool {
    range.since.is_none_or(|since| at >= since) && range.until.is_none_or(|until| at <= until)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chat_gets_its_own_directory_under_the_workspaces_root() {
        let first = Workspace::for_chat(ChatId(4242));
        assert_eq!(first.dir(), Paths::inferred().workspaces_dir().join("4242"));
        assert_ne!(first.dir(), Workspace::for_chat(ChatId(4243)).dir());
    }

    /// A purge deletes the files the window holds, whatever depth they sit at, and takes the
    /// directories they empty with them — while the workspace itself stays, because the chat's tools
    /// still work in it.
    #[tokio::test]
    async fn only_the_files_inside_the_window_go() {
        let (workspace, _guard) = crate::agent::test_support::created_workspace().await;
        let nested = workspace.dir().join("sub");
        tokio::fs::create_dir_all(&nested)
            .await
            .expect("a nested directory");
        tokio::fs::write(workspace.dir().join("root.txt"), b"content")
            .await
            .expect("a file");
        tokio::fs::write(nested.join("deep.txt"), b"content")
            .await
            .expect("a file");

        let now = jiff::Timestamp::now();
        let hour = jiff::SignedDuration::from_secs(3_600);

        let closed = TimeRange::new(None, Some(now - hour));
        assert_eq!(
            workspace
                .remove_files_in(closed)
                .await
                .expect("a workspace"),
            0,
            "a window that closed before the files were written holds none"
        );
        assert!(workspace.dir().join("root.txt").exists());
        assert!(nested.join("deep.txt").exists());

        let open = TimeRange::new(Some(now - hour), None);
        assert_eq!(
            workspace.remove_files_in(open).await.expect("a workspace"),
            2
        );
        assert!(!workspace.dir().join("root.txt").exists());
        assert!(!nested.exists(), "a directory the purge emptied goes too");
        assert!(workspace.dir().exists(), "the workspace itself stays");
    }

    /// A symlink is neither followed nor deleted: reaching whatever it points at would take the purge
    /// outside the workspace it was asked about.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlink_is_left_alone() {
        let (workspace, _guard) = crate::agent::test_support::created_workspace().await;
        let outside = crate::test_support::TempDir::create("hai-workspace-outside");
        let target = outside.path().join("target.txt");
        tokio::fs::write(&target, b"content").await.expect("a file");
        let link = workspace.dir().join("link.txt");
        tokio::fs::symlink(&target, &link).await.expect("a symlink");

        let removed = workspace
            .remove_files_in(TimeRange::new(None, None))
            .await
            .expect("a workspace");

        assert_eq!(removed, 0);
        assert!(
            tokio::fs::symlink_metadata(&link).await.is_ok(),
            "the link itself stays"
        );
        assert!(target.exists(), "what it points at is not ours to delete");
    }
}
