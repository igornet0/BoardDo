//! Content Engine — channel adapters, publishing, tools (Goal Runtime application layer).

mod adapters;
mod publishing;
pub mod tools;

pub use adapters::adapter_for;
pub use publishing::{prepare_publication, publish_content};
