pub mod agent_environment;
pub mod api;
pub mod auth;
pub mod config;

mod domain_events;
pub mod error;
pub mod events;
mod main_lib;
pub mod mcp;
pub mod models;
pub mod oidc;

mod secrets;
pub mod static_files;

pub use agent_environment::ServerAgentEnvironment;
pub use main_lib::{
    build_state, init_tracing, run_database_maintenance, run_profile_database_maintenance, AppState,
};

pub mod profiles;
