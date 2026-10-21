use super::{ChunkCfg, Unit, UnitKind};

pub(super) fn split_unit(unit: &Unit, cfg: &ChunkCfg) -> Vec<String> {
    match unit.kind {
        UnitKind::Code => split_code(&unit.text, cfg),
        UnitKind::Table => split_table(&unit.text, cfg),
        UnitKind::List => split_list(&unit.text, cfg),
        UnitKind::Paragraph | UnitKind::Quote => split_text(&unit.text, cfg),
    }
}

/// Split a code block at blank lines; every sub-chunk re-emits the fence, language tag included.
fn split_code(text: &str, cfg: &ChunkCfg) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let (open, rest) = match lines.first() {
        Some(first) => (first.to_string(), &lines[1..]),
        None => return Vec::new(),
    };
    let close = lines
        .last()
        .filter(|l| {
            let t = l.trim_end().trim_start();
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

/// Heuristic line-wise split. A single line longer than the budget is not cut, so a piece can
/// still exceed `max`.
fn split_by_line_count<'a>(lines: &[&'a str], cfg: &ChunkCfg) -> Vec<Vec<&'a str>> {
    // Pieces target half of `max`: each one gets its fences and its join re-added on top.
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

/// Split a table by character budget, repeating the header and separator rows in every sub-chunk.
/// A table holding only those two rows stays whole.
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

/// Split a list group at item boundaries; an item that is still oversized falls back to sentence
/// boundaries with its marker kept.
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

/// Greedy sentence-boundary split up to `max`; when no boundary is in reach it hard-cuts at a
/// character boundary, so no Unicode scalar is split.
fn split_text(text: &str, cfg: &ChunkCfg) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let len = rest.chars().count();
        if len <= cfg.max {
            out.push(rest.to_string());
            break;
        }
        let window: Vec<(usize, char)> = rest.char_indices().take(cfg.max).collect();
        let mut cut = None;
        for (idx, ch) in window.iter().rev() {
            if is_sentence_boundary(*ch) {
                cut = Some(idx + ch.len_utf8());
                break;
            }
        }
        let cut = match cut {
            Some(cut) => cut,
            // An empty window means no budget at all: emitting the remainder is the only progress
            // left.
            None if window.is_empty() => {
                out.push(rest.to_string());
                break;
            }
            None => window.last().map(|(i, c)| i + c.len_utf8()).unwrap_or(0),
        };
        out.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    out
}

pub(super) fn is_sentence_boundary(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';' | '\n')
}
