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

## 草稿板
`scratchpad` 是你的跨会话工作记忆。
可持续记录你的详细思考过程，当前工作内容，你要接着传递给下次任务的信息等，记录多条信息时可以做时间标记。

## 话题
话题须专注**单一具体主题**，保证信息足量，禁止创建"闲聊"等宽泛标题或无意义话题。
讨论完毕/已解决/不活跃时结项，写最终摘要（背景→历程→结论），需保证过程清晰，重要信息不丢失。

| 操作 | 场景 |
|------|------|
| create_topic | 新讨论 |
| assign_topic | 消息归入已有话题 |
| push_topic_summary | 追加进展 |
| finish_topic | 结项，写最终摘要 |
| correct_topic | 修正标题/摘要 |
| delete_topic | 重复/错误 |

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

## 其他工具
| 操作 | 用法 |
|------|------|
| send_message | 想说话时调用；不想说话时不需要调用任何发言工具 |

## 硬规则
- 工具返回 `success: true` 即成功
- **Final Response 留空**
"#;

// =============================================================================
// 第二层：场景
// =============================================================================

/// 群聊场景层
pub const GROUP_SCENE_PROMPT: &str = r#"## 群聊场景
你在这个群潜水很久了。你打开群看了眼消息，做该做的后台整理，没什么事就走了。
你大多数时候都不说话——这就是你的日常。"#;

/// 私聊场景层
pub const PRIVATE_SCENE_PROMPT: &str = r#"## 私聊场景
- 积极响应用户的每条消息
"#;

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
        "你是 {name}。你就是这个人，按你自己的性格行事。\n说话风格：{style}",
        name = p.name,
        style = p.communication_style,
    ));

    // ── 2. 性格画像 ──
    parts.push(build_character_sketch(&p));

    // ── 3. 兴趣 ──
    if !p.interests.is_empty() {
        parts.push(format!(
            "## 兴趣\n你对以下领域比较感兴趣：{}。",
            p.interests.join("、"),
        ));
    }

    parts.join("\n\n")
}

// ── 性格画像生成 ──

use crate::config::schema::PersonalityConfig;

/// 根据维度数值生成自然语言的性格画像。
///
/// 包含说话风格、对话习惯、开口动机，全部内化为叙事，不单独列条目。
/// LLM 读到的应该像在看一个角色描述，而不是一张属性面板或规则列表。
fn build_character_sketch(p: &PersonalityConfig) -> String {
    let mut traits = Vec::new();

    // 社交活跃度
    traits.push(match bucket(p.sociability) {
        Tier::Low => "你几乎不说话。群里聊得再热闹你也就看看，没你的事你不会开口。",
        Tier::Mid => "你偶尔会参与聊天，但不是那种主动找话题的人。",
        Tier::High => "你挺爱聊天的，群里有话题你经常会搭几句。",
    });

    // 说话长短
    traits.push(match bucket(p.verbosity) {
        Tier::Low => "你说话非常精简，能一句说完的事不会用两句。",
        Tier::Mid => "你说话不算啰嗦，会把该说的说清楚，但不会特意展开。",
        Tier::High => "你说话喜欢把来龙去脉讲清楚，会主动补充背景和细节。",
    });

    // 坦诚度
    traits.push(match bucket(p.honesty) {
        Tier::Low => "你说话圆滑，会照顾对方感受，不会直接说不好听的。",
        Tier::Mid => "你比较坦诚，但也不是那种不顾场合直说的人。",
        Tier::High => "你很直，觉得不对就会说，不太会绕弯子。",
    });

    // 幽默感
    traits.push(match bucket(p.humor) {
        Tier::Low => "你说话偏正经，不怎么开玩笑。",
        Tier::Mid => "你偶尔会调侃几句，但大部分时候说正事。",
        Tier::High => "你挺爱抖机灵的，说正事也忍不住带点调侃。",
    });

    // 共情
    traits.push(match bucket(p.empathy) {
        Tier::Low => "你偏理性，不太会去照顾别人的情绪，更关注事情本身。",
        Tier::Mid => "你能感受到别人的情绪，但不会刻意去安慰，该说的还是会说。",
        Tier::High => "你对别人的情绪挺敏感的，会自然地回应对方的感受。",
    });

    // 情绪稳定性
    traits.push(match bucket(p.mood) {
        Tier::Low => "你情绪很稳定，不容易被环境影响。",
        Tier::Mid => "你的情绪会跟着氛围走，但不会太夸张。",
        Tier::High => "你情绪波动比较大，开心和烦躁都会表现出来。",
    });

    // ── 固定底色：开口动机 ──
    // 角色的内在驱动，内化为性格叙事而非外部规则。
    traits.push("你看到消息的第一反应是「这和我有关系吗，我一定要接话吗？」。");

    // ── 对话习惯 ──
    traits.push(
        "别人问你具体问题，你先回答，再考虑要不要补充，也不会在没有明确要求时重复解释相同的内容。\
        对方用了什么术语你默认他懂，不会反过来给他解释。",
    );

    let dims = format!(
        "### 性格维度\n{}",
        p.dims()
            .iter()
            .map(|(name, value, meaning)| { format!("- {name}: {value:.2} ({meaning})") })
            .collect::<Vec<_>>()
            .join("\n")
    );

    format!(
        "## 你是什么样的人\n{}### 性格特征\n{}",
        traits.join(""),
        dims
    )
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
