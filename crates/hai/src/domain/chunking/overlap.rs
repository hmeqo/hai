use super::{Chunk, UnitKind, split::is_sentence_boundary};

/// Copy up to `k` characters of the previous chunk's tail into the head of the next one.
///
/// Both edges must be unprotected units (code, table, and list are structural boundaries), and the
/// two chunks must share a heading path: a heading is a semantic boundary, so overlapping across it
/// would pull the previous section's text in.
pub(super) fn apply_overlap(chunks: &mut [Chunk], k: usize) {
    if k == 0 {
        return;
    }
    for i in 1..chunks.len() {
        if chunks[i].heading_path != chunks[i - 1].heading_path {
            continue;
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
        let tail = tail_up_to(&prev_body, k);
        if tail.is_empty() {
            continue;
        }
        chunks[i].body.insert_str(0, &tail);
    }
}

/// Up to `k` characters from the end of `text`, trimmed back to the last sentence boundary inside
/// that window so no sentence is cut.
fn tail_up_to(text: &str, k: usize) -> String {
    let total = text.chars().count();
    if total <= k {
        return text.to_string();
    }
    let skip = total - k;
    let start = text.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0);
    let tail = &text[start..];
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
