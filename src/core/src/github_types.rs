use serde::{Deserialize, Serialize};

pub const GITHUB_NAMESPACE: &str = "github-com-rest-v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubRepository {
    pub owner: String,
    pub repository: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum GitHubOpenValue {
    Known(String),
    Unknown(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitHubFields {
    Pr {
        state: GitHubOpenValue,
        merged: bool,
        created_at: i64,
        head_sha: String,
        base_sha: String,
        author: String,
        body_provided: bool,
    },
    Review {
        state: String,
        author: Option<String>,
        commit_id: Option<String>,
        empty_body: bool,
    },
    Inline {
        author: Option<String>,
        created_at: i64,
        review_id: Option<i64>,
        reply_id: Option<i64>,
        path: String,
        commit_id: String,
        original_commit_id: String,
        line: Option<i64>,
        original_line: Option<i64>,
        side: Option<GitHubOpenValue>,
        subject_type: Option<GitHubOpenValue>,
        start_line: Option<i64>,
        original_start_line: Option<i64>,
        start_side: Option<GitHubOpenValue>,
    },
    General {
        author: Option<String>,
        created_at: i64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubMetadata {
    pub profile_id: u64,
    pub settings_revision: u64,
    pub credential_generation: u64,
    pub repository_full_name: String,
    pub parent_pr_id: i64,
    pub pr_number: i64,
    pub interval_start: i64,
    pub interval_end: i64,
    pub coverage: String,
    pub observation: String,
    pub current_state: String,
    pub app_projection: bool,
    pub fields: GitHubFields,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubCoverage {
    pub category: u32,
    pub status: String,
    pub pages: u32,
    pub objects: u32,
    pub malformed: u32,
    pub empty: u32,
    pub oversized: u32,
    pub ambiguous: u32,
}
/// A permitted GET; providers build the URL from Core-validated segments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubRequest {
    pub owner: String,
    pub repository: String,
    pub number: i64,
    pub category: u32,
    pub page: u32,
}
#[derive(Clone)]
pub(crate) struct GitHubTarget {
    pub repository: GitHubRepository,
    pub number: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitHubAnchor {
    pub repository_id: i64,
    pub pr_id: i64,
    pub head: String,
}
