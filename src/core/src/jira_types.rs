use crate::IntegrationProfileVersion;
use serde::{Deserialize, Serialize};

pub const JIRA_NAMESPACE: &str = "jira-cloud-rest-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "presence",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum JiraPresence<T> {
    Missing,
    Null,
    Invalid,
    Value(T),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraDate {
    pub utc_seconds: i64,
    pub precision_lost: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraNamed {
    pub id: JiraPresence<String>,
    pub name: JiraPresence<String>,
    pub key: JiraPresence<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraAuthor {
    pub account_id: JiraPresence<String>,
    pub display_name: JiraPresence<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraBody {
    pub text: JiraPresence<String>,
    pub losses: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraChange {
    pub field: JiraPresence<String>,
    pub field_id: JiraPresence<String>,
    pub field_type: JiraPresence<String>,
    pub from: JiraPresence<String>,
    pub to: JiraPresence<String>,
    pub from_string: JiraPresence<String>,
    pub to_string: JiraPresence<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JiraFields {
    Issue {
        summary: JiraPresence<String>,
        description: JiraBody,
        status: JiraPresence<JiraNamed>,
        issue_type: JiraPresence<JiraNamed>,
        project: JiraPresence<JiraNamed>,
        created: JiraPresence<JiraDate>,
        updated: JiraPresence<JiraDate>,
    },
    Comment {
        body: JiraBody,
        author: JiraPresence<JiraAuthor>,
        created: JiraPresence<JiraDate>,
        updated: JiraPresence<JiraDate>,
    },
    Changelog {
        author: JiraPresence<JiraAuthor>,
        created: JiraPresence<JiraDate>,
        items: Vec<JiraChange>,
    },
    Relation {
        target_id: String,
        target_key: JiraPresence<String>,
        summary: JiraPresence<String>,
        status: JiraPresence<JiraNamed>,
        direction: String,
        label: String,
        link_type: JiraPresence<JiraNamed>,
        inward: JiraPresence<String>,
        outward: JiraPresence<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraContent {
    pub tag: String,
    pub parent_issue_id: String,
    pub object_id: String,
    pub observed_issue_key: String,
    pub fields: JiraFields,
    pub losses: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraCoverage {
    pub category: u32,
    pub status: String,
    pub pages: u32,
    pub objects: u32,
    pub unavailable: u32,
    pub reasons: Vec<String>,
    pub observed_total: Option<i64>,
    pub omitted_prefix: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraMetadata {
    pub profile_id: u64,
    pub settings_revision: u64,
    pub credential_generation: u64,
    pub cloud_id: String,
    pub interval_start: i64,
    pub interval_end: i64,
    pub coverage: Vec<JiraCoverage>,
    pub observation: String,
    pub current_state: String,
    pub content: JiraContent,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JiraRequest {
    pub cloud_id: String,
    pub issue: String,
    pub category: u32,
    pub start_at: i64,
    pub max_results: i32,
}
#[derive(Clone)]
pub(crate) struct JiraTarget {
    pub selection: String,
}
/// The profile identity a Jira reader stamps into every object's metadata.
pub(crate) struct JiraScope {
    pub profile: IntegrationProfileVersion,
    pub cloud_id: String,
}
