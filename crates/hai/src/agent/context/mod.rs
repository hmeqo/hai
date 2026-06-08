//! 上下文模块
//!
//! - `builder`：上下文渲染编排（`build_first_round_prompt`, `build_next_round_prompt`）
//! - `types`：共享类型（`ContentParser`, `PreviousRound` 等）
//! - `helpers`：DB 查询辅助函数
//! - `render_context`：`RenderContext` 数据结构与渲染入口
//! - `sections`：从业务数据构建 XML 节点的纯函数集合

pub mod builder;
pub mod helper;
pub mod render_context;
pub mod sections;
pub mod types;

pub use builder::{build_first_round_prompt, build_next_round_prompt};
pub use render_context::RenderContext;
pub use sections::*;
pub use types::{Attachment, AttachmentPerceptionMap, ContentParser, ParsedContent};
