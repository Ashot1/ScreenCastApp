use crate::IntegrationProfileVersion;
use serde::{Deserialize, Serialize};

pub const TODOIST_NAMESPACE: &str = "todoist-api-v1";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoistConfig {}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "presence",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TodoistPresence<T> {
    Missing,
    Null,
    Invalid,
    Value(T),
}
/// An RFC 3339 instant; sub-second precision is reported, not kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoistTime {
    pub utc_seconds: i64,
    pub precision_lost: bool,
}
/// Observed due subfields; dates stay literal (date-only and floating times are never resolved).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoistDue {
    pub date: TodoistPresence<String>,
    pub timezone: TodoistPresence<String>,
    pub is_recurring: TodoistPresence<bool>,
    pub string: TodoistPresence<String>,
    pub lang: TodoistPresence<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoistDeadline {
    pub date: TodoistPresence<String>,
    pub lang: TodoistPresence<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoistDuration {
    pub amount: TodoistPresence<i64>,
    pub unit: TodoistPresence<String>,
}
/// The selected task fields; boxed in `TodoistFields` because they far outweigh the others.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoistTask {
    pub user_id: TodoistPresence<String>,
    pub project_id: String,
    pub content: TodoistPresence<String>,
    pub description: TodoistPresence<String>,
    pub parent_id: TodoistPresence<String>,
    pub section_id: TodoistPresence<String>,
    pub added_by_uid: TodoistPresence<String>,
    pub assigned_by_uid: TodoistPresence<String>,
    pub responsible_uid: TodoistPresence<String>,
    pub completed_by_uid: TodoistPresence<String>,
    pub checked: TodoistPresence<bool>,
    pub is_deleted: TodoistPresence<bool>,
    pub added_at: TodoistPresence<TodoistTime>,
    pub updated_at: TodoistPresence<TodoistTime>,
    pub completed_at: TodoistPresence<TodoistTime>,
    pub due: TodoistPresence<TodoistDue>,
    pub deadline: TodoistPresence<TodoistDeadline>,
    pub duration: TodoistPresence<TodoistDuration>,
}
/// The selected fields of one object; the variant names the object kind in its SourceKey.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TodoistFields {
    Task(Box<TodoistTask>),
    Project {
        name: TodoistPresence<String>,
        description: TodoistPresence<String>,
        parent_id: TodoistPresence<String>,
        workspace_id: TodoistPresence<String>,
        is_archived: TodoistPresence<bool>,
        is_deleted: TodoistPresence<bool>,
        created_at: TodoistPresence<TodoistTime>,
        updated_at: TodoistPresence<TodoistTime>,
    },
    Comment {
        content: TodoistPresence<String>,
        posted_uid: TodoistPresence<String>,
        posted_at: TodoistPresence<TodoistTime>,
        is_deleted: TodoistPresence<bool>,
        file_attachment: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoistContent {
    pub parent_task_id: String,
    pub object_id: String,
    pub fields: TodoistFields,
    pub losses: Vec<String>,
}
/// Category 0 task, 1 project, 2 comments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoistCoverage {
    pub category: u32,
    pub status: String,
    pub pages: u32,
    pub objects: u32,
    pub unavailable: u32,
    pub reasons: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoistMetadata {
    pub profile_id: u64,
    pub settings_revision: u64,
    pub credential_generation: u64,
    pub account: String,
    pub interval_start: i64,
    pub interval_end: i64,
    pub coverage: Vec<TodoistCoverage>,
    pub observation: String,
    pub current_state: String,
    pub content: TodoistContent,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TodoistRequest {
    User,
    Task {
        id: String,
    },
    Project {
        id: String,
    },
    Comments {
        task_id: String,
        cursor: Option<String>,
    },
}
#[derive(Clone)]
pub(crate) struct TodoistTarget {
    pub selection: String,
}
/// The profile identity a Todoist reader stamps into every object's metadata.
pub(crate) struct TodoistScope {
    pub profile: IntegrationProfileVersion,
    pub account: String,
}
