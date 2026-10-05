//! Scenario runtime supervisor — long-lived armed instances that wait for events.
//!
//! Scenario (graph) ≠ Runtime (armed loop) ≠ Execution (one DAG run) ≠ Event (cause).
//! Graph-level `logic.loop` is out of scope.

mod events;
mod lifecycle;
mod registry;

pub use events::{ExecutionFinished, RuntimeEvent, RuntimeEventSource};
pub use registry::{RuntimeSupervisor, TelegramAccountDemand};
