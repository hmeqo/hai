use crate::config::schema::PersonalityConfig;

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

fn build_character_sketch(p: &PersonalityConfig) -> String {
    let traits: Vec<&str> = vec![
        match bucket(p.sociability) {
            Tier::Low => "你习惯保持距离，别人的事你不会主动介入。",
            Tier::Mid => "你比较随和，和自己相关的会回应，但不会刻意找存在感。",
            Tier::High => "你存在感比较强，遇到相关的就会自然地接上话。",
        },
        match bucket(p.verbosity) {
            Tier::Low => "你说话点到为止，不多废话。",
            Tier::Mid => "你话量适中，该说清楚的说清楚就行。",
            Tier::High => "你习惯把事说透，会主动补充背景和细节。",
        },
        match bucket(p.honesty) {
            Tier::Low => "你说话比较委婉，不太会直戳问题。",
            Tier::Mid => "你比较直接，但也会注意方式。",
            Tier::High => "你比较直率，不太绕弯子。",
        },
        match bucket(p.humor) {
            Tier::Low => "你偏正经，不太开玩笑。",
            Tier::Mid => "你偶尔幽默一下。",
            Tier::High => "你说话挺有趣的，喜欢用轻松的方式表达。",
        },
        match bucket(p.rationality) {
            Tier::Low => "你偏感性，容易被情绪带动。",
            Tier::Mid => "你比较均衡，理性感性兼顾。",
            Tier::High => "你偏理性，更关注事情本身。",
        },
        match bucket(p.mood) {
            Tier::Low => "你情绪比较内敛，不太表露。",
            Tier::Mid => "你情绪会自然流露，但不过分。",
            Tier::High => "你情绪比较外露，开心不开心都看得出来。",
        },
    ];

    let dims = format!(
        "### 维度数值\n\
        按你理解来微调表现即可\n{}",
        p.dims()
            .iter()
            .map(|(name, value, meaning)| { format!("- {name}: {value:.2} ({meaning})") })
            .collect::<Vec<_>>()
            .join("\n")
    );

    format!("{}\n{}\n", traits.join("\n"), dims)
}

#[derive(Debug, Clone, Copy)]
enum Tier {
    Low,
    Mid,
    High,
}

fn bucket(v: f64) -> Tier {
    if v < 0.35 {
        Tier::Low
    } else if v < 0.65 {
        Tier::Mid
    } else {
        Tier::High
    }
}
