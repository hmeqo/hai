//! Agent Skills 支持
//!
//! 实现 Anthropic 提出的 Agent Skills 规范：
//! - 扫描 skills 目录中的 SKILL.md 文件
//! - 解析 frontmatter（name, description, allowed-tools 等）
//! - 将 skill 列表注入到 system prompt（Level 1 发现）
//! - 提供 `load_skill` 元工具（Level 2 激活）：将 SKILL.md 正文注入对话上下文

mod manager;
mod parser;
mod tool;

pub use manager::SkillManager;
pub use parser::{Skill, SkillFrontmatter};
pub use tool::load_skill_tool;
