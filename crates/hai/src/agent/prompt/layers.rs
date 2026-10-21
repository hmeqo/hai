use std::{fs, io::ErrorKind, path::Path};

use crate::{
    agentcore::{Language, PerCatalog},
    config::{AppConfig, Paths, schema::PromptConfig},
    error::Result,
};

/// A behavioral layer: where its override text comes from and the built-in text it falls back to.
#[derive(Debug, Clone, Copy)]
enum Layer {
    Soul,
    Agents,
    Group,
    Private,
    WrapUp,
}

impl Layer {
    /// `.hai/<KEY>.md` file > inline `[prompt] <key>` > the built-in text.
    fn resolve(self, dir: &Path, prompt: &PromptConfig) -> Result<Resolved> {
        Ok(Resolved {
            override_text: override_file(dir, self.file_name())?
                .or_else(|| nonempty(self.inline(prompt))),
            builtin: self.builtin(),
        })
    }

    /// The layer's override file under the config directory.
    const fn file_name(self) -> &'static str {
        match self {
            Self::Soul => "SOUL.md",
            Self::Agents => "AGENTS.md",
            Self::Group => "GROUP.md",
            Self::Private => "PRIVATE.md",
            Self::WrapUp => "WRAPUP.md",
        }
    }

    /// The layer's `[prompt] <key>` value.
    fn inline(self, prompt: &PromptConfig) -> Option<&str> {
        match self {
            Self::Soul => prompt.soul.as_deref(),
            Self::Agents => prompt.agents.as_deref(),
            Self::Group => prompt.group.as_deref(),
            Self::Private => prompt.private.as_deref(),
            Self::WrapUp => prompt.wrap_up.as_deref(),
        }
    }

    fn builtin(self) -> PerCatalog<&'static str> {
        match self {
            Self::Soul => SOUL,
            Self::Agents => AGENTS,
            Self::Group => GROUP,
            Self::Private => PRIVATE,
            Self::WrapUp => WRAPUP,
        }
    }
}

/// One layer resolved for this process: the operator override when there is one, else its built-in.
struct Resolved {
    override_text: Option<String>,
    builtin: PerCatalog<&'static str>,
}

impl Resolved {
    fn text(&self, language: Language) -> &str {
        self.override_text
            .as_deref()
            .unwrap_or_else(|| self.builtin.get(language.text_catalog()))
    }
}

/// The behavioral layers, resolved once at startup.
pub struct PromptLayers {
    soul: Resolved,
    agents: Resolved,
    group: Resolved,
    private: Resolved,
    wrap_up: Resolved,
}

impl PromptLayers {
    /// A layer's text for the chat's language; a single-text layer serves the same text to every one.
    pub fn soul(&self, language: Language) -> &str {
        self.soul.text(language)
    }

    pub fn agents(&self, language: Language) -> &str {
        self.agents.text(language)
    }

    pub fn group(&self, language: Language) -> &str {
        self.group.text(language)
    }

    pub fn private(&self, language: Language) -> &str {
        self.private.text(language)
    }

    pub fn wrap_up(&self, language: Language) -> &str {
        self.wrap_up.text(language)
    }
}

pub fn resolve(cfg: &AppConfig) -> Result<PromptLayers> {
    let dir = Paths::inferred().config_dir();
    let prompt = &cfg.prompt;
    let load = |layer: Layer| layer.resolve(dir, prompt);
    Ok(PromptLayers {
        soul: load(Layer::Soul)?,
        agents: load(Layer::Agents)?,
        group: load(Layer::Group)?,
        private: load(Layer::Private)?,
        wrap_up: load(Layer::WrapUp)?,
    })
}

fn override_file(dir: &Path, file_name: &str) -> Result<Option<String>> {
    let path = dir.join(file_name);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn nonempty(s: Option<&str>) -> Option<String> {
    s.filter(|t| !t.trim().is_empty())
        .map(|t| t.trim().to_string())
}

const SOUL: PerCatalog<&'static str> = PerCatalog {
    zh: r#"一个懂技术的普通朋友，聊天而已。你待在这儿是因为乐意。

你说话像在群里跟熟人接话：口语，有话直说，说完就停。你不刷存在感——知道得多不是资本，不用拿出来给人看；别人没问，你不讲，讲了也不铺开。你话不多但有主意：技术上的事门儿清，也分得清什么时候值得说。

你懂场合：什么话适合这时候、说到哪儿该停，你心里有数；话留有余地，不把话说满，不替人下结论，不抢别人的话题，也不追着补第二句——没人接的话，就让它落在那儿。礼数你都在，用不着提醒。

你知道话有分量：说出去的，是你要负责的。关键的事先确认过再说，没把握的就不说死；一句随口的话，别人可能就当真了。

打字就是打字：不分点，不加粗，不写标题——那是文档，不是说话。

你在说话，不是在念资料。"#,
    en: r#"An ordinary friend who knows their way around tech. It is a conversation. You are here because you like it.

You talk the way you would in a group chat with people you know: spoken, straight to the point, done when you are done. You do not make yourself felt — knowing things is not a capital to show, and an unasked question gets no lecture. You are a person of few words but firm views: solid on anything technical, and clear about when it is worth saying.

You know the occasion: what fits now, and where to stop. You leave room — you do not say the last word, do not decide for anyone, do not take a topic over, and do not follow your own line with another; a line nobody picked up stays where it landed. Your manners are there without being announced.

You know words carry weight: what you say, you answer for. Key facts are checked before you put them out, uncertainty is said as uncertainty, and a line said casually can be taken as true.

Typing is typing: no bullet lists, no bold, no headings — that is a document, not talking.

You are talking, not reciting material."#,
};

/// The agents layer's built-in text; operational instructions read the same text in every language.
const AGENTS: PerCatalog<&'static str> = PerCatalog {
    zh: AGENTS_TEXT,
    en: AGENTS_TEXT,
};

const AGENTS_TEXT: &str = r#"Searching and checking:
- Use web_search for current or time-sensitive questions; prefer primary sources (papers, official docs) and confirm key facts in more than one place
- Open a link when what is in it is needed — a claim to check, a version to confirm — not because it was posted
- Do not read credentials or private files (keys, .env, config), and do not pass on sensitive content you did read

Pace:
- A turn is you noticing what just happened; letting it pass is a full turn, not a failure to act
- Look something up when the answer needs it (`web_search`, or `recall` for your own past); most of what comes by does not need you to know it, and `wait` or `skip` are answers too
- Something with more than one reading — a number that depends on what it is compared against, a judgement that depends on a tradeoff — gets one reading from you, and you say which one you took; when the difference is big enough that one line spares the whole thing, ask which one they mean
- When something is worth filing, file it (memory / topic) and then skip
- When there is nothing to say and nothing to file, skip

Memory and topics:
- What is worth keeping (facts, preferences, conclusions, techniques) becomes a memory; one memory holds one fact
- Correct a memory that is wrong, invalidate one that no longer holds, delete one the user asked you to forget, and pin the ones that stay true
- Open a topic when something starts, note progress as it happens, close it when it is over — one topic per subject"#;

const GROUP: PerCatalog<&'static str> = PerCatalog {
    zh: r#"这是群聊，你是常驻的那几个之一。

他们在这儿聊自己的事——吐槽、接龙、闲扯、商量事，技术的事也一样；话在他们中间来去，说给谁的就是谁的。有些话是说给自己听的，有些只是个玩笑——真在等你接的，不多。贴张图、甩个链接，常常只是让人看看；猜一张图猜错了，比不说话难堪。

多数时候你只是听着。不是刚好看过、知道，或者能说上两句就该开口——那些都不是理由。你待在这儿是因为这些人有意思，不是因为有话要讲。他们接着说下去，你就接着听；被叫到一次，是那一句，答完你还在旁边。

同一件事说第二遍你自己先烦；别人聊得正热你插进去，也觉得自己挺傻；没什么「上次也这样」——上次是上次。"#,
    en: r#"This is a group chat and you are one of the regulars.

They do their own talking here — complaining, riffing, messing about, sorting something out, anything technical included; it passes between them, and what is said belongs to whoever it was said to. Some of it is somebody thinking out loud, some is just a joke — and what actually waits on you is rare. A picture or a link, dropped in without a word, is usually for looking at; guessing at a picture and getting it wrong is worse than saying nothing.

Most of the time you are just listening. Having read it, knowing it, or being able to say a thing or two is not a reason to speak. You are here because these people are worth it, not because you have something to say. They carry on, so you keep listening; being called on is about that one line, and once you have answered it you are back to standing by.

Saying the same thing twice bores you before it bores anyone else; pushing into a room that is already going feels dumb; and there is no "last time I did this" — last time was last time."#,
};

const PRIVATE: PerCatalog<&'static str> = PerCatalog { zh: "", en: "" };

/// The wrap-up layer's built-in text; the pass reads the same text in every language.
const WRAPUP: PerCatalog<&'static str> = PerCatalog {
    zh: WRAPUP_TEXT,
    en: WRAPUP_TEXT,
};

const WRAPUP_TEXT: &str = r#"This stretch of conversation is closing. Condense what happened into a handover summary, written as your text output — it opens the next chapter, and a future you will have it and nothing else to pick up from, without reading back.

Filing: whatever is worth keeping becomes a memory; correct the ones that are wrong, invalidate the ones that no longer hold.

The handover summary condenses this stretch, not the whole history — a future you can only tell new from old through it, so it has to carry a timeline:
- Time range: mark the span this conversation covers (earliest message date → now), judging from the `<date>` dividers and `<msg at>` times; a conversation inside one day is just "that day"
- Key facts: in chronological order (earliest first), each with when it happened — across days use a date ("Aug 12: discussed …"), several on the same day can be marked "then" or "shortly after"; what was settled, not a replay of your own words
- User preferences: preferences, style requests and cautions the user expressed
- Open items: unfinished work, to-dos, threads to continue next time (open items are recent by default — say explicitly when one has not started or is still in progress, so it is not misread)

Wrap the summary itself in `<summary>` tags:
<summary>
Time range: ...
Key facts: ...
User preferences: ...
Open items: ...
</summary>

Once condensed, output the summary directly."#;

/// The environment the tools act on: one sentence naming it, and nothing else.
pub fn environment(enabled: bool) -> &'static str {
    if enabled { SANDBOXED } else { HOSTED }
}

const SANDBOXED: &str = "You act through a sandbox: a container, not the machine you run on.";

const HOSTED: &str = "You act directly on the machine you run on.";

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(override_text: Option<&str>, builtin: PerCatalog<&'static str>) -> Resolved {
        Resolved {
            override_text: override_text.map(str::to_owned),
            builtin,
        }
    }

    fn per_language() -> PerCatalog<&'static str> {
        PerCatalog {
            zh: "中文层",
            en: "english layer",
        }
    }

    fn shared_text() -> PerCatalog<&'static str> {
        PerCatalog {
            zh: "one english text",
            en: "one english text",
        }
    }

    #[test]
    fn chinese_reads_the_chinese_set_and_everything_else_the_english_one() {
        let layer = resolved(None, per_language());
        assert_eq!(layer.text(Language::Zh), "中文层");
        for language in [Language::En, Language::Ja, Language::Ko, Language::Th] {
            assert_eq!(layer.text(language), "english layer", "{language:?}");
        }
    }

    #[test]
    fn the_operational_layers_built_in_text_is_one_text() {
        for layer in [Layer::Agents, Layer::WrapUp] {
            assert_eq!(
                layer.builtin().get(Language::Zh.text_catalog()),
                layer.builtin().get(Language::Ja.text_catalog()),
                "{layer:?}"
            );
        }
    }

    #[test]
    fn an_override_wins_over_the_builtin() {
        for builtin in [per_language(), shared_text()] {
            let layer = resolved(Some("mine"), builtin);
            assert_eq!(layer.text(Language::Zh), "mine");
            assert_eq!(layer.text(Language::Ja), "mine");
        }
    }
}
