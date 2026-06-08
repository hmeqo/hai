use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use tracing::{debug, warn};

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub allowed_tools: Option<String>,
    pub model: Option<String>,
    pub version: Option<String>,
    pub disable_model_invocation: bool,
    pub body: String,
    pub base_dir: PathBuf,
}

impl Skill {
    pub fn resolved_body(&self) -> String {
        let base = self.base_dir.to_string_lossy();
        self.body.replace("{baseDir}", &base)
    }

    pub fn discovery_entry(&self) -> String {
        format!("\"{}\": {}", self.name, self.description)
    }
}

#[derive(Debug, Default, Clone)]
pub struct SkillManager {
    skills: Arc<Vec<Skill>>,
}

impl SkillManager {
    pub async fn load(dirs: &[PathBuf], disabled: &[String]) -> Result<Self> {
        let mut buf = Vec::new();
        for dir in dirs {
            if dir.exists() {
                Self::load_dir(&mut buf, dir, disabled).await;
            } else {
                debug!("Skills directory not found, skipping: {}", dir.display());
            }
        }
        Ok(Self {
            skills: Arc::new(buf),
        })
    }

    async fn load_dir(buf: &mut Vec<Skill>, dir: &Path, disabled: &[String]) {
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            warn!("Failed to read skills directory: {}", dir.display());
            return;
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let skill_file = Self::find_skill_file(&path);
            let Some(skill_file) = skill_file else {
                continue;
            };

            let content = match tokio::fs::read_to_string(&skill_file).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to read {}: {e}", skill_file.display());
                    continue;
                }
            };

            let parsed = match agent_skills::Skill::parse(&content) {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to parse {}: {e}", skill_file.display());
                    continue;
                }
            };

            let name = parsed.name().as_str().to_string();

            if disabled.contains(&name) {
                debug!("Skill '{}' is disabled, skipping", name);
                continue;
            }

            if buf.iter().any(|s| s.name == name) {
                debug!("Skill '{}' already loaded, skipping duplicate", name);
                continue;
            }

            let metadata = parsed.frontmatter().metadata();
            let model = metadata.and_then(|m| m.get("model")).map(String::from);
            let version = metadata.and_then(|m| m.get("version")).map(String::from);
            let disable_model_invocation = metadata
                .and_then(|m| m.get("disable-model-invocation"))
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);
            let allowed_tools = parsed
                .frontmatter()
                .allowed_tools()
                .map(|at| at.as_slice().join(" "));

            buf.push(Skill {
                name,
                description: parsed.description().as_str().to_string(),
                allowed_tools,
                model,
                version,
                disable_model_invocation,
                body: parsed.body_trimmed().to_string(),
                base_dir: path,
            });
        }
    }

    fn find_skill_file(dir: &Path) -> Option<PathBuf> {
        let candidates = [dir.join("SKILL.md"), dir.join("skill.md")];
        candidates.into_iter().find(|p| p.exists())
    }

    pub fn discoverable_skills(&self) -> impl Iterator<Item = &Skill> {
        self.skills.iter().filter(|s| !s.disable_model_invocation)
    }

    pub fn find(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
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
