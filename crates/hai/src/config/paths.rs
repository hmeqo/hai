use std::path::PathBuf;

use crate::config::{env, meta::PROJECT_NAME};

pub struct PathResolver;

impl PathResolver {
    fn local_dir() -> PathBuf {
        PathBuf::from(format!(".{PROJECT_NAME}"))
    }

    fn config_dir() -> PathBuf {
        let local = Self::local_dir();

        if local.exists() || env::local_mode() {
            return local;
        }

        dirs::config_dir()
            .map(|p| p.join(PROJECT_NAME))
            .unwrap_or(local)
    }

    /// 数据目录
    fn data_dir() -> PathBuf {
        let local = Self::local_dir();

        if local.exists() || env::local_mode() {
            return local;
        }

        dirs::data_dir()
            .map(|p| p.join(PROJECT_NAME))
            .unwrap_or(local)
    }

    /// 解析配置文件路径（支持多路径回退）
    pub fn config_file() -> PathBuf {
        let config_dir = Self::config_dir();
        config_dir.join("config.toml")
    }

    /// 解析 skills 目录列表
    pub fn skill_dirs() -> Vec<PathBuf> {
        let dirs = vec![
            Self::config_dir().join("skills"),
            Self::local_dir().join("skills"),
            PathBuf::from(".agents/skills"),
        ];

        dirs.into_iter().filter(|d| d.exists()).collect()
    }

    /// 附件缓存目录
    pub fn file_cache_dir() -> PathBuf {
        let mut path = Self::data_dir();
        path.push("files");
        path
    }
}
