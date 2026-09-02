use std::path::PathBuf;

use crate::{config::Paths, error::Result, util::path::sanitize_path};

#[derive(Debug)]
pub struct FileCache {
    cache_dir: PathBuf,
}

impl FileCache {
    pub fn new() -> Self {
        let cache_dir = Paths::inferred().file_cache_dir().to_owned();
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir).ok();
        }
        Self { cache_dir }
    }

    pub fn with_tag(tag: &str) -> Self {
        let cache_dir = Paths::inferred().file_cache_dir().join(tag);
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir).ok();
        }
        Self { cache_dir }
    }

    pub fn find(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.key_to_path(key);
        std::fs::read(path).ok()
    }

    pub fn add(&self, key: &str, data: &[u8]) -> Result<()> {
        let path = self.key_to_path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, data)?;
        Ok(())
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(sanitize_path(key))
    }
}

impl Default for FileCache {
    fn default() -> Self {
        Self::new()
    }
}
