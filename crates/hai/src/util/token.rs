//! Token 计数工具
//!
//! 封装 tiktoken 调用，提供全局复用的编码器和统一的 token 计数接口。

use std::sync::LazyLock;

use anyhow::Result;
use tiktoken_rs::CoreBPE;

static ENCODER: LazyLock<CoreBPE> =
    LazyLock::new(|| tiktoken_rs::cl100k_base().expect("Failed to load cl100k_base encoder"));

/// 计算字符串的 token 数量
pub fn count_tokens(text: &str) -> usize {
    ENCODER.encode_with_special_tokens(text).len()
}

/// 计算 JSON 值的 token 数量
pub fn count_json_tokens(value: &serde_json::Value) -> Result<usize> {
    let s = value.to_string();
    Ok(count_tokens(&s))
}

/// 计算多个字符串的总 token 数量
pub fn count_tokens_batch(texts: &[&str]) -> usize {
    texts.iter().map(|t| count_tokens(t)).sum()
}
