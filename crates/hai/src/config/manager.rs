use arc_swap::ArcSwap;
use config::{Environment, File};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use struct_patch::Patch;
use tokio::fs;
use tokio::sync::Mutex;

use crate::error::{ErrorKind, Result};

// --- 核心 Trait 定义 ---

/// 可配置对象的抽象接口
pub trait Configurable: Patch<Self::Patch> + Default + Clone + Send + Sync + 'static {
    /// 对应的补丁类型（由 struct_patch 生成）
    type Patch: Default + Serialize + for<'de> Deserialize<'de> + Clone + Send + std::fmt::Debug;
}

// --- 配置管理器实现 ---

#[derive(Debug)]
struct ConfigInner<T: Configurable> {
    file_path: String,
    intent: Mutex<T::Patch>,
    env_intent: ArcSwap<Option<T::Patch>>,
    current: ArcSwap<T>,
}

#[derive(Debug, Clone)]
pub struct Config<T: Configurable> {
    inner: Arc<ConfigInner<T>>,
}

impl<T: Configurable> Config<T> {
    pub fn from_file(file_path: &str) -> Result<Self> {
        let intent = Self::try_load_patch_from_file(file_path).unwrap_or_default();

        let manager = Self {
            inner: Arc::new(ConfigInner {
                file_path: file_path.to_string(),
                intent: Mutex::new(intent.clone()),
                env_intent: ArcSwap::from_pointee(None),
                current: ArcSwap::from_pointee(T::default()),
            }),
        };

        manager.rebuild_and_store(&intent);
        Ok(manager)
    }

    pub fn with_env(self, prefix: &str) -> Result<Self> {
        let env_intent = Self::load_patch_from_env(prefix)?;
        self.inner.env_intent.store(Arc::new(Some(env_intent)));
        Ok(self)
    }

    /// 获取当前配置的只读快照
    pub fn load(&self) -> Arc<T> {
        self.inner.current.load_full()
    }

    /// 更新配置意图（仅修改内存，需调用 save 持久化）
    pub async fn update<F>(&self, modifier: F)
    where
        F: FnOnce(&mut T::Patch),
    {
        let mut intent_guard = self.inner.intent.lock().await;
        modifier(&mut intent_guard);
        self.rebuild_and_store(&intent_guard);
    }

    /// 将当前内存中的意图保存到文件
    pub async fn save(&self) -> Result<()> {
        let intent_guard = self.inner.intent.lock().await;
        let path = Path::new(&self.inner.file_path);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("toml");

        let content = match ext {
            // "toml" => toml::to_string_pretty(&*intent_guard)?,
            "json" => serde_json::to_string_pretty(&*intent_guard)?,
            _ => return Err(ErrorKind::Config.with_message(format!("不支持的文件格式: {}", ext))),
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&self.inner.file_path, content).await?;
        Ok(())
    }

    /// 从文件重新加载配置（丢弃未保存的内存修改）
    pub async fn reload(&self) -> Result<()> {
        let Some(new_intent) = Self::try_load_patch_from_file(&self.inner.file_path) else {
            return Ok(());
        };
        let mut intent_guard = self.inner.intent.lock().await;
        *intent_guard = new_intent;
        self.rebuild_and_store(&intent_guard);
        Ok(())
    }

    // --- 内部辅助方法 ---

    fn rebuild_and_store(&self, file_intent: &T::Patch) {
        let mut next_config = T::default();
        next_config.apply(file_intent.clone());
        if let Some(env_intent) = self.inner.env_intent.load_full().deref() {
            next_config.apply(env_intent.clone());
        }
        self.inner.current.store(Arc::new(next_config));
    }

    fn try_load_patch_from_file(path: &str) -> Option<T::Patch> {
        Self::load_patch_from_file(path).ok()
    }

    fn load_patch_from_file(path: &str) -> Result<T::Patch> {
        Ok(config::Config::builder()
            .add_source(File::with_name(path).required(false))
            .build()?
            .try_deserialize()?)
    }

    fn load_patch_from_env(prefix: &str) -> Result<T::Patch> {
        Ok(config::Config::builder()
            .add_source(Environment::with_prefix(prefix).separator("__"))
            .build()?
            .try_deserialize()?)
    }
}
