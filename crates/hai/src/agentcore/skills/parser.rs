use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppResultExt, ErrorKind, OptionAppExt, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub allowed_tools: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub disable_model_invocation: bool,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub frontmatter: SkillFrontmatter,
    pub body: String,
    pub base_dir: PathBuf,
}

impl Skill {
    pub fn parse(content: &str, base_dir: PathBuf) -> Result<Self> {
        let (frontmatter, body) = extract_frontmatter(content).change_err_msg(
            ErrorKind::DataParse,
            "Failed to extract SKILL.md frontmatter",
        )?;

        let fm: SkillFrontmatter = serde_yml::from_str(frontmatter).change_err_msg(
            ErrorKind::DataParse,
            "Failed to parse SKILL.md frontmatter YAML",
        )?;

        Ok(Self {
            frontmatter: fm,
            body: body.trim().to_string(),
            base_dir,
        })
    }

    pub fn resolved_body(&self) -> String {
        let base = self.base_dir.to_string_lossy();
        self.body.replace("{baseDir}", &base)
    }

    pub fn discovery_entry(&self) -> String {
        format!(
            "\"{}\": {}",
            self.frontmatter.name, self.frontmatter.description
        )
    }
}

fn extract_frontmatter(content: &str) -> Result<(&str, &str)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return Err(ErrorKind::InvalidParameter
            .with_msg("SKILL.md must start with '---' frontmatter delimiter"));
    }

    let rest = &content[3..];
    let end = rest.find("\n---").ok_or_err_msg(
        ErrorKind::InvalidParameter,
        "SKILL.md frontmatter closing '---' not found",
    )?;

    let frontmatter = rest[..end].trim();
    let body = &rest[end + 4..];

    Ok((frontmatter, body))
}
