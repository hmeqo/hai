//! SkillManager：扫描目录、缓存 skills、提供查询接口

use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::{debug, warn};

use super::parser::Skill;

/// Skill 管理器
///
/// 负责从配置的目录列表中扫描 SKILL.md 文件并缓存解析结果。
/// 扫描顺序：全局目录 → 项目目录（后者覆盖同名 skill）。
#[derive(Debug, Default)]
pub struct SkillManager {
    skills: Vec<Skill>,
}

impl SkillManager {
    /// 从多个目录加载 skills（后加载的同名 skill 覆盖先加载的）
    pub async fn load(dirs: &[PathBuf]) -> Result<Self> {
        let mut manager = Self::default();
        for dir in dirs {
            if dir.exists() {
                manager.load_dir(dir).await;
            } else {
                debug!("Skills directory not found, skipping: {}", dir.display());
            }
        }
        Ok(manager)
    }

    /// 扫描指定目录下所有子目录中的 SKILL.md
    async fn load_dir(&mut self, dir: &Path) {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            warn!("Failed to read skills directory: {}", dir.display());
            return;
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let skill_file = if path.join("SKILL.md").exists() {
                path.join("SKILL.md")
            } else {
                path.join("skill.md")
            };
            if !skill_file.exists() {
                continue;
            }

            match tokio::fs::read_to_string(&skill_file).await {
                Ok(content) => match Skill::parse(&content, path.clone()) {
                    Ok(skill) => {
                        debug!("Loaded skill: {}", skill.frontmatter.name);
                        // 同名 skill 覆盖
                        self.skills
                            .retain(|s| s.frontmatter.name != skill.frontmatter.name);
                        self.skills.push(skill);
                    }
                    Err(e) => {
                        warn!("Failed to parse {}: {e}", skill_file.display());
                    }
                },
                Err(e) => {
                    warn!("Failed to read {}: {e}", skill_file.display());
                }
            }
        }
    }

    /// 返回所有可被 LLM 自动路由的 skills（未设置 disable_model_invocation）
    pub fn discoverable_skills(&self) -> impl Iterator<Item = &Skill> {
        self.skills
            .iter()
            .filter(|s| !s.frontmatter.disable_model_invocation)
    }

    /// 按名称查找 skill
    pub fn find(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.frontmatter.name == name)
    }

    /// 是否有任何 skill 已加载
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// 生成 Level 1 发现列表文本（注入到 system prompt）
    pub fn discovery_prompt(&self) -> Option<String> {
        let entries: Vec<String> = self
            .discoverable_skills()
            .map(|s| format!("  - {}", s.discovery_entry()))
            .collect();

        if entries.is_empty() {
            return None;
        }

        Some(format!(
            "## Skills\n\
            你可以通过调用 `load_skill` 工具来激活以下专项能力（skills）。\
            当用户请求与某个 skill 的描述匹配时，请调用对应的 skill：\n\
            {}\n\
            调用 `load_skill` 后，你会收到该 skill 的详细指令，请严格按照指令执行。",
            entries.join("\n")
        ))
    }
}
