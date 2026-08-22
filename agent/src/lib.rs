pub mod project;
mod compact;
mod context;
mod deepseek;
mod loop_;
mod tools;

pub use deepseek::{catalog as model_catalog, normalize_effort, normalize_model, Message};
pub use loop_::run_agent;
pub use tools::{PermGate, ToolCtx, ToolRegistry};
