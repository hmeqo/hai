//! The instruction the digest pass reads: what its report covers, over which window, and what it
//! holds.
use super::copy::covered_days;
use crate::{
    agent::prompt::text::{LanguageText, PerCatalog},
    config::conversation::Language,
    domain::vo::Stretch,
};

/// The instruction the pass reads: what its report covers, over which window, what it holds, and the
/// language it is written in.
pub(crate) fn task(language: Language, stretch: Stretch) -> String {
    let (from, to) = covered_days(stretch);
    let name = language.names().get(language.text_catalog()).as_str();
    (TASK.get(language.text_catalog()))(&from, &to, name)
}

const TASK: PerCatalog<fn(&str, &str, &str) -> String> = PerCatalog {
    zh: task_zh,
    en: task_en,
};

fn task_zh(from: &str, to: &str, language: &str) -> String {
    format!(
        "写一份客观的报告，覆盖 {from}–{to} 这段时间聊过的、值得注意的内容。\
         报告用{language}写。\
         先用 recall（since={from}，until={to}）列出这段时间的话题与记忆，\
         需要细节时用 read 打开 topic://<id> 或 memory://<id> 看全文，不凭印象写。\
         输出格式：① 首行「{from}–{to}」+ 一句总体情况；\
         ② 主体：按话题分组逐条列要点，每条写清三件事——\
         时间（能到具体时刻更好）、\
         内容（关键结论、决定、数字或原话要点，不用空泛概括代替）、\
         为什么值得注意（进展、变化、待办、悬而未决）；\
         ③ 结尾：有还没下文的事项时，单列「未决事项」，写清谁在等谁、等什么；没有就不写。\
         语言直白完整：不省略主语，不用「那事」「那个」这类读者没见过的指代，\
         不写需要猜的省略句。"
    )
}

fn task_en(from: &str, to: &str, language: &str) -> String {
    format!(
        "Write an objective report on what was worth noting between {from} and {to}. \
         Write the report in {language}. \
         Start by using recall (since={from}, until={to}) to list the topics and memories of that \
         range, and open topic://<id> or memory://<id> with read when you need the detail — do not \
         write from impression. \
         Format: (1) first line, \"{from}–{to}\" plus one sentence on the overall state; \
         (2) body: group by topic, one bullet per point, each stating three things — \
         when (the time, and the moment itself when it matters), \
         what (conclusions, decisions, numbers or the gist of what was said, never a vague \
         paraphrase), \
         and why it is worth noting (progress, change, a to-do, something unresolved); \
         (3) closing: when something is still open, a separate \"Open items\" list saying who is \
         waiting on whom and for what; leave it out when nothing is. \
         Write plainly and completely: keep the subject in, never \"that thing\" or \"the other one\" \
         the reader has not seen, and no elliptical sentence that has to be guessed."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The task names the language the report is written in, in that language's own name, so a pass
    /// that runs beside a Chinese conversation does not drift back into it.
    #[test]
    fn the_task_names_the_language_of_the_report() {
        let week = Stretch::Last { secs: 7 * 86_400 };

        assert!(
            task(Language::Zh, week).contains("报告用中文写"),
            "{}",
            task(Language::Zh, week)
        );
        assert!(
            task(Language::Ja, week).contains("Write the report in Japanese"),
            "{}",
            task(Language::Ja, week)
        );
        assert!(
            task(Language::EnGb, week).contains("Write the report in British English"),
            "{}",
            task(Language::EnGb, week)
        );
    }
}
