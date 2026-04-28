use std::sync::LazyLock;

use tiktoken_rs::CoreBPE;

static ENCODER: LazyLock<CoreBPE> =
    LazyLock::new(|| tiktoken_rs::cl100k_base().expect("Failed to load cl100k_base encoder"));

pub fn count_tokens(text: &str) -> usize {
    ENCODER.encode_with_special_tokens(text).len()
}

pub fn count_json_tokens(value: &serde_json::Value) -> usize {
    let s = value.to_string();
    count_tokens(&s)
}
