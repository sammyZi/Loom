pub mod agents;
pub mod project;
pub mod provider;
pub mod settings;
mod compact;
mod context;
mod loop_;
mod tools;

pub use loop_::{run_agent, RunEnv};
pub use provider::{
    groups_json as model_groups, groups_json_live as model_groups_live, normalize_effort,
    normalize_model, provider_def, Message, DEFAULT_MODEL,
};
pub use settings::Settings;
pub use tools::{PermGate, SubagentRunner, ToolCtx, ToolRegistry};
