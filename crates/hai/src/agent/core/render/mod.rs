//! 渲染模块
//!
//! 通用渲染层：把 RenderElement 渲染为不同格式（XML/JSON/MD）

pub mod content;
pub mod elements;
pub mod renderer;

pub use content::*;
pub use elements::*;
pub use renderer::*;
