//! 组件模块
//!
//! 从业务数据构建 RenderElement 的组件，包含上下文和预制组件

pub mod context;
pub mod sections;

pub use context::{AccountInfo, RenderContext};
pub use sections::*;
