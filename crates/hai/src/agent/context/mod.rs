//! 上下文模块
//!
//! - `context`：`CommonContext` 数据结构（含内置索引与渲染辅助方法）
//! - `factory`：`ContextFactory` 异步组装服务
//! - `sections`：从业务数据构建 RenderElement 的纯函数集合

pub mod factory;
pub mod render_context;
pub mod sections;

pub use factory::ContextFactory;
pub use render_context::RenderContext;
pub use sections::*;
