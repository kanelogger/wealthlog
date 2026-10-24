// context/mod.rs
mod agent_environment;
mod providers;
mod registry;

pub use providers::initialize_context;
// ContextInitResult is used internally but not exported
pub use registry::ServiceContext;
