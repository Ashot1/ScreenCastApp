//! Portable, bounded task and source memory. Rust is its only owner.
mod assistance_types;
mod check_types;
mod memory_store;
mod memory_types;
mod observation_types;
pub use assistance_types::*;
pub use check_types::*;
pub use memory_store::Store;
pub use memory_types::*;
pub use observation_types::*;

mod local_git;
mod local_source_types;
pub use local_git::parse_local_git;
pub use local_source_types::*;
mod codex_parser;
mod codex_types;
pub use codex_parser::{parse_codex_header, parse_codex_window};
pub use codex_types::*;

mod github_adapter;
mod github_parser;
mod github_types;
use github_adapter::GitHub;
pub use github_types::*;

mod integration_types;
pub use integration_types::*;
mod jira_adapter;
mod jira_parser;
mod jira_types;
use jira_adapter::Jira;
pub use jira_types::*;
mod todoist_adapter;
mod todoist_parser;
mod todoist_types;
use todoist_adapter::Todoist;
pub use todoist_types::*;

mod plugin_package;
mod plugin_types;
pub use plugin_package::{canonical_tool, discovery_json, review_plugin};
pub use plugin_types::*;
