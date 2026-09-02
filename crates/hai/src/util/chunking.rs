//! 知识库文档分块：纯函数、确定性——同输入同参数产出相同块序列。

/// 分块参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkCfg {
    /// 目标块长（Unicode 字符）。
    pub size: usize,
    /// 相邻块重叠（字符）。
    pub overlap: usize,
    /// 单块硬上限（字符）。
    pub max: usize,
}

impl Default for ChunkCfg {
    fn default() -> Self {
        Self {
            size: 512,
            overlap: 51,
            max: 1536,
        }
    }
}

/// 文档切块：返回块文本序列（每块 ≤ `max` 字符，含标题路径前缀与 overlap）。
///
/// 空输入返回空 Vec。单块超长（`len > max`）的文档不在此截断——
/// 由调用方（导入/服务层）负责上限决策，本函数只做确定性切分。
pub fn chunk(content: &str, cfg: &ChunkCfg) -> Vec<String> {
    let blocks = scan_blocks(content);
    if blocks.is_empty() {
        return Vec::new();
    }
    let sections = build_sections(blocks);
    let mut out: Vec<Chunk> = Vec::new();
    for sec in sections {
        for piece in chunk_units(&sec.heading_path, &sec.units, cfg) {
            out.push(piece);
        }
    }
    apply_overlap(&mut out, cfg.overlap);
    out.into_iter().map(|c| c.into_text()).collect()
}

// ── 内部结构 ────────────────────────────────────────────────────────────────

/// 结构单元类型。决定超长切分与 overlap 策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    /// 普通段落文本（可参与 overlap，句子边界切）
    Paragraph,
    /// 引用块（`>` 连续行；句子边界切，参与 overlap）
    Quote,
    /// 代码围栏块（整体保护；超长按空行切，子块补围栏）
    Code,
    /// 表格（整体保护；超长按行切，子块重复表头）
    Table,
    /// 列表组（不切断列表项；超长在句子边界切并保留标记）
    List,
}

/// 一个结构单元：类型 + 原始文本。
#[derive(Debug, Clone)]
struct Unit {
    kind: UnitKind,
    text: String,
}

/// 一个块（含标题路径前缀信息）。
#[derive(Debug, Clone)]
struct Chunk {
    /// 标题路径（如 ["# 服务器部署", "## nginx"]），空 = 文档顶层
    heading_path: Vec<String>,
    body: String,
    /// 首个单元类型：overlap 不得跨入受保护单元（代码/表格/列表）
    first_kind: UnitKind,
    /// 最后追加的单元类型：overlap 不得从受保护单元跨出
    last_kind: UnitKind,
}

impl Chunk {
    fn into_text(self) -> String {
        let mut s = String::new();
        for (i, h) in self.heading_path.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(h);
        }
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(&self.body);
        s
    }
}

// ── 扫描：行分类 → 结构单元 ─────────────────────────────────────────────────

/// 行分类结果。
#[derive(Debug, Clone, PartialEq)]
enum LineKind {
    /// 标题（级别 1-6）
    Heading(u8),
    /// 代码围栏起始（含语言标注原文）
    FenceOpen(String),
    /// 表格行
    Table,
    /// 引用行
    Quote,
    /// 列表项
    List,
    /// 普通文本行
    Text,
    /// 空行
    Blank,
}

/// 围栏标记长度（` 或 ~ 连续 ≥3），非围栏返回 None。
fn fence_marker_len(trimmed: &str) -> Option<usize> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let n = trimmed.chars().take_while(|c| *c == first).count();
    (n >= 3).then_some(n)
}

/// 标题级别判定（与 classify_line 共用同一规则）：`#` 后须为空格或行尾，级别 ≤ 6。
fn heading_level(trimmed: &str) -> Option<u8> {
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let after = &trimmed[level..];
    if after.is_empty() || after.starts_with(' ') {
        Some(level as u8)
    } else {
        None
    }
}

fn classify_line(line: &str) -> LineKind {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return LineKind::Blank;
    }
    if fence_marker_len(trimmed).is_some() {
        return LineKind::FenceOpen(trimmed.to_string());
    }
    if let Some(level) = heading_level(trimmed) {
        return LineKind::Heading(level);
    }
    if trimmed.starts_with('|') {
        return LineKind::Table;
    }
    if trimmed.starts_with('>') {
        return LineKind::Quote;
    }
    let list_marker = trimmed
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    let rest = &trimmed[list_marker..];
    // 无序：- / * / + 后接空格；有序：数字后接 `. ` / `) `——必须锚定行首，防散文误判
    let is_unordered = rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ");
    let is_ordered = {
        let after_digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        !after_digits.is_empty()
            && after_digits.len() != rest.len()
            && (after_digits.starts_with(". ") || after_digits.starts_with(") "))
    };
    if is_unordered || is_ordered {
        return LineKind::List;
    }
    LineKind::Text
}

/// 单遍扫描：文本 → 结构单元列表。
fn scan_blocks(content: &str) -> Vec<Unit> {
    let lines: Vec<&str> = content.lines().collect();
    let mut units: Vec<Unit> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        match classify_line(line) {
            LineKind::Blank => i += 1,
            LineKind::Heading(_) => {
                units.push(Unit {
                    kind: UnitKind::Paragraph,
                    text: line.trim_end().to_string(),
                });
                i += 1;
            }
            LineKind::FenceOpen(_) => {
                let open = line;
                let mut body: Vec<&str> = vec![open];
                i += 1;
                let mut closed = false;
                while i < lines.len() {
                    let l = lines[i];
                    body.push(l);
                    if is_fence_close(l, open) {
                        closed = true;
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                let _ = closed; // 未闭合围栏按 EOF 截断（仍视为代码块）
                units.push(Unit {
                    kind: UnitKind::Code,
                    text: body.join("\n"),
                });
            }
            LineKind::Table => {
                let mut body: Vec<&str> = vec![line];
                i += 1;
                while i < lines.len() && classify_line(lines[i]) == LineKind::Table {
                    body.push(lines[i]);
                    i += 1;
                }
                units.push(Unit {
                    kind: UnitKind::Table,
                    text: body.join("\n"),
                });
            }
            LineKind::Quote => {
                let mut body: Vec<&str> = vec![line];
                i += 1;
                while i < lines.len() && classify_line(lines[i]) == LineKind::Quote {
                    body.push(lines[i]);
                    i += 1;
                }
                units.push(Unit {
                    kind: UnitKind::Quote,
                    text: body.join("\n"),
                });
            }
            LineKind::List => {
                let mut body: Vec<&str> = vec![line];
                i += 1;
                while i < lines.len() && classify_line(lines[i]) == LineKind::List {
                    body.push(lines[i]);
                    i += 1;
                }
                units.push(Unit {
                    kind: UnitKind::List,
                    text: body.join("\n"),
                });
            }
            LineKind::Text => {
                let mut body: Vec<&str> = vec![line];
                i += 1;
                while i < lines.len() {
                    match classify_line(lines[i]) {
                        LineKind::Text => body.push(lines[i]),
                        LineKind::Blank => break,
                        _ => break,
                    }
                    i += 1;
                }
                units.push(Unit {
                    kind: UnitKind::Paragraph,
                    text: body.join("\n"),
                });
            }
        }
    }
    units
}

/// 判断行是否为围栏关闭行：允许前导空格（缩进围栏，CommonMark 闭合围栏可缩进），
/// 整行仅由 marker 组成，且长度 ≥ 起始围栏的 marker 数（语言标注不计入）。
fn is_fence_close(line: &str, open: &str) -> bool {
    let trimmed = line.trim_end();
    let content = trimmed.trim_start(); // 容忍缩进闭合（如 "  ```"）
    let open_trim = open.trim_start();
    let Some(marker) = open_trim.chars().next() else {
        return false;
    };
    if marker != '`' && marker != '~' {
        return false;
    }
    let open_len = open_trim.chars().take_while(|c| *c == marker).count();
    let close_len = content.chars().take_while(|c| *c == marker).count();
    close_len >= open_len && content.chars().all(|c| c == marker)
}

// ── 组织：标题树 → section ──────────────────────────────────────────────────

struct Section {
    heading_path: Vec<String>,
    units: Vec<Unit>,
}

/// 把结构单元按标题层级组织成 sections。
fn build_sections(blocks: Vec<Unit>) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut pending = false; // 刚遇到标题：下一个单元开启新 section

    for unit in blocks {
        // 标题行：Paragraph 单行、以 # 开头（与 classify_line 同一合法性规则）
        if unit.kind == UnitKind::Paragraph && !unit.text.contains('\n') {
            let trimmed = unit.text.trim_start();
            if let Some(level) = heading_level(trimmed) {
                while path.len() >= level as usize {
                    path.pop();
                }
                path.push(unit.text.trim().to_string());
                pending = true;
                continue;
            }
        }
        if pending || sections.is_empty() {
            sections.push(Section {
                heading_path: path.clone(),
                units: Vec::new(),
            });
            pending = false;
        }
        sections
            .last_mut()
            .expect("section pushed above")
            .units
            .push(unit);
    }
    sections
}

// ── 成块：section 内单元聚合 ────────────────────────────────────────────────

/// section 内聚合：目标 `size`、硬上限 `max`；超长单元内部切。
fn chunk_units(heading_path: &[String], units: &[Unit], cfg: &ChunkCfg) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::new();
    let mut cur: Vec<Unit> = Vec::new();
    let mut cur_len = 0usize;

    let flush = |cur: &mut Vec<Unit>, out: &mut Vec<Chunk>, path: &[String]| {
        if cur.is_empty() {
            return;
        }
        let first_kind = cur.first().map(|u| u.kind).unwrap_or(UnitKind::Paragraph);
        let last_kind = cur.last().map(|u| u.kind).unwrap_or(UnitKind::Paragraph);
        let body = cur
            .iter()
            .map(|u| u.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        out.push(Chunk {
            heading_path: path.to_vec(),
            body,
            first_kind,
            last_kind,
        });
        cur.clear();
    };

    for unit in units {
        let unit_len = unit.text.chars().count();
        if unit_len > cfg.max {
            // 超长单元：先 flush 当前，再单独切
            flush(&mut cur, &mut out, heading_path);
            cur_len = 0;
            for piece in split_unit(unit, cfg) {
                out.push(Chunk {
                    heading_path: heading_path.to_vec(),
                    body: piece,
                    first_kind: unit.kind,
                    last_kind: unit.kind,
                });
            }
            continue;
        }
        // max 仅作单块硬上限
        if cur_len + unit_len + 1 > cfg.size && !cur.is_empty() {
            flush(&mut cur, &mut out, heading_path);
            cur_len = 0;
        }
        cur_len += unit_len + 1; // +1 换行
        cur.push(unit.clone());
    }
    flush(&mut cur, &mut out, heading_path);
    out
}

/// 超长结构单元的内部切分（仅 `len > cfg.max` 时触发）。
fn split_unit(unit: &Unit, cfg: &ChunkCfg) -> Vec<String> {
    match unit.kind {
        UnitKind::Code => split_code(&unit.text, cfg),
        UnitKind::Table => split_table(&unit.text, cfg),
        UnitKind::List => split_list(&unit.text, cfg),
        UnitKind::Paragraph | UnitKind::Quote => split_text(&unit.text, cfg),
    }
}

/// 代码块：按空行分段，子块补围栏（保留语言标注）。
fn split_code(text: &str, cfg: &ChunkCfg) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let (open, rest) = match lines.first() {
        Some(first) => (first.to_string(), &lines[1..]),
        None => return Vec::new(),
    };
    // 关闭围栏 = 末尾纯围栏行（与起始同标记字符，如 "```" 或 "~~~"）
    let close = lines
        .last()
        .filter(|l| {
            let t = l.trim_end().trim_start(); // 容忍缩进闭合
            let mut it = t.chars();
            match it.next() {
                Some('`') | Some('~') => it.all(|c| c == '`' || c == '~'),
                _ => false,
            }
        })
        .map(|l| l.to_string())
        .unwrap_or_default();
    let inner: Vec<&str> = if close.is_empty() {
        rest.to_vec()
    } else {
        rest[..rest.len().saturating_sub(1)].to_vec()
    };

    let mut pieces: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for &l in &inner {
        if l.trim().is_empty() && !cur.is_empty() {
            pieces.push(std::mem::take(&mut cur));
        } else {
            cur.push(l);
        }
    }
    if !cur.is_empty() {
        pieces.push(cur);
    }
    if pieces.is_empty() {
        pieces.push(inner);
    }

    // 每段包围栏；仍超长则按行再切
    let mut out: Vec<String> = Vec::new();
    for piece in pieces {
        let mut block = String::new();
        block.push_str(&open);
        block.push('\n');
        block.push_str(&piece.join("\n"));
        if !close.is_empty() {
            block.push('\n');
            block.push_str(&close);
        }
        if block.chars().count() <= cfg.max {
            out.push(block);
        } else {
            // 罕见：单段超长，按行二分
            for sub in split_by_line_count(&piece, cfg) {
                let mut b = String::new();
                b.push_str(&open);
                b.push('\n');
                b.push_str(&sub.join("\n"));
                if !close.is_empty() {
                    b.push('\n');
                    b.push_str(&close);
                }
                out.push(b);
            }
        }
    }
    out
}

/// 按行数切分（每段字符 ≤ max 的启发式）。
fn split_by_line_count<'a>(lines: &[&'a str], cfg: &ChunkCfg) -> Vec<Vec<&'a str>> {
    // 均分：每段约 max/2 字符
    let target_chars = cfg.max.saturating_sub(8) / 2;
    let mut out: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut cur_len = 0usize;
    for &l in lines {
        let lc = l.chars().count();
        if cur_len + lc > target_chars && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cur_len = 0;
        }
        cur_len += lc;
        cur.push(l);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 表格：超长按字符预算切，每个子块重复表头 + 分隔行。
/// 仅表头+分隔行（无数据行）时整体一块，不丢数据。
fn split_table(text: &str, cfg: &ChunkCfg) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= 2 {
        return vec![text.to_string()];
    }
    let head = &lines[..2];
    let data = &lines[2..];
    let head_len = head.iter().map(|l| l.chars().count()).sum::<usize>() + 2;
    let budget = cfg.max.saturating_sub(head_len + 2);
    let mut out: Vec<String> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut cur_len = 0usize;
    for row in data {
        let rc = row.chars().count() + 1;
        if cur_len + rc > budget && !cur.is_empty() {
            out.push(build_table_block(head, &cur));
            cur.clear();
            cur_len = 0;
        }
        cur_len += rc;
        cur.push(row);
    }
    if !cur.is_empty() {
        out.push(build_table_block(head, &cur));
    }
    out
}

fn build_table_block(head: &[&str], rows: &[&str]) -> String {
    let mut b = String::new();
    b.push_str(&head.join("\n"));
    b.push('\n');
    b.push_str(&rows.join("\n"));
    b
}

/// 列表：按列表项边界切；超长项在句子边界切并保留标记。
fn split_list(text: &str, cfg: &ChunkCfg) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut cur_len = 0usize;
    for &l in &lines {
        let lc = l.chars().count();
        if cur_len + lc > cfg.max && !cur.is_empty() {
            out.push(cur.join("\n"));
            cur.clear();
            cur_len = 0;
        }
        cur_len += lc + 1;
        cur.push(l);
    }
    if !cur.is_empty() {
        out.push(cur.join("\n"));
    }
    // 仍超长的项：句子边界切，保留标记
    let mut final_out: Vec<String> = Vec::new();
    for piece in out {
        if piece.chars().count() <= cfg.max {
            final_out.push(piece);
        } else {
            for sub in split_text(&piece, cfg) {
                final_out.push(sub);
            }
        }
    }
    final_out
}

/// 文本/引用：贪心按句子边界凑 `max`；无法满足时字符硬切（char 安全）。
fn split_text(text: &str, cfg: &ChunkCfg) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let len = rest.chars().count();
        if len <= cfg.max {
            out.push(rest.to_string());
            break;
        }
        // 在 max 窗口内找最近的句子边界
        let window: Vec<(usize, char)> = rest.char_indices().take(cfg.max).collect();
        let mut cut = None;
        for (idx, ch) in window.iter().rev() {
            if is_sentence_boundary(*ch) {
                cut = Some(idx + ch.len_utf8());
                break;
            }
        }
        let cut = cut.unwrap_or_else(|| {
            // 无句子边界：窗口末尾的字符边界
            window.last().map(|(i, c)| i + c.len_utf8()).unwrap_or(0)
        });
        out.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    out
}

fn is_sentence_boundary(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';' | '\n')
}

// ── overlap ─────────────────────────────────────────────────────────────────

/// 相邻块间补 overlap：取前块正文尾部 `k` 字符（句子边界回退）接到后块正文头部。
/// 代码/表格/列表单元边界跳过（结构单元保护）；**跨 section（标题边界）跳过**——
/// 标题是语义边界，overlap 会引入前节内容。
fn apply_overlap(chunks: &mut [Chunk], k: usize) {
    if k == 0 {
        return;
    }
    for i in 1..chunks.len() {
        if chunks[i].heading_path != chunks[i - 1].heading_path {
            continue; // 跨 section：不 overlap
        }
        let (prev_body, prev_kind) = {
            let prev = &chunks[i - 1];
            (prev.body.clone(), prev.last_kind)
        };
        if !matches!(prev_kind, UnitKind::Paragraph | UnitKind::Quote)
            || !matches!(chunks[i].first_kind, UnitKind::Paragraph | UnitKind::Quote)
        {
            continue;
        }
        // 前块去掉标题前缀后的尾部字符（前缀在 into_text 时拼，body 不含前缀）
        let tail = tail_up_to(&prev_body, k);
        if tail.is_empty() {
            continue;
        }
        chunks[i].body.insert_str(0, &tail);
    }
}

/// 取文本尾部最多 `k` 字符，回退到最近的句子边界（保证不切断句子）。
fn tail_up_to(text: &str, k: usize) -> String {
    let total = text.chars().count();
    if total <= k {
        return text.to_string();
    }
    let skip = total - k;
    let start = text.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0);
    let tail = &text[start..];
    // 回退到句子边界
    let mut cut_at = None;
    for (i, c) in tail.char_indices() {
        if is_sentence_boundary(c) {
            cut_at = Some(i + c.len_utf8());
        }
    }
    match cut_at {
        Some(end) => tail[..end].to_string(),
        None => tail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ChunkCfg {
        ChunkCfg {
            size: 512,
            overlap: 51,
            max: 1536,
        }
    }

    #[test]
    fn empty_doc_yields_no_chunks() {
        assert!(chunk("", &cfg()).is_empty());
        assert!(chunk("   \n\n  ", &cfg()).is_empty());
    }

    #[test]
    fn short_doc_single_chunk() {
        let doc = "# 标题\n\n一段内容。";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], "# 标题\n一段内容。");
    }

    #[test]
    fn heading_split_adds_path_prefix() {
        let doc = "# A\n\naaaa。\n\n# B\n\nbbbb。";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 2);
        assert!(out[0].starts_with("# A\n"));
        assert!(out[1].starts_with("# B\n"));
    }

    #[test]
    fn nested_heading_path() {
        let doc = "# A\n\n## B\n\ncontent。";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with("# A\n## B\ncontent"));
    }

    #[test]
    fn code_fence_preserved() {
        let doc = "# 代码\n\n```rust\nfn main() {}\n```";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("```rust\nfn main() {}\n```"));
    }

    #[test]
    fn oversized_code_split_keeps_fences() {
        let cfg = ChunkCfg {
            size: 30,
            overlap: 0,
            max: 30,
        };
        let code = format!(
            "```rust\n{}\n\n{}\n\n{}",
            "x".repeat(20),
            "y".repeat(20),
            "z".repeat(20)
        );
        let doc = format!("# T\n\n{code}\n```");
        let out = chunk(&doc, &cfg);
        // 每子块都是合法围栏
        assert!(out.len() >= 2, "got {} chunks: {out:?}", out.len());
        for c in &out {
            assert!(
                c.starts_with("# T\n```rust\n"),
                "子块保留围栏+语言标注: {c:?}"
            );
            assert!(c.ends_with("```"), "子块以围栏结束: {c:?}");
        }
    }

    #[test]
    fn oversized_table_repeats_header() {
        let cfg = ChunkCfg {
            size: 20,
            overlap: 0,
            max: 40,
        };
        let table = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |";
        let doc = format!("# T\n\n{table}");
        let out = chunk(&doc, &cfg);
        for c in &out {
            assert!(c.contains("| a | b |\n|---|---|"), "每个子块重复表头: {c}");
        }
    }

    #[test]
    fn text_split_at_sentence_boundary() {
        let cfg = ChunkCfg {
            size: 10,
            overlap: 0,
            max: 12,
        };
        let doc = format!("# T\n\n{}", "一二三四五六七八九十。第二句话。第三句话。");
        let out = chunk(&doc, &cfg);
        for c in &out {
            // 不切断句子：除最后一块外都以句末标点结尾
            let body = c.strip_prefix("# T\n").unwrap();
            if body.ends_with('。')
                || body.ends_with('。')
                    && body != out.last().unwrap().strip_prefix("# T\n").unwrap()
            {
                // ok
            } else if out.last().unwrap() == c {
                // 最后一块允许不完整
            } else {
                panic!("中间块被切断: {body:?}");
            }
        }
        assert!(out.len() > 1);
    }

    #[test]
    fn emoji_not_split_mid_scalar() {
        let cfg = ChunkCfg {
            size: 3,
            overlap: 0,
            max: 5,
        };
        let doc = "🦀🦀🦀🦀🦀";
        let out = chunk(doc, &cfg);
        for c in &out {
            assert!(c.chars().all(|ch| ch == '🦀'), "不拆 Unicode 标量: {c:?}");
        }
    }

    #[test]
    fn deterministic() {
        let doc = "# A\n\n## B\n\n```\ncode\n```\n\n| x | y |\n|---|---|\n| 1 | 2 |\n\n- 列表项一\n- 列表项二";
        let a = chunk(doc, &cfg());
        let b = chunk(doc, &cfg());
        assert_eq!(a, b);
    }

    #[test]
    fn overlap_applied_between_text_chunks() {
        let cfg = ChunkCfg {
            size: 20,
            overlap: 10,
            max: 40,
        };
        let body = format!("{}。{}。", "x".repeat(30), "y".repeat(30));
        let doc = format!("# T\n\n{body}");
        let out = chunk(&doc, &cfg);
        assert!(out.len() > 1);
        // 第二块含前块尾部（overlap 句子边界回退后仍带 x 内容）
        assert!(out[1].contains('x'), "overlap 应把前块尾部带入: {out:?}");
    }

    #[test]
    fn overlap_skipped_across_code_boundary() {
        let cfg = ChunkCfg {
            size: 30,
            overlap: 20,
            max: 60,
        };
        let doc = format!("# T\n\n{}\n\n```\ncode block\n```", "a".repeat(80));
        let out = chunk(&doc, &cfg);
        assert!(out.len() > 1);
        // 代码块子块不参与 overlap（不会以 a 开头）
        let code_chunk = out.iter().find(|c| c.contains("```")).unwrap();
        assert!(
            !code_chunk.strip_prefix("# T\n").unwrap().starts_with('a'),
            "overlap 不得跨入代码块: {code_chunk:?}"
        );
    }

    #[test]
    fn blank_lines_separate_paragraphs_not_units() {
        let doc = "# T\n\n第一段。\n\n第二段。";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("第一段。"));
        assert!(out[0].contains("第二段。"));
    }

    #[test]
    fn list_group_not_split_across_items() {
        let doc = "# T\n\n- 甲\n- 乙\n- 丙";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("- 甲\n- 乙\n- 丙"));
    }

    #[test]
    fn fenced_block_with_language_tag_closes() {
        // 回归：起始围栏带语言标注时，"```" 必须能关闭（否则吞掉后续文档）
        let doc = "# A\n\n```rust\nfn main() {}\n```\n\n# B\n\n正文乙。";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 2, "代码块后应正常开新 section: {out:?}");
        assert!(out[0].contains("```rust\nfn main() {}\n```"));
        assert!(out[1].starts_with("# B\n正文乙"));
    }

    #[test]
    fn english_prose_not_misclassified_as_list() {
        // 回归：含 ". " 的散文不是有序列表
        let doc = "# T\n\nHello. World.\n\ne.g. example text.";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 1, "散文不应被拆成列表组: {out:?}");
        assert!(out[0].contains("Hello. World."));
        assert!(out[0].contains("e.g. example text."));
    }

    #[test]
    fn ordered_list_still_detected() {
        let doc = "# T\n\n1. 第一步\n2. 第二步\n\n3) 三";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("1. 第一步\n2. 第二步"));
    }

    #[test]
    fn size_target_drives_aggregation() {
        // 回归：块聚合按 size（目标块长）而非 max（硬上限）
        let cfg = ChunkCfg {
            size: 150,
            overlap: 0,
            max: 1000,
        };
        // 4 段各 55 字符：size=150 → 两段一块（111 ≤ 150），共 2 块；若按 max 聚合则 1 块
        let body: String = (0..4)
            .map(|i| format!("段落{i}：{}。", "字".repeat(50)))
            .collect::<Vec<_>>()
            .join("\n\n");
        let doc = format!("# T\n\n{body}");
        let out = chunk(&doc, &cfg);
        assert_eq!(out.len(), 2, "应按 size=150 聚合成 2 块: {out:?}");
    }

    #[test]
    fn tiny_table_without_data_rows_not_lost() {
        // 回归：仅表头+分隔行的超长表格不丢数据
        let cfg = ChunkCfg {
            size: 5,
            overlap: 0,
            max: 10,
        };
        let table = "| a |\n|---|";
        let doc = format!("# T\n\n{table}");
        let out = chunk(&doc, &cfg);
        assert_eq!(out.len(), 1, "无数据行的表格不应被切没: {out:?}");
        assert!(out[0].contains("| a |\n|---|"));
    }

    #[test]
    fn hash_pound_without_space_is_text_not_heading() {
        // 回归：#foo 不是标题（classify 与 build_sections 判定一致）
        let doc = "#foo 不是标题。\n\n正文。";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 1);
        assert!(
            out[0].starts_with("#foo 不是标题。"),
            "应作为正文而非标题: {out:?}"
        );
    }

    #[test]
    fn fence_with_three_plus_markers() {
        let doc = "# A\n\n````\ncode\n````\n\n# B\n\n乙。";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("````\ncode\n````"));
        assert!(out[1].starts_with("# B"));
    }

    #[test]
    fn indented_fence_closes_without_swallowing() {
        // 回归：缩进围栏（"  ```bash" / "  ```"）必须正常闭合，
        // 围栏外的列表项/文本不得被吞进代码块；跨 section 不 overlap
        let doc = "# A\n\n  ```bash\n  pacman -S x\n  ```\n\n- AMD\n\n  开源驱动说明。\n\n# B\n\n正文乙。";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 2, "两个 section 应各成一块: {out:?}");
        let a = &out[0];
        // 围栏正确闭合：闭合 ``` 在 pacman 之后、AMD 之前（AMD 在围栏外）
        assert!(
            a.contains("  ```bash\n  pacman -S x\n  ```\n- AMD"),
            "闭合围栏应在 AMD 之前: {a:?}"
        );
        assert!(a.contains("开源驱动说明"));
        // 跨 section 不 overlap：chunk2 只含 # B 内容
        assert_eq!(
            out[1], "# B\n正文乙。",
            "标题边界不应被 overlap 污染: {out:?}"
        );
    }
}
