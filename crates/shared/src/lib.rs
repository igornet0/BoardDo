//! BoardDo shared domain contract.
//!
//! This crate defines the types exchanged between SmartDo (engine) and SBDO (canvas).
//! It must stay free of business logic and I/O.
//!
//! - Phase 1–2: [`node`], [`workflow`], [`execution`], [`connection`], [`api`]
//! - Phase 3+: [`v2`] kernel — Node/Workflow contracts, Artifact, Context, Agent, Capability

pub mod agent_editor;
pub mod api;
pub mod channel;
pub mod connection;
pub mod execution;
pub mod node;
pub mod runtime;
pub mod v2;
pub mod workflow;

pub use agent_editor::*;
pub use channel::*;
pub use connection::*;
pub use execution::*;
pub use node::*;
pub use runtime::*;
pub use workflow::*;
