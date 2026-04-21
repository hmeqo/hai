use std::path::PathBuf;

pub struct PathResolver;

impl PathResolver {
    /// 解析配置文件路径（支持多路径回退）
    pub fn config_file() -> PathBuf {
        let local = PathBuf::from(".hai/config.toml");

        if local.exists() {
            return local;
        }
        if let Some(mut path) = dirs::config_dir() {
            path.push("hai");
            path.push("config.toml");
            path
        } else {
            local
        }
    }

    /// 解析 skills 目录列表（按顺序加载，后者同名覆盖前者）
    /// 优先级：XDG_CONFIG_HOME/hai/skills > .hai/skills > .agents/skills
    pub fn skill_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        if let Some(mut path) = dirs::config_dir() {
            path.push("hai");
            path.push("skills");
            dirs.push(path);
        }

        dirs.push(PathBuf::from(".hai/skills"));
        dirs.push(PathBuf::from(".agents/skills"));

        dirs
    }
}
