use crate::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
#[derive(Clone, Copy, Debug)]
pub(crate) enum Loss {
    Malformed,
    Empty,
    Oversized,
}
fn field<'a>(v: &'a Value, key: &str) -> Result<&'a Value, Loss> {
    v.get(key).ok_or(Loss::Malformed)
}
fn string(v: &Value, max: usize, empty: bool) -> Result<String, Loss> {
    let s = v.as_str().ok_or(Loss::Malformed)?;
    if (!empty && s.is_empty()) || s.len() > max {
        return Err(Loss::Oversized);
    }
    Ok(s.into())
}
fn id(v: &Value) -> Result<i64, Loss> {
    v.as_i64().filter(|v| *v > 0).ok_or(Loss::Malformed)
}
fn date(v: &Value) -> Result<i64, Loss> {
    OffsetDateTime::parse(v.as_str().ok_or(Loss::Malformed)?, &Rfc3339)
        .map(|t| t.unix_timestamp())
        .map_err(|_| Loss::Malformed)
}
fn author(v: &Value, nullable: bool) -> Result<Option<String>, Loss> {
    if nullable && v.is_null() {
        return Ok(None);
    }
    Ok(Some(string(field(v, "login")?, 256, false)?))
}
fn optional<T>(
    v: &Value,
    k: &str,
    nullable: bool,
    parse: impl FnOnce(&Value) -> Result<T, Loss>,
) -> Result<Option<T>, Loss> {
    match v.get(k) {
        None => Ok(None),
        Some(Value::Null) if nullable => Ok(None),
        Some(v) => parse(v).map(Some),
    }
}
fn open(v: &Value, known: &[&str]) -> Result<GitHubOpenValue, Loss> {
    let s = string(v, 256, false)?;
    Ok(if known.contains(&s.as_str()) {
        GitHubOpenValue::Known(s)
    } else {
        GitHubOpenValue::Unknown(s)
    })
}
pub(crate) fn normalize(
    v: &Value,
    category: u32,
    profile: IntegrationProfileVersion,
    repo: &GitHubRepository,
    number: i64,
    anchor: Option<&GitHubAnchor>,
    interval: (i64, i64),
) -> Result<(IntegrationObject, Option<GitHubAnchor>), Loss> {
    let (start, end) = interval;
    let object_id = id(field(v, "id")?)?;
    let (text, source_time, fields, projection, new_anchor, repository_id, parent_id) =
        match category {
            0 => {
                if id(field(v, "number")?)? != number {
                    return Err(Loss::Malformed);
                }
                let base = field(v, "base")?;
                let base_repo = field(base, "repo")?;
                let repository_id = id(field(base_repo, "id")?)?;
                let full = string(field(base_repo, "full_name")?, 201, false)?;
                if !full.eq_ignore_ascii_case(&format!("{}/{}", repo.owner, repo.repository)) {
                    return Err(Loss::Malformed);
                }
                let title = string(field(v, "title")?, MAX_INTEGRATION_BODY, true)?;
                let body = field(v, "body")?;
                let provided = !body.is_null();
                let body = if provided {
                    string(body, MAX_INTEGRATION_BODY, true)?
                } else {
                    "(not provided)".into()
                };
                let head = string(field(field(v, "head")?, "sha")?, 256, false)?;
                let fields = GitHubFields::Pr {
                    state: open(field(v, "state")?, &["open", "closed"])?,
                    merged: field(v, "merged")?.as_bool().ok_or(Loss::Malformed)?,
                    created_at: date(field(v, "created_at")?)?,
                    head_sha: head.clone(),
                    base_sha: string(field(base, "sha")?, 256, false)?,
                    author: author(field(v, "user")?, false)?.ok_or(Loss::Malformed)?,
                    body_provided: provided,
                };
                (
                    format!("GitHub PR title:\n{title}\n\nGitHub PR description:\n{body}"),
                    Some(date(field(v, "updated_at")?)?),
                    fields,
                    true,
                    Some(GitHubAnchor {
                        repository_id,
                        pr_id: object_id,
                        head,
                    }),
                    repository_id,
                    object_id,
                )
            }
            1 => {
                let a = anchor.ok_or(Loss::Malformed)?;
                let body = string(field(v, "body")?, MAX_INTEGRATION_BODY, true)?;
                let state = string(field(v, "state")?, 256, true)?;
                let empty = body.is_empty();
                if empty && state.is_empty() {
                    return Err(Loss::Empty);
                }
                let commit = field(v, "commit_id")?;
                let commit_id = if commit.is_null() {
                    None
                } else {
                    Some(string(commit, 256, false)?)
                };
                let fields = GitHubFields::Review {
                    state,
                    author: author(field(v, "user")?, true)?,
                    commit_id,
                    empty_body: empty,
                };
                (
                    if empty {
                        "Observed GitHub review with an empty body.".into()
                    } else {
                        body
                    },
                    optional(v, "submitted_at", false, date)?,
                    fields,
                    empty,
                    None,
                    a.repository_id,
                    a.pr_id,
                )
            }
            2 => {
                let a = anchor.ok_or(Loss::Malformed)?;
                let body = string(field(v, "body")?, MAX_INTEGRATION_BODY, true)?;
                let user = author(field(v, "user")?, true)?;
                let r = field(v, "pull_request_review_id")?;
                let review_id = if r.is_null() { None } else { Some(id(r)?) };
                let fields = GitHubFields::Inline {
                    author: user,
                    created_at: date(field(v, "created_at")?)?,
                    review_id,
                    reply_id: optional(v, "in_reply_to_id", false, id)?,
                    path: string(field(v, "path")?, 4096, false)?,
                    commit_id: string(field(v, "commit_id")?, 256, false)?,
                    original_commit_id: string(field(v, "original_commit_id")?, 256, false)?,
                    line: optional(v, "line", false, id)?,
                    original_line: optional(v, "original_line", false, id)?,
                    side: optional(v, "side", false, |x| open(x, &["LEFT", "RIGHT"]))?,
                    subject_type: optional(v, "subject_type", false, |x| {
                        open(x, &["line", "file"])
                    })?,
                    start_line: optional(v, "start_line", true, id)?,
                    original_start_line: optional(v, "original_start_line", true, id)?,
                    start_side: optional(v, "start_side", true, |x| open(x, &["LEFT", "RIGHT"]))?,
                };
                let time = date(field(v, "updated_at")?)?;
                if body.is_empty() {
                    return Err(Loss::Empty);
                }
                (
                    body,
                    Some(time),
                    fields,
                    false,
                    None,
                    a.repository_id,
                    a.pr_id,
                )
            }
            3 => {
                let a = anchor.ok_or(Loss::Malformed)?;
                let user = author(field(v, "user")?, true)?;
                let fields = GitHubFields::General {
                    author: user,
                    created_at: date(field(v, "created_at")?)?,
                };
                let time = date(field(v, "updated_at")?)?;
                let body = optional(v, "body", false, |x| string(x, MAX_INTEGRATION_BODY, true))?
                    .ok_or(Loss::Empty)?;
                if body.is_empty() {
                    return Err(Loss::Empty);
                }
                (
                    body,
                    Some(time),
                    fields,
                    false,
                    None,
                    a.repository_id,
                    a.pr_id,
                )
            }
            _ => return Err(Loss::Malformed),
        };
    let full = if category == 0 {
        string(
            field(field(field(v, "base")?, "repo")?, "full_name")?,
            201,
            false,
        )?
    } else {
        format!("{}/{}", repo.owner, repo.repository)
    };
    let metadata = GitHubMetadata {
        profile_id: profile.id,
        settings_revision: profile.settings_revision,
        credential_generation: profile.credential_generation,
        repository_full_name: full,
        parent_pr_id: parent_id,
        pr_number: number,
        interval_start: start,
        interval_end: end,
        coverage: "selected_objects".into(),
        observation: "non_atomic_observation".into(),
        current_state: "current_state_unverified".into(),
        app_projection: projection,
        fields,
    };
    let kind = ["pr", "review", "inline", "general"]
        .get(category as usize)
        .ok_or(Loss::Malformed)?;
    let mut o = IntegrationObject {
        key: SourceKey {
            namespace: GITHUB_NAMESPACE.into(),
            scope: repository_id.to_string(),
            object_id: format!("{kind}:{object_id}"),
        },
        version: String::new(),
        text,
        observed_at_utc: source_time,
        metadata: IntegrationMetadata::GitHub(metadata),
        bindable: true,
    };
    o.version = content_version(&o)?;
    if combined_bytes(&o).map_err(|_| Loss::Malformed)? > MAX_TEXT_BYTES {
        return Err(Loss::Oversized);
    }
    Ok((o, new_anchor))
}
struct Canonical(Sha256);
impl Canonical {
    fn byte(&mut self, b: u8) {
        self.0.update([b]);
    }
    fn integer(&mut self, v: i64) {
        self.0.update(v.to_be_bytes());
    }
    fn string(&mut self, s: &str) {
        self.0.update((s.len() as u64).to_be_bytes());
        self.0.update(s.as_bytes());
    }
    fn opt_int(&mut self, v: Option<i64>) {
        self.byte(u8::from(v.is_some()));
        if let Some(v) = v {
            self.integer(v);
        }
    }
    fn opt_str(&mut self, v: Option<&str>) {
        self.byte(u8::from(v.is_some()));
        if let Some(v) = v {
            self.string(v);
        }
    }
    fn open(&mut self, v: &GitHubOpenValue) {
        self.string(match v {
            GitHubOpenValue::Known(s) | GitHubOpenValue::Unknown(s) => s,
        });
    }
    fn opt_open(&mut self, v: &Option<GitHubOpenValue>) {
        self.byte(u8::from(v.is_some()));
        if let Some(v) = v {
            self.open(v);
        }
    }
}
pub(crate) fn content_version(o: &IntegrationObject) -> Result<String, Loss> {
    let IntegrationMetadata::GitHub(m) = &o.metadata else {
        return Err(Loss::Malformed);
    };
    let mut h = Canonical(Sha256::new());
    h.byte(1);
    h.byte(match m.fields {
        GitHubFields::Pr { .. } => 0,
        GitHubFields::Review { .. } => 1,
        GitHubFields::Inline { .. } => 2,
        GitHubFields::General { .. } => 3,
    });
    h.string(&o.key.namespace);
    h.string(&o.key.scope);
    h.string(&o.key.object_id);
    h.string(&o.text);
    h.byte(u8::from(m.app_projection));
    h.string(&m.repository_full_name);
    h.integer(m.parent_pr_id);
    h.integer(m.pr_number);
    match &m.fields {
        GitHubFields::Pr {
            state,
            merged,
            created_at,
            head_sha,
            base_sha,
            author,
            body_provided,
        } => {
            h.open(state);
            h.byte(u8::from(*merged));
            h.integer(*created_at);
            h.opt_int(o.observed_at_utc);
            h.string(head_sha);
            h.string(base_sha);
            h.string(author);
            h.byte(u8::from(*body_provided));
        }
        GitHubFields::Review {
            state,
            author,
            commit_id,
            empty_body,
        } => {
            h.string(state);
            h.opt_str(author.as_deref());
            h.opt_str(commit_id.as_deref());
            h.opt_int(o.observed_at_utc);
            h.byte(u8::from(*empty_body));
        }
        GitHubFields::Inline {
            author,
            created_at,
            review_id,
            reply_id,
            path,
            commit_id,
            original_commit_id,
            line,
            original_line,
            side,
            subject_type,
            start_line,
            original_start_line,
            start_side,
        } => {
            h.opt_str(author.as_deref());
            h.integer(*created_at);
            h.opt_int(o.observed_at_utc);
            h.opt_int(*review_id);
            h.opt_int(*reply_id);
            h.string(path);
            h.string(commit_id);
            h.string(original_commit_id);
            h.opt_int(*line);
            h.opt_int(*original_line);
            h.opt_open(side);
            h.opt_open(subject_type);
            h.opt_int(*start_line);
            h.opt_int(*original_start_line);
            h.opt_open(start_side);
        }
        GitHubFields::General { author, created_at } => {
            h.opt_str(author.as_deref());
            h.integer(*created_at);
            h.opt_int(o.observed_at_utc);
        }
    }
    let bytes = h.0.finalize();
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub(crate) fn validate_metadata(m: &GitHubMetadata) -> Result<(), StoreError> {
    for id in [m.profile_id, m.settings_revision, m.credential_generation] {
        if id == 0 || id > i64::MAX as u64 {
            return Err(StoreError::InvalidInput);
        }
    }
    let parts = m.repository_full_name.split('/').collect::<Vec<_>>();
    if parts.len() != 2
        || !valid_segment(parts[0])
        || !valid_segment(parts[1])
        || m.parent_pr_id <= 0
        || m.pr_number <= 0
        || m.interval_end < m.interval_start
        || m.coverage != "selected_objects"
        || m.observation != "non_atomic_observation"
        || m.current_state != "current_state_unverified"
        || serde_json::to_vec(m)
            .map_err(|_| StoreError::InvalidInput)?
            .len()
            > MAX_TEXT_BYTES
    {
        return Err(StoreError::InvalidInput);
    }
    // Retained typed data is checked again at database admission, without external I/O.
    fn bounded(s: &str, max: usize) -> bool {
        !s.is_empty() && s.len() <= max
    }
    fn author_ok(v: &Option<String>) -> bool {
        v.as_ref().is_none_or(|s| bounded(s, 256))
    }
    fn value_ok(v: &GitHubOpenValue, known: &[&str]) -> bool {
        match v {
            GitHubOpenValue::Known(s) => bounded(s, 256) && known.contains(&s.as_str()),
            GitHubOpenValue::Unknown(s) => bounded(s, 256) && !known.contains(&s.as_str()),
        }
    }
    let good = match &m.fields {
        GitHubFields::Pr {
            state,
            created_at,
            head_sha,
            base_sha,
            author,
            ..
        } => {
            value_ok(state, &["open", "closed"])
                && bounded(head_sha, 256)
                && bounded(base_sha, 256)
                && bounded(author, 256)
                && OffsetDateTime::from_unix_timestamp(*created_at).is_ok()
        }
        GitHubFields::Review {
            state,
            author,
            commit_id,
            empty_body,
        } => {
            state.len() <= 256
                && author_ok(author)
                && commit_id.as_ref().is_none_or(|s| bounded(s, 256))
                && (!*empty_body || (!state.is_empty()))
        }
        GitHubFields::Inline {
            author,
            created_at,
            review_id,
            reply_id,
            path,
            commit_id,
            original_commit_id,
            line,
            original_line,
            side,
            subject_type,
            start_line,
            original_start_line,
            start_side,
        } => {
            author_ok(author)
                && bounded(path, 4096)
                && bounded(commit_id, 256)
                && bounded(original_commit_id, 256)
                && [
                    review_id,
                    reply_id,
                    line,
                    original_line,
                    start_line,
                    original_start_line,
                ]
                .iter()
                .all(|v| v.is_none_or(|n| n > 0))
                && side
                    .as_ref()
                    .is_none_or(|v| value_ok(v, &["LEFT", "RIGHT"]))
                && start_side
                    .as_ref()
                    .is_none_or(|v| value_ok(v, &["LEFT", "RIGHT"]))
                && subject_type
                    .as_ref()
                    .is_none_or(|v| value_ok(v, &["line", "file"]))
                && OffsetDateTime::from_unix_timestamp(*created_at).is_ok()
        }
        GitHubFields::General { author, created_at } => {
            author_ok(author) && OffsetDateTime::from_unix_timestamp(*created_at).is_ok()
        }
    };
    let projection = match &m.fields {
        GitHubFields::Pr { .. } => true,
        GitHubFields::Review { empty_body, .. } => *empty_body,
        _ => false,
    };
    if !good || projection != m.app_projection {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
pub(crate) fn validate_object(o: &IntegrationObject, edited: bool) -> Result<(), StoreError> {
    let IntegrationMetadata::GitHub(m) = &o.metadata else {
        return Err(StoreError::InvalidInput);
    };
    validate_metadata(m)?;
    let scope = o.key.scope.parse::<i64>().ok().filter(|i| *i > 0);
    let kind = match &m.fields {
        GitHubFields::Pr { .. } => "pr",
        GitHubFields::Review { .. } => "review",
        GitHubFields::Inline { .. } => "inline",
        GitHubFields::General { .. } => "general",
    };
    let object_id = o
        .key
        .object_id
        .strip_prefix(&format!("{kind}:"))
        .and_then(|x| x.parse::<i64>().ok())
        .filter(|i| *i > 0)
        .ok_or(StoreError::InvalidInput)?;
    if o.key.namespace != GITHUB_NAMESPACE
        || scope.map(|i| i.to_string()).as_deref() != Some(&o.key.scope)
        || o.key.object_id != format!("{kind}:{object_id}")
        || (kind == "pr" && object_id != m.parent_pr_id)
        || combined_bytes(o)? > MAX_TEXT_BYTES
        || o.text.is_empty()
        || o.version.len() != 64
        || !o
            .version
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || (!edited
            && (content_version(o).ok().as_deref() != Some(o.version.as_str())
                || (kind != "review" && o.observed_at_utc.is_none())
                || o.observed_at_utc
                    .is_some_and(|t| OffsetDateTime::from_unix_timestamp(t).is_err())
                || (matches!(
                    m.fields,
                    GitHubFields::Review {
                        empty_body: true,
                        ..
                    }
                ) && o.text != "Observed GitHub review with an empty body.")))
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
pub(crate) fn valid_segment(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 100
        && v != "."
        && v != ".."
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
