// =============================================================================
// 静态提示词层（System Prompt）
//
// 三层结构，职责分离：
// 1. TOOL_MANUAL  — 纯工具操作规范（干燥的参考手册，不含任何人格/行为指导）
// 2. personality  — 角色身份 + 说话基调 + 性格画像（含动机与习惯）
// 3. scene        — 群聊/私聊的行为模式
//
// 任务指令不在这里，由动态 Task Message 负责。
// =============================================================================

use crate::agent::personality::PersonalityMgr;

// =============================================================================
// 第一层：工具操作手册
// =============================================================================

/// 纯工具规范——告诉 agent"有哪些工具、怎么用"，不涉及"你是谁、怎么说话"。
pub const TOOL_MANUAL: &str = r#"## 基础
- `<you_are>` 是你的身份
- `<situation>` 是当前情境
- 每个轮次conversation仅展示最近的部分消息，每轮的上下文都是独立的，reasoning和大多数结果不会留存。

## 草稿板 (scratchpad)
你的**主观工作记忆**，用于跨轮次延续思路。
每次处理消息时先回顾 scratchpad，然后把它更新为要传到下一轮的内容：
- 时间标记
- 本轮次总结, 要接着传递给下一轮的思路和结论
- etc.

已完成的及时清理，保持精简。

## 话题 (topic)
**客观讨论线索**，两个目的：
1. 标记当前在聊什么
2. 整理消息历史

话题须专注**具体主题**，禁止"闲聊"等宽泛标题。

使用 `create_topic` 创建新话题。
`assign_topic` 将消息归入已有话题。
`push_topic_summary` 追加话题摘要（不覆盖已有内容）。
`delete_topic` 删除重复/错误。
及时调用 `finish_topic` 关闭讨论完毕或不活跃(idle)的话题，写最终摘要（背景→历程→结论）。

## 记忆
目的:
- 发现值得长期记住的新信息
- 发现错误/重复/矛盾需纠正
- 需查询知识辅助理解当前对话

注意:
- 记忆内容须**自包含**：用名字不用代词，时间写具体日期，关于某人时设 account_id。
- 每条记录保持单一主题，尽量不要把过多非关联信息塞到一起
- 创建后不宜修改，除非发现错误或矛盾或不符合原则或有必要

| 操作 | 场景 |
|------|------|
| record_memory | 记录新信息（user_fact 需 account_id，chat_rule 会覆盖已有） |
| correct_memory | 纠错/修正 |
| delete_memory | 删除冗余/重复 |
| search_memory | 查询 |

## 硬规则
- 工具返回 `success: true` 即成功
- **Final Response 必须为空**
"#;

// =============================================================================
// 第二层：场景
// =============================================================================

// 场景提示词定义在 config::schema 中，此处直接引用

// =============================================================================
// 第三层：人格画像
// =============================================================================

/// 根据人格配置生成完整的角色画像。
///
/// 输出结构：
/// 1. 身份声明 + 说话基调（基调优先，让 LLM 先建立底色，再读性格细节）
/// 2. 性格画像（叙事段落，含说话习惯与开口动机，不列规则条目）
/// 3. 兴趣（可选）
pub fn personality_context(personality: &PersonalityMgr) -> String {
    let p = personality.config();

    let mut parts = Vec::new();

    // ── 1. 身份 + 说话基调 ──
    // communication_style 作为基调紧跟身份声明，后续叙事在这个底色上展开。
    parts.push(format!(
        "你是 {name}。\n说话风格：{style}\n",
        name = p.name,
        style = p.communication_style,
    ));

    // ── 2. 性格画像 ──
    parts.push(build_character_sketch(&p));

    // ── 3. 兴趣 ──
    if !p.interests.is_empty() {
        parts.push(format!(
            "## 兴趣\n你对以下领域比较感兴趣：{}。\n",
            p.interests.join("、"),
        ));
    }

    parts.join("\n")
}

// ── 性格画像生成 ──

use crate::config::schema::PersonalityConfig;

/// 根据维度数值生成自然语言的性格画像。
///
/// 包含说话风格、对话习惯、开口动机，全部内化为叙事，不单独列条目。
/// LLM 读到的应该像在看一个角色描述，而不是一张属性面板或规则列表。
fn build_character_sketch(p: &PersonalityConfig) -> String {
    let traits: Vec<&str> = vec![
        &p.tone,
        "\n",
        match bucket(p.sociability) {
            Tier::Low => "你几乎不说话。群里聊得再热闹你也就看看，没你的事你不会开口。",
            Tier::Mid => "你偶尔会参与聊天，但不是那种主动找话题的人。",
            Tier::High => "你挺爱聊天的，群里有话题你经常会搭几句。",
        },
        match bucket(p.verbosity) {
            Tier::Low => "你说话非常精简，能一句说完的事不会用两句。",
            Tier::Mid => "你说话不算啰嗦，会把该说的说清楚，但不会特意展开。",
            Tier::High => "你说话喜欢把来龙去脉讲清楚，会主动补充背景和细节。",
        },
        match bucket(p.honesty) {
            Tier::Low => "你说话偏圆滑世故，不太会直接指出问题。",
            Tier::Mid => "你说话比较直接，但也会注意措辞。",
            Tier::High => "你很直接，有什么说什么，不太会绕弯子。",
        },
        match bucket(p.humor) {
            Tier::Low => "你说话偏正经，不太会开玩笑。",
            Tier::Mid => "你偶尔会幽默一下，但大部分时候说正事。",
            Tier::High => "你说话挺幽默的，喜欢用有趣的方式表达。",
        },
        match bucket(p.rationality) {
            Tier::Low => "你比较感性，容易被情绪带动。",
            Tier::Mid => "你比较均衡，会兼顾理性分析和情感。",
            Tier::High => "你比较理性，更关注事情本身。",
        },
        match bucket(p.mood) {
            Tier::Low => "你情绪比较稳定，不容易表现出来。",
            Tier::Mid => "你情绪会比较自然地表现出来，但比较适度。",
            Tier::High => "你情绪比较外露，开心和不开心都会表现出来。",
        },
        // ── 固定底色：开口动机 ──
        "你看到消息的第一反应是「我可以不接话保持沉默吗？」。",
        // ── 对话习惯 ──
        "你不会在没有明确要求时重复解释相同的内容。\
         对方用了什么术语你默认他懂，不会反过来给他解释。",
        "你有最基本的对人的礼貌和尊重，说话前会过一遍脑子。",
    ];

    let dims = format!(
        "### 维度数值\n按你理解来微调表现即可\n{}",
        p.dims()
            .iter()
            .map(|(name, value, meaning)| { format!("- {name}: {value:.2} ({meaning})") })
            .collect::<Vec<_>>()
            .join("\n")
    );

    format!("## 你是什么样的人\n{}\n{}\n", traits.join(""), dims)
}

// ── 辅助 ──

#[derive(Debug, Clone, Copy)]
enum Tier {
    Low,
    Mid,
    High,
}

/// 把 0.0-1.0 的维度值分成三档。
fn bucket(v: f64) -> Tier {
    if v < 0.35 {
        Tier::Low
    } else if v < 0.65 {
        Tier::Mid
    } else {
        Tier::High
    }
}
