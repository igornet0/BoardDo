//! Workflow helpers shared by REST API and Goal Runtime.

pub mod agent_ops;

pub use agent_ops::{
    apply_ops, create_workflow, definition_from_plan, propose_stub_plan, snapshot_from_record,
    stub_scenario_graph, update_workflow, validate_ops, validate_record,
};
