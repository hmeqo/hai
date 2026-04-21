//! SKILL.md 解析器
//!
//! SKILL.md 格式：
//! ```
//! ---
//! name: skill-name
//! description: 何时使用该 skill 的描述
//! allowed-tools: "Read,Write,Bash"   # 可选
//! model: "claude-opus-4"             # 可选
//! version: "1.0.0"                   # 可选
//! ---
//!
//! # Skill 正文（注入到对话的 Markdown 内容）
//! ...
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// SKILL.md frontmatter 元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SkillFrontmatter {
    /// 唯一标识符，用作 load_skill 工具的 command 参数
    pub name: String,
    /// LLM 用来决定何时调用此 skill 的关键描述
    pub description: String,
    /// 该 skill 允许使用的工具列表（逗号分隔），可选
    #[serde(default)]
    pub allowed_tools: Option<String>,
    /// 可选的模型覆盖
    #[serde(default)]
    pub model: Option<String>,
    /// 版本号（仅文档用途）
    #[serde(default)]
    pub version: Option<String>,
    /// 是否禁用 LLM 自动路由（只能手动调用）
    #[serde(default)]
    pub disable_model_invocation: bool,
}

/// 完整的 Skill 定义
#[derive(Debug, Clone)]
pub struct Skill {
    pub frontmatter: SkillFrontmatter,
    /// SKILL.md 中 frontmatter 之后的正文内容（Markdown）
    pub body: String,
    /// skill 所在目录的绝对路径（用于 {baseDir} 替换）
    pub base_dir: PathBuf,
}

impl Skill {
    /// 解析 SKILL.md 文件内容
    pub fn parse(content: &str, base_dir: PathBuf) -> Result<Self> {
        let (frontmatter, body) =
            extract_frontmatter(content).context("Failed to extract SKILL.md frontmatter")?;

        let fm: SkillFrontmatter = serde_yml::from_str(frontmatter)
            .context("Failed to parse SKILL.md frontmatter YAML")?;

        Ok(Self {
            frontmatter: fm,
            body: body.trim().to_string(),
            base_dir,
        })
    }

    /// 将 {baseDir} 替换为实际路径后返回正文
    pub fn resolved_body(&self) -> String {
        let base = self.base_dir.to_string_lossy();
        self.body.replace("{baseDir}", &base)
    }

    /// 用于 Level 1 发现列表的单行描述
    pub fn discovery_entry(&self) -> String {
        format!(
            "\"{}\": {}",
            self.frontmatter.name, self.frontmatter.description
        )
    }
}

/// 从 Markdown 文本中提取 `---` 包围的 frontmatter 和正文
fn extract_frontmatter(content: &str) -> Result<(&str, &str)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        bail!("SKILL.md must start with '---' frontmatter delimiter");
    }

    // 跳过开头的 ---
    let rest = &content[3..];
    // 找结束的 ---
    let end = rest
        .find("\n---")
        .context("SKILL.md frontmatter closing '---' not found")?;

    let frontmatter = rest[..end].trim();
    let body = &rest[end + 4..]; // 跳过 \n---

    Ok((frontmatter, body))
}
