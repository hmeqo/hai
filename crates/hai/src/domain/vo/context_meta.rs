use serde::{Deserialize, Serialize};

/// 章节上下文元信息（持久化标量聚合）。
///
/// 描述"当前章节上下文的状态"——与 Turn 产物（events/上下文消息）解耦；
/// 重开章节（`start_new_chapter`）整体归零。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextMeta {
    /// 上下文占用 = 最后一次成功 Turn 的 prompt tokens（上下文超限触发点判据）。
    pub tokens: u32,
    /// 章节内成功 Turn 数（章节非空判定；Success/Steered 推进，Failed 不推进）。
    pub turn_count: u64,
    /// 章节规模（状态展示 / WrapUpCompleted.step_count）。
    pub step_count: u64,
}

impl ContextMeta {
    pub fn new() -> Self {
        Self::default()
    }

    /// Turn 正常结束（Success/Steered）后的推进：tokens 取最后一次调用的输入占用，
    /// turn_count +1、step_count 累加本轮 Step 数。
    pub fn advance(&mut self, step_count: usize, last_prompt_tokens: u32) {
        self.tokens = last_prompt_tokens;
        self.turn_count += 1;
        self.step_count += step_count as u64;
    }

    /// 章节非空判定（idle 到期需要重开的判据）。
    pub fn is_non_empty(&self) -> bool {
        self.turn_count > 0
    }
}
