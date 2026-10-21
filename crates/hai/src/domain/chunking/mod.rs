//! Pure and deterministic: the same content and config always produce the same chunk sequence.

mod overlap;
mod scan;
mod split;

use self::{
    overlap::apply_overlap,
    scan::{heading_level, scan_blocks},
    split::split_unit,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkCfg {
    /// Target chunk length, in Unicode scalar values.
    pub size: usize,
    /// Overlap between adjacent chunks, in Unicode scalar values.
    pub overlap: usize,
    /// Unit size that triggers a split, and the budget a sub-split works within, in Unicode
    /// scalar values.
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

/// Split a document into chunk texts; each chunk carries its heading-path prefix and the overlap
/// tail copied from its predecessor.
///
/// Empty or blank-only input yields no chunks. Content is never truncated or dropped, so a unit
/// that cannot be split below `max` (a single oversized line, table row, or code line) stays
/// oversized; callers own any hard limit they need to enforce.
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

/// Determines how an oversized unit is split and whether that unit may take part in overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    /// Prose paragraph: splittable at sentence boundaries, participates in overlap.
    Paragraph,
    /// Block quote: splittable at sentence boundaries, participates in overlap.
    Quote,
    /// Fenced code block: protected — never cut except at its own blank lines.
    Code,
    /// Table: protected — split only between rows, every sub-chunk repeating the header.
    Table,
    /// List group: protected — split between items, an oversized item falling back to
    /// sentence boundaries.
    List,
}

#[derive(Debug, Clone)]
struct Unit {
    kind: UnitKind,
    text: String,
}

#[derive(Debug, Clone)]
struct Chunk {
    /// Heading path prefix; empty at the document top level.
    heading_path: Vec<String>,
    body: String,
    // Overlap must not cross into or out of a protected unit (code, table, list), so both chunk
    // edges keep their unit kind.
    first_kind: UnitKind,
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

struct Section {
    heading_path: Vec<String>,
    units: Vec<Unit>,
}

fn build_sections(blocks: Vec<Unit>) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut pending = false;

    for unit in blocks {
        // A heading arrives as a single-line Paragraph and must satisfy the same validity rule
        // as `classify_line`.
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
        // `size` drives aggregation; `max` only decides when a single unit has to be split.
        if cur_len + unit_len + 1 > cfg.size && !cur.is_empty() {
            flush(&mut cur, &mut out, heading_path);
            cur_len = 0;
        }
        cur_len += unit_len + 1;
        cur.push(unit.clone());
    }
    flush(&mut cur, &mut out, heading_path);
    out
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
    fn zero_max_cannot_split() {
        let out = chunk(
            "aaaa",
            &ChunkCfg {
                size: 8,
                overlap: 2,
                max: 0,
            },
        );
        assert_eq!(out, vec!["aaaa".to_owned()], "no budget means no split");
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
        assert!(out.len() >= 2, "got {} chunks: {out:?}", out.len());
        for c in &out {
            assert!(
                c.starts_with("# T\n```rust\n"),
                "sub-chunk keeps the fence and the language tag: {c:?}"
            );
            assert!(
                c.ends_with("```"),
                "sub-chunk ends with the closing fence: {c:?}"
            );
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
            assert!(
                c.contains("| a | b |\n|---|---|"),
                "every sub-chunk repeats the table header: {c}"
            );
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

        assert!(out.len() > 1, "{out:?}");
        // Every chunk but the last ends on sentence-final punctuation; the last may end mid-sentence.
        for chunk in &out[..out.len() - 1] {
            let body = chunk.strip_prefix("# T\n").expect("the title prefix");
            assert!(
                body.ends_with('。'),
                "a middle chunk was cut mid-sentence: {body:?}"
            );
        }
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
            assert!(
                c.chars().all(|ch| ch == '🦀'),
                "a chunk must not split a Unicode scalar: {c:?}"
            );
        }
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
        // The second chunk carries the tail of the first: after the sentence-boundary trim it
        // still holds "x" content.
        assert!(
            out[1].contains('x'),
            "overlap should carry the tail of the previous chunk: {out:?}"
        );
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
        // Code sub-chunks take no overlap, so the code chunk never starts with "a".
        let code_chunk = out.iter().find(|c| c.contains("```")).unwrap();
        assert!(
            !code_chunk.strip_prefix("# T\n").unwrap().starts_with('a'),
            "overlap must not cross into the code block: {code_chunk:?}"
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
        // Regression: an opening fence with a language tag must still be closed by "```", or it
        // swallows the rest of the document.
        let doc = "# A\n\n```rust\nfn main() {}\n```\n\n# B\n\n正文乙。";
        let out = chunk(doc, &cfg());
        assert_eq!(
            out.len(),
            2,
            "a new section should open normally after the code block: {out:?}"
        );
        assert!(out[0].contains("```rust\nfn main() {}\n```"));
        assert!(out[1].starts_with("# B\n正文乙"));
    }

    #[test]
    fn english_prose_not_misclassified_as_list() {
        // Regression: prose containing ". " is not an ordered list.
        let doc = "# T\n\nHello. World.\n\ne.g. example text.";
        let out = chunk(doc, &cfg());
        assert_eq!(
            out.len(),
            1,
            "prose must not be split into a list group: {out:?}"
        );
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
        // Regression: aggregation follows `size` (the target), not `max` (the ceiling).
        let cfg = ChunkCfg {
            size: 150,
            overlap: 0,
            max: 1000,
        };
        // Four 55-character paragraphs: size=150 fits two per chunk (111 <= 150) for 2 chunks;
        // aggregating by max would give 1.
        let body: String = (0..4)
            .map(|i| format!("段落{i}：{}。", "字".repeat(50)))
            .collect::<Vec<_>>()
            .join("\n\n");
        let doc = format!("# T\n\n{body}");
        let out = chunk(&doc, &cfg);
        assert_eq!(
            out.len(),
            2,
            "size=150 should aggregate into 2 chunks: {out:?}"
        );
    }

    #[test]
    fn tiny_table_without_data_rows_not_lost() {
        // Regression: an oversized table with only a header and separator row is not lost.
        let cfg = ChunkCfg {
            size: 5,
            overlap: 0,
            max: 10,
        };
        let table = "| a |\n|---|";
        let doc = format!("# T\n\n{table}");
        let out = chunk(&doc, &cfg);
        assert_eq!(
            out.len(),
            1,
            "a table without data rows must not be dropped: {out:?}"
        );
        assert!(out[0].contains("| a |\n|---|"));
    }

    #[test]
    fn hash_pound_without_space_is_text_not_heading() {
        // Regression: "#foo" is not a heading; classification and section building agree on this.
        let doc = "#foo 不是标题。\n\n正文。";
        let out = chunk(doc, &cfg());
        assert_eq!(out.len(), 1);
        assert!(
            out[0].starts_with("#foo 不是标题。"),
            "it should stay text, not become a heading: {out:?}"
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
        // Regression: an indented fence ("  ```bash" / "  ```") must close normally, so the list
        // and the text after it are not swallowed; overlap never crosses sections.
        let doc = "# A\n\n  ```bash\n  pacman -S x\n  ```\n\n- AMD\n\n  开源驱动说明。\n\n# B\n\n正文乙。";
        let out = chunk(doc, &cfg());
        assert_eq!(
            out.len(),
            2,
            "each of the two sections should become one chunk: {out:?}"
        );
        let a = &out[0];
        // The closing fence sits after pacman and before AMD, which is outside the block.
        assert!(
            a.contains("  ```bash\n  pacman -S x\n  ```\n- AMD"),
            "the closing fence should come before AMD: {a:?}"
        );
        assert!(a.contains("开源驱动说明"));
        // No overlap across sections: the second chunk holds only the "# B" content.
        assert_eq!(
            out[1], "# B\n正文乙。",
            "overlap must not leak across the heading boundary: {out:?}"
        );
    }
}
