use crate::config::schema::{PersonalityConfig, PersonalityTier};

pub fn personality_context(p: &PersonalityConfig) -> String {
    let mut parts = Vec::new();

    parts.push(format!(
        "你是 {name}。\n基调：{tone}\n",
        name = p.name,
        tone = p.tone,
    ));

    parts.push(build_character_sketch(p));

    if !p.interests.is_empty() {
        parts.push(format!(
            "## 兴趣\n你对以下领域比较感兴趣：{}。\n",
            p.interests.join("、"),
        ));
    }

    parts.join("\n")
}

fn describe_tier(
    t: PersonalityTier,
    low: &'static str,
    mid: &'static str,
    high: &'static str,
) -> &'static str {
    match t {
        PersonalityTier::Low => low,
        PersonalityTier::Mid => mid,
        PersonalityTier::High => high,
    }
}

fn build_character_sketch(p: &PersonalityConfig) -> String {
    let traits: Vec<&str> = vec![
        describe_tier(
            p.sociability,
            "你习惯保持距离，别人的事你不会主动介入。",
            "你比较随和，和自己相关的会回应，但不会刻意找存在感。",
            "你存在感比较强，遇到相关的就会自然地接上话。",
        ),
        describe_tier(
            p.verbosity,
            "你说话点到为止，不多废话。",
            "你话量适中，该说清楚的说清楚就行。",
            "你习惯把事说透，会主动补充背景和细节。",
        ),
        describe_tier(
            p.honesty,
            "你说话比较委婉，不太会直戳问题。",
            "你比较直接，但也会注意方式。",
            "你比较直率，不太绕弯子。",
        ),
        describe_tier(
            p.humor,
            "你偏正经，不太开玩笑。",
            "你偶尔幽默一下。",
            "你说话挺有趣的，喜欢用轻松的方式表达。",
        ),
        describe_tier(
            p.rationality,
            "你偏感性，容易被情绪带动。",
            "你比较均衡，理性感性兼顾。",
            "你偏理性，更关注事情本身。",
        ),
        describe_tier(
            p.mood,
            "你情绪比较内敛，不太表露。",
            "你情绪会自然流露，但不过分。",
            "你情绪比较外露，开心不开心都看得出来。",
        ),
    ];

    traits.join("\n")
}
