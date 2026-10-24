//! Runtime-neutral agent tool catalog.
//!
//! This crate owns the tool definitions shared by every agent surface:
//! the in-app AI assistant (via a rig adapter in `wealthfolio-ai`) and the
//! MCP server (`wealthfolio-mcp`). Tools execute against an abstract
//! [`AgentEnvironment`] and never depend on a runtime shell (Tauri/Axum),
//! an LLM orchestration library, or app authentication.
//!
//! Scope enforcement happens here, at the tool boundary
//! ([`catalog::AgentToolCatalog::execute`]), so no transport can reach a
//! tool the caller's scopes don't grant.

pub mod catalog;
pub mod constants;
pub mod context;
pub mod env;
pub mod scope;
pub mod tool;
pub mod tools;

pub use catalog::{
    mcp_write_blocked, mcp_write_blocked_for, AgentToolCatalog, McpHost,
    DESKTOP_FORMAL_WRITE_TOOLS, MCP_HIDDEN_TOOLS,
};
pub use context::{CallActorKind, ToolCallContext};
pub use env::AgentEnvironment;
pub use scope::{AgentScope, AgentScopeSet};
pub use tool::{AgentTool, AgentToolAccess, AgentToolError, AgentToolResult};
