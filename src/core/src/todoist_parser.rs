//! Todoist REST v1 projection: selected fields with Missing/Null/Invalid/Value and explicit losses.
use crate::*;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Loss {
    Malformed,
    Oversized,
    Empty,
    /// A deleted comment is never bound; coverage counts it as skipped.
    Deleted,
    /// Text alone is a fact; an attachment alone is not turned into one.
    AttachmentOnly,
}
fn opaque(v: &Value, max: usize) -> Result<String, Loss> {
    v.as_str()
        .filter(|s| valid_opaque(s, max))
        .map(str::to_owned)
        .ok_or(Loss::Malformed)
}
fn valid_opaque(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
/// Task, project and comment IDs: opaque strings, never path dots or client `tmp-` placeholders.
pub(crate) fn valid_object_id(s: &str) -> bool {
    valid_opaque(s, 256) && s != "." && s != ".." && !s.starts_with("tmp-")
}
fn object_id(v: Option<&Value>) -> Result<String, Loss> {
    v.and_then(Value::as_str)
        .filter(|s| valid_object_id(s))
        .map(str::to_owned)
        .ok_or(Loss::Malformed)
}
/// The account recorded as the profile scope: 1..128 bytes without control characters.
pub(crate) fn valid_account(s: &str) -> bool {
    valid_opaque(s, 128)
}
fn string(v: &Value, max: usize) -> Result<String, Loss> {
    v.as_str()
        .filter(|s| s.len() <= max)
        .map(str::to_owned)
        .ok_or(Loss::Malformed)
}
fn boolean(v: &Value) -> Result<bool, Loss> {
    v.as_bool().ok_or(Loss::Malformed)
}
fn time(v: &Value) -> Result<TodoistTime, Loss> {
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};
    let raw = v
        .as_str()
        .filter(|s| s.is_ascii() && s.len() <= 128)
        .ok_or(Loss::Malformed)?;
    let t = OffsetDateTime::parse(raw, &Rfc3339).map_err(|_| Loss::Malformed)?;
    Ok(TodoistTime {
        utc_seconds: t.unix_timestamp(),
        precision_lost: t.nanosecond() != 0,
    })
}
fn presence<T>(
    v: Option<&Value>,
    parse: impl FnOnce(&Value) -> Result<T, Loss>,
) -> TodoistPresence<T> {
    match v {
        None => TodoistPresence::Missing,
        Some(Value::Null) => TodoistPresence::Null,
        Some(v) => parse(v).map_or(TodoistPresence::Invalid, TodoistPresence::Value),
    }
}
fn field<T>(
    v: &Value,
    k: &str,
    losses: &mut Vec<String>,
    parse: impl FnOnce(&Value) -> Result<T, Loss>,
) -> TodoistPresence<T> {
    let result = presence(v.get(k), parse);
    match &result {
        TodoistPresence::Missing => losses.push(format!("missing:{k}")),
        TodoistPresence::Invalid => losses.push(format!("invalid:{k}")),
        _ => {}
    }
    result
}
/// Known subfields of a due/deadline/duration object; unknown keys and wrong shapes are losses.
fn record<T>(
    v: &Value,
    name: &str,
    known: &[&str],
    losses: &mut Vec<String>,
    build: impl FnOnce(&Value, &mut Vec<String>) -> T,
) -> Result<T, Loss> {
    let map = v.as_object().ok_or(Loss::Malformed)?;
    if map.keys().any(|k| !known.contains(&k.as_str())) {
        losses.push(format!("unknown:{name}"));
    }
    let mut inner = Vec::new();
    let value = build(v, &mut inner);
    // `missing:date` inside `due` becomes `missing:due.date`.
    losses.extend(
        inner
            .into_iter()
            .map(|l| l.replacen(':', &format!(":{name}."), 1)),
    );
    Ok(value)
}
fn due(v: &Value, losses: &mut Vec<String>) -> Result<TodoistDue, Loss> {
    let known = ["date", "timezone", "is_recurring", "string", "lang"];
    record(v, "due", &known, losses, |v, l| TodoistDue {
        date: field(v, "date", l, |v| string(v, 128)),
        timezone: field(v, "timezone", l, |v| string(v, 128)),
        is_recurring: field(v, "is_recurring", l, boolean),
        string: field(v, "string", l, |v| string(v, 1024)),
        lang: field(v, "lang", l, |v| string(v, 64)),
    })
}
fn deadline(v: &Value, losses: &mut Vec<String>) -> Result<TodoistDeadline, Loss> {
    record(v, "deadline", &["date", "lang"], losses, |v, l| {
        TodoistDeadline {
            date: field(v, "date", l, |v| string(v, 128)),
            lang: field(v, "lang", l, |v| string(v, 64)),
        }
    })
}
fn duration(v: &Value, losses: &mut Vec<String>) -> Result<TodoistDuration, Loss> {
    record(v, "duration", &["amount", "unit"], losses, |v, l| {
        TodoistDuration {
            amount: field(v, "amount", l, |v| {
                v.as_i64().filter(|n| *n >= 0).ok_or(Loss::Malformed)
            }),
            unit: field(v, "unit", l, |v| string(v, 64)),
        }
    })
}
/// A nested record kept with presence; its own shape failure is one `invalid:` loss.
fn nested<T>(
    v: &Value,
    k: &str,
    losses: &mut Vec<String>,
    parse: impl FnOnce(&Value, &mut Vec<String>) -> Result<T, Loss>,
) -> TodoistPresence<T> {
    let mut inner = Vec::new();
    let result = field(v, k, losses, |v| parse(v, &mut inner));
    losses.extend(inner);
    result
}
fn uid(v: &Value, k: &str, losses: &mut Vec<String>) -> TodoistPresence<String> {
    field(v, k, losses, |v| opaque(v, 256))
}

/// The selected task. Its ID and project ID are identity, so their absence fails the anchor.
pub(crate) fn task(v: &Value) -> Result<TodoistContent, Loss> {
    let id = object_id(v.get("id"))?;
    let project_id = object_id(v.get("project_id"))?;
    let mut losses = Vec::new();
    let l = &mut losses;
    let fields = TodoistFields::Task(Box::new(TodoistTask {
        user_id: uid(v, "user_id", l),
        project_id,
        content: field(v, "content", l, |v| string(v, 4096)),
        description: field(v, "description", l, |v| string(v, 16384)),
        parent_id: uid(v, "parent_id", l),
        section_id: uid(v, "section_id", l),
        added_by_uid: uid(v, "added_by_uid", l),
        assigned_by_uid: uid(v, "assigned_by_uid", l),
        responsible_uid: uid(v, "responsible_uid", l),
        completed_by_uid: uid(v, "completed_by_uid", l),
        checked: field(v, "checked", l, boolean),
        is_deleted: field(v, "is_deleted", l, boolean),
        added_at: field(v, "added_at", l, time),
        updated_at: field(v, "updated_at", l, time),
        completed_at: field(v, "completed_at", l, time),
        due: nested(v, "due", l, due),
        deadline: nested(v, "deadline", l, deadline),
        duration: nested(v, "duration", l, duration),
    }));
    Ok(TodoistContent {
        parent_task_id: id.clone(),
        object_id: id,
        fields,
        losses,
    })
}
/// One project object of the personal/workspace union; its kind is never guessed from a field.
pub(crate) fn project(v: &Value, task: &str) -> Result<TodoistContent, Loss> {
    if !v.is_object() {
        return Err(Loss::Malformed);
    }
    let id = object_id(v.get("id"))?;
    let mut losses = Vec::new();
    let l = &mut losses;
    let fields = TodoistFields::Project {
        name: field(v, "name", l, |v| string(v, 4096)),
        description: field(v, "description", l, |v| string(v, 16384)),
        parent_id: uid(v, "parent_id", l),
        // Only workspace projects carry it; its absence is no loss and no type guess.
        workspace_id: presence(v.get("workspace_id"), |v| opaque(v, 256)),
        is_archived: field(v, "is_archived", l, boolean),
        is_deleted: field(v, "is_deleted", l, boolean),
        created_at: field(v, "created_at", l, time),
        updated_at: field(v, "updated_at", l, time),
    };
    Ok(TodoistContent {
        parent_task_id: task.into(),
        object_id: id,
        fields,
        losses,
    })
}
/// A comment of the fixed task query; ownership follows from the query, not from `item_id`.
pub(crate) fn comment(v: &Value, task: &str) -> Result<TodoistContent, Loss> {
    if !v.is_object() {
        return Err(Loss::Malformed);
    }
    let id = object_id(v.get("id"))?;
    let mut losses = Vec::new();
    let l = &mut losses;
    // A null text is invalid, never an empty string; a missing one stays missing.
    let content = match v.get("content") {
        Some(Value::Null) => {
            l.push("invalid:content".into());
            TodoistPresence::Invalid
        }
        _ => field(v, "content", l, |v| string(v, 16384)),
    };
    let file_attachment = match v.get("file_attachment") {
        None => {
            l.push("missing:file_attachment".into());
            false
        }
        Some(Value::Null) => false,
        Some(Value::Object(_)) => {
            l.push("attachment_omitted".into());
            true
        }
        Some(_) => {
            l.push("invalid:file_attachment".into());
            false
        }
    };
    let fields = TodoistFields::Comment {
        content,
        posted_uid: uid(v, "posted_uid", l),
        posted_at: field(v, "posted_at", l, time),
        is_deleted: field(v, "is_deleted", l, boolean),
        file_attachment,
    };
    Ok(TodoistContent {
        parent_task_id: task.into(),
        object_id: id,
        fields,
        losses,
    })
}

fn value_text(p: &TodoistPresence<String>) -> &str {
    if let TodoistPresence::Value(s) = p {
        s
    } else {
        ""
    }
}
/// The literal Markdown an object contributes; empty means no fact to bind.
fn text(fields: &TodoistFields) -> String {
    let (head, body) = match fields {
        TodoistFields::Task(t) => (value_text(&t.content), value_text(&t.description)),
        TodoistFields::Project {
            name, description, ..
        } => (value_text(name), value_text(description)),
        TodoistFields::Comment { content, .. } => (value_text(content), ""),
    };
    if head.trim().is_empty() {
        String::new()
    } else if body.is_empty() {
        head.into()
    } else {
        format!("{head}\n\n{body}")
    }
}
fn source_time(fields: &TodoistFields) -> Option<i64> {
    let t = match fields {
        TodoistFields::Task(t) => &t.updated_at,
        TodoistFields::Project { updated_at, .. } => updated_at,
        TodoistFields::Comment { posted_at, .. } => posted_at,
    };
    if let TodoistPresence::Value(t) = t {
        Some(t.utc_seconds)
    } else {
        None
    }
}
fn precision_lost(fields: &TodoistFields) -> bool {
    let lost = |t: &TodoistPresence<TodoistTime>| matches!(t, TodoistPresence::Value(t) if t.precision_lost);
    match fields {
        TodoistFields::Task(t) => lost(&t.added_at) || lost(&t.updated_at) || lost(&t.completed_at),
        TodoistFields::Project {
            created_at,
            updated_at,
            ..
        } => lost(created_at) || lost(updated_at),
        TodoistFields::Comment { posted_at, .. } => lost(posted_at),
    }
}
/// Compact JSON tuples keep opaque IDs separate without a delimiter.
fn object_key(account: &str, c: &TodoistContent) -> Result<SourceKey, StoreError> {
    let object_id = match c.fields {
        TodoistFields::Task(_) => serde_json::to_string(&("task", &c.object_id)),
        TodoistFields::Project { .. } => serde_json::to_string(&("project", &c.object_id)),
        TodoistFields::Comment { .. } => {
            serde_json::to_string(&("comment", &c.parent_task_id, &c.object_id))
        }
    }
    .map_err(|_| StoreError::InvalidInput)?;
    Ok(SourceKey {
        namespace: TODOIST_NAMESPACE.into(),
        scope: account.into(),
        object_id,
    })
}
fn content_version(o: &IntegrationObject) -> Result<String, StoreError> {
    use sha2::{Digest, Sha256};
    let IntegrationMetadata::Todoist(m) = &o.metadata else {
        return Err(StoreError::InvalidInput);
    };
    let mut hash = Sha256::new();
    // Typed fields have a fixed serialization order; length framing separates each canonical component.
    hash.update(b"todoist-api-content-v1");
    for bytes in [
        o.key.namespace.as_bytes(),
        o.key.scope.as_bytes(),
        o.key.object_id.as_bytes(),
        o.text.as_bytes(),
        &serde_json::to_vec(&m.content).map_err(|_| StoreError::InvalidInput)?,
    ] {
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
/// Builds the window object; coverage and the interval end are stamped when the read finishes.
pub(crate) fn object(
    scope: &TodoistScope,
    mut c: TodoistContent,
    interval: (i64, i64),
) -> Result<IntegrationObject, Loss> {
    if let TodoistFields::Comment { is_deleted, .. } = &c.fields {
        match is_deleted {
            TodoistPresence::Value(false) => {}
            TodoistPresence::Value(true) => return Err(Loss::Deleted),
            _ => return Err(Loss::Malformed),
        }
    }
    let text = text(&c.fields);
    if text.is_empty() {
        return Err(match c.fields {
            TodoistFields::Comment {
                file_attachment: true,
                ..
            } => Loss::AttachmentOnly,
            _ => Loss::Empty,
        });
    }
    if precision_lost(&c.fields) {
        c.losses.push("precision_lost".into());
    }
    let mut o = IntegrationObject {
        key: object_key(&scope.account, &c).map_err(|_| Loss::Malformed)?,
        version: String::new(),
        text,
        observed_at_utc: source_time(&c.fields),
        metadata: IntegrationMetadata::Todoist(TodoistMetadata {
            profile_id: scope.profile.id,
            settings_revision: scope.profile.settings_revision,
            credential_generation: scope.profile.credential_generation,
            account: scope.account.clone(),
            interval_start: interval.0,
            interval_end: interval.1,
            coverage: Vec::new(),
            observation: "non_atomic_observation".into(),
            current_state: "current_state_unverified".into(),
            content: c,
        }),
        bindable: true,
    };
    o.version = content_version(&o).map_err(|_| Loss::Malformed)?;
    if combined_bytes(&o).map_err(|_| Loss::Malformed)? > MAX_TEXT_BYTES {
        return Err(Loss::Oversized);
    }
    Ok(o)
}

pub(crate) fn validate_metadata(m: &TodoistMetadata) -> Result<(), StoreError> {
    fn check<T>(p: &TodoistPresence<T>, f: impl FnOnce(&T) -> bool) -> bool {
        match p {
            TodoistPresence::Value(v) => f(v),
            _ => true,
        }
    }
    fn s(p: &TodoistPresence<String>, max: usize) -> bool {
        check(p, |v| v.len() <= max)
    }
    fn id(p: &TodoistPresence<String>) -> bool {
        check(p, |v| valid_opaque(v, 256))
    }
    fn t(p: &TodoistPresence<TodoistTime>) -> bool {
        check(p, |v| {
            time::OffsetDateTime::from_unix_timestamp(v.utc_seconds).is_ok()
        })
    }
    fn active(p: &TodoistPresence<bool>) -> bool {
        matches!(p, TodoistPresence::Value(false))
    }
    fn text(p: &TodoistPresence<String>, max: usize) -> bool {
        matches!(p, TodoistPresence::Value(v) if !v.trim().is_empty() && v.len() <= max)
    }
    let c = &m.content;
    let good = match &c.fields {
        TodoistFields::Task(task) => {
            let TodoistTask {
                user_id,
                project_id,
                content,
                description,
                parent_id,
                section_id,
                added_by_uid,
                assigned_by_uid,
                responsible_uid,
                completed_by_uid,
                checked,
                is_deleted,
                added_at,
                updated_at,
                completed_at,
                due,
                deadline,
                duration,
            } = task.as_ref();
            c.parent_task_id == c.object_id
                && valid_object_id(project_id)
                && text(content, 4096)
                && s(description, 16384)
                && [
                    user_id,
                    parent_id,
                    section_id,
                    added_by_uid,
                    assigned_by_uid,
                    responsible_uid,
                    completed_by_uid,
                ]
                .into_iter()
                .all(id)
                && active(checked)
                && active(is_deleted)
                && [added_at, updated_at, completed_at].into_iter().all(t)
                && check(due, |d| {
                    s(&d.date, 128) && s(&d.timezone, 128) && s(&d.string, 1024) && s(&d.lang, 64)
                })
                && check(deadline, |d| s(&d.date, 128) && s(&d.lang, 64))
                && check(duration, |d| {
                    check(&d.amount, |n| *n >= 0) && s(&d.unit, 64)
                })
        }
        TodoistFields::Project {
            name,
            description,
            parent_id,
            workspace_id,
            created_at,
            updated_at,
            ..
        } => {
            text(name, 4096)
                && s(description, 16384)
                && id(parent_id)
                && id(workspace_id)
                && t(created_at)
                && t(updated_at)
        }
        TodoistFields::Comment {
            content,
            posted_uid,
            posted_at,
            is_deleted,
            ..
        } => text(content, 16384) && id(posted_uid) && t(posted_at) && active(is_deleted),
    };
    if !good
        || [m.profile_id, m.settings_revision, m.credential_generation]
            .iter()
            .any(|id| *id == 0 || *id > i64::MAX as u64)
        || !valid_account(&m.account)
        || !valid_object_id(&c.parent_task_id)
        || !valid_object_id(&c.object_id)
        || m.interval_end < m.interval_start
        || m.observation != "non_atomic_observation"
        || m.current_state != "current_state_unverified"
        || c.losses.len() > 1024
        || c.losses.iter().any(|s| s.len() > 128)
        || m.coverage.len() != 3
        || m.coverage.iter().enumerate().any(|(i, c)| {
            c.category != i as u32
                || !["not_read", "read", "partial"].contains(&c.status.as_str())
                || c.objects > 512
                || c.unavailable > 512
                || c.pages > crate::todoist_adapter::MAX_COMMENT_PAGES
                || c.reasons.len() > 16
                || c.reasons.iter().any(|s| s.len() > 128)
        })
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
pub(crate) fn validate_object(o: &IntegrationObject, edited: bool) -> Result<(), StoreError> {
    let IntegrationMetadata::Todoist(m) = &o.metadata else {
        return Err(StoreError::InvalidInput);
    };
    validate_metadata(m)?;
    if object_key(&m.account, &m.content)? != o.key
        || o.key.validate().is_err()
        || combined_bytes(o)? > MAX_TEXT_BYTES
        || o.text.is_empty()
        || o.version.len() != 64
        || !o
            .version
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || (!edited
            && (content_version(o)? != o.version
                || o.text != text(&m.content.fields)
                || o.observed_at_utc != source_time(&m.content.fields)))
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
