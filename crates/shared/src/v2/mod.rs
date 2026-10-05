//! BoardDo v2 domain kernel.
//!
//! Six entities everything else hangs on:
//!
//! 1. [`NodeContract`] — intent + typed ports + capabilities + policy
//! 2. [`WorkflowContract`] — executable graph (may nest as a composite node)
//! 3. [`Artifact`] — first-class data that flows between nodes
//! 4. [`RunContext`] — variables, artifacts, permissions, memory for one run
//! 5. [`AgentSpec`] — specialized workflow with tools, memory, capability budget
//! 6. [`Capability`] — what a node/agent is allowed to do
//!
//! Internet research lives beside these: [`web`] (typed tools + policy) and
//! [`research`] (documents, claims, evidence). Agents never get raw HTTP.
//!
//! Phase 1–2 types (`Node`, `Edge`, `WorkflowDefinition`) remain the runtime
//! wire format. v2 contracts describe *intent* and *safety* on top of that
//! graph so SBDO can show high-level nodes while SmartDo stays deterministic.

pub mod agent;
pub mod artifact;
pub mod capability;
pub mod content;
pub mod custom_tool;
pub mod context;
pub mod goal;
pub mod goal_runtime;
pub mod marketing;
pub mod node_contract;
pub mod ports;
pub mod research;
pub mod templates;
pub mod web;
pub mod workflow_contract;

pub use agent::*;
pub use artifact::*;
pub use capability::*;
pub use content::*;
pub use custom_tool::*;
pub use context::*;
pub use goal::*;
pub use goal_runtime::*;
pub use marketing::*;
pub use node_contract::*;
pub use ports::*;
pub use research::*;
pub use templates::*;
pub use web::*;
pub use workflow_contract::*;

#[cfg(test)]
mod tests;
