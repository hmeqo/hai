use crate::config::schema::PersonalityConfig;

/// 渲染 `# 人格` 节。
pub fn personality_context(p: &PersonalityConfig) -> String {
    format!("你是 {name}。\n{p}", name = p.name, p = p.description)
}
