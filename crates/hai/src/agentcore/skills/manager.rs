use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use super::parser::Skill;
use crate::error::Result;

#[derive(Debug, Default)]
pub struct SkillManager {
    skills: Vec<Skill>,
}

impl SkillManager {
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

    pub fn discoverable_skills(&self) -> impl Iterator<Item = &Skill> {
        self.skills
            .iter()
            .filter(|s| !s.frontmatter.disable_model_invocation)
    }

    pub fn find(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.frontmatter.name == name)
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

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
