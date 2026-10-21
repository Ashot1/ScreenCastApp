use crate::*;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Loss {
    Malformed,
    Oversized,
    Empty,
}
fn date(v: &Value) -> Result<JiraDate, Loss> {
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};
    let raw = v
        .as_str()
        .filter(|s| s.is_ascii() && s.len() <= 128 && s.len() >= 20)
        .ok_or(Loss::Malformed)?;
    if raw
        .as_bytes()
        .get(17..19)
        .and_then(|s| std::str::from_utf8(s).ok())
        .and_then(|s| s.parse::<u8>().ok())
        .is_none_or(|s| s > 59)
    {
        return Err(Loss::Malformed);
    }
    let mut normalized = raw.to_owned();
    let n = raw.len();
    if matches!(raw.as_bytes()[n - 5], b'+' | b'-')
        && raw.as_bytes()[n - 4..].iter().all(u8::is_ascii_digit)
    {
        normalized.insert(n - 2, ':');
    }
    if let Some(fraction) = normalized.get(19..).and_then(|s| s.strip_prefix('.')) {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if !(1..=9).contains(&digits) {
            return Err(Loss::Malformed);
        }
    }
    let d = OffsetDateTime::parse(&normalized, &Rfc3339).map_err(|_| Loss::Malformed)?;
    Ok(JiraDate {
        utc_seconds: d.unix_timestamp(),
        precision_lost: d.nanosecond() != 0,
    })
}
fn body(v: Option<&Value>) -> Result<JiraBody, Loss> {
    let Some(v) = v else {
        return Ok(JiraBody {
            text: JiraPresence::Missing,
            losses: vec!["missing_body".into()],
        });
    };
    if v.is_null() {
        return Ok(JiraBody {
            text: JiraPresence::Null,
            losses: Vec::new(),
        });
    }
    if !v.is_object()
        || v.get("type").and_then(Value::as_str) != Some("doc")
        || v.get("version").and_then(Value::as_i64) != Some(1)
        || !v.get("content").is_some_and(Value::is_array)
    {
        return Ok(JiraBody {
            text: JiraPresence::Invalid,
            losses: vec!["unsupported_body".into()],
        });
    }
    let mut text = String::new();
    let mut losses = std::collections::BTreeSet::new();
    let mut stack = vec![(v, 0usize, false)];
    let mut nodes = 0usize;
    fn append(out: &mut String, part: &str) -> Result<(), Loss> {
        if out.len().checked_add(part.len()).is_none_or(|n| n > 16384) {
            return Err(Loss::Oversized);
        }
        out.push_str(part);
        Ok(())
    }
    // An explicit stack bounds both traversal depth and work without walking arbitrary attrs.
    while let Some((node, depth, closing)) = stack.pop() {
        let kind = node
            .get("type")
            .and_then(Value::as_str)
            .ok_or(Loss::Malformed)?;
        if closing {
            let end = match kind {
                "paragraph" | "heading" | "blockquote" | "codeBlock" | "listItem" | "tableRow" => {
                    "\n"
                }
                "tableCell" | "tableHeader" => " | ",
                _ => "",
            };
            append(&mut text, end)?;
            continue;
        }
        nodes = nodes.checked_add(1).ok_or(Loss::Oversized)?;
        if depth > 64 || nodes > 10000 || kind.len() > 256 {
            return Err(Loss::Oversized);
        }
        if let Some(marks) = node.get("marks") {
            let marks = marks.as_array().ok_or(Loss::Malformed)?;
            nodes = nodes.checked_add(marks.len()).ok_or(Loss::Oversized)?;
            if nodes > 10000 {
                return Err(Loss::Oversized);
            }
            for mark in marks {
                let name = mark
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or(Loss::Malformed)?;
                if name.len() > 256 {
                    return Err(Loss::Oversized);
                }
                losses.insert(
                    if name == "link" {
                        "link_address_omitted"
                    } else {
                        "marks_omitted"
                    }
                    .to_owned(),
                );
            }
        }
        match kind {
            "doc" => {}
            "text" => append(
                &mut text,
                node.get("text")
                    .and_then(Value::as_str)
                    .ok_or(Loss::Malformed)?,
            )?,
            "hardBreak" => append(&mut text, "\n")?,
            "mention" => {
                if let Some(label) = node
                    .get("attrs")
                    .and_then(|a| a.get("text"))
                    .and_then(Value::as_str)
                {
                    append(&mut text, label)?;
                } else {
                    losses.insert("mention_label_missing".into());
                }
            }
            "media" | "mediaSingle" | "mediaGroup" | "inlineCard" | "blockCard" | "embedCard" => {
                losses.insert("media_or_card_omitted".into());
                continue;
            }
            "paragraph" => {}
            "listItem" => {
                append(&mut text, "- ")?;
                losses.insert("formatting_omitted".into());
            }
            "heading" | "bulletList" | "orderedList" | "blockquote" | "codeBlock" | "table"
            | "tableRow" | "tableCell" | "tableHeader" => {
                losses.insert("formatting_omitted".into());
            }
            _ => {
                losses.insert("unknown_node".into());
            }
        }
        stack.push((node, depth, true));
        if let Some(content) = node.get("content") {
            let children = content.as_array().ok_or(Loss::Malformed)?;
            if children.len().checked_add(nodes).is_none_or(|n| n > 10000) {
                return Err(Loss::Oversized);
            }
            for child in children.iter().rev() {
                stack.push((child, depth + 1, false));
            }
        }
    }
    Ok(JiraBody {
        text: JiraPresence::Value(text),
        losses: losses.into_iter().collect(),
    })
}

pub(crate) fn valid_cloud_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
pub(crate) fn opaque(v: &Value, max: usize) -> Result<String, Loss> {
    let s = v
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
        .ok_or(Loss::Malformed)?;
    Ok(s.into())
}
fn string(v: &Value, max: usize) -> Result<String, Loss> {
    v.as_str()
        .filter(|s| s.len() <= max)
        .map(str::to_owned)
        .ok_or(Loss::Malformed)
}
fn presence<T>(
    v: Option<&Value>,
    parse: impl FnOnce(&Value) -> Result<T, Loss>,
) -> JiraPresence<T> {
    match v {
        None => JiraPresence::Missing,
        Some(Value::Null) => JiraPresence::Null,
        Some(v) => parse(v).map_or(JiraPresence::Invalid, JiraPresence::Value),
    }
}
fn field<T>(
    v: &Value,
    k: &str,
    losses: &mut Vec<String>,
    parse: impl FnOnce(&Value) -> Result<T, Loss>,
) -> JiraPresence<T> {
    let result = presence(v.get(k), parse);
    match &result {
        JiraPresence::Missing => losses.push(format!("missing:{k}")),
        JiraPresence::Invalid => losses.push(format!("invalid:{k}")),
        _ => {}
    }
    result
}
fn named(v: &Value, project: bool, losses: &mut Vec<String>) -> Result<JiraNamed, Loss> {
    if !v.is_object() {
        return Err(Loss::Malformed);
    }
    Ok(JiraNamed {
        id: field(v, "id", losses, |v| opaque(v, 256)),
        name: field(v, "name", losses, |v| string(v, 512)),
        key: if project {
            field(v, "key", losses, |v| string(v, 256))
        } else {
            JiraPresence::Missing
        },
    })
}
fn author(v: &Value, losses: &mut Vec<String>) -> Result<JiraAuthor, Loss> {
    if !v.is_object() {
        return Err(Loss::Malformed);
    }
    Ok(JiraAuthor {
        account_id: field(v, "accountId", losses, |v| opaque(v, 128)),
        display_name: field(v, "displayName", losses, |v| string(v, 512)),
    })
}
fn value_text(p: &JiraPresence<String>) -> &str {
    if let JiraPresence::Value(s) = p {
        s
    } else {
        ""
    }
}
fn source_time(fields: &JiraFields) -> Option<i64> {
    let date = match fields {
        JiraFields::Issue { updated, .. } | JiraFields::Comment { updated, .. } => updated,
        JiraFields::Changelog { created, .. } => created,
        _ => return None,
    };
    if let JiraPresence::Value(d) = date {
        Some(d.utc_seconds)
    } else {
        None
    }
}
fn object_key(cloud: &str, c: &JiraContent) -> Result<SourceKey, Loss> {
    Ok(SourceKey {
        namespace: JIRA_NAMESPACE.into(),
        scope: cloud.into(),
        object_id: serde_json::to_string(&(&c.tag, &c.parent_issue_id, &c.object_id))
            .map_err(|_| Loss::Malformed)?,
    })
}
pub(crate) fn content_version(o: &IntegrationObject) -> Result<String, StoreError> {
    use sha2::{Digest, Sha256};
    let IntegrationMetadata::Jira(m) = &o.metadata else {
        return Err(StoreError::InvalidInput);
    };
    let mut hash = Sha256::new();
    // Typed fields have a fixed serialization order; length framing separates each canonical component.
    hash.update(b"jira-cloud-content-v1");
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
fn make(
    p: &JiraScope,
    mut c: JiraContent,
    text: String,
    interval: (i64, i64),
) -> Result<IntegrationObject, Loss> {
    if text.trim().is_empty() {
        return Err(Loss::Empty);
    }
    let lost =
        |date: &JiraPresence<JiraDate>| matches!(date,JiraPresence::Value(d) if d.precision_lost);
    let precision = match &c.fields {
        JiraFields::Issue {
            created, updated, ..
        }
        | JiraFields::Comment {
            created, updated, ..
        } => lost(created) || lost(updated),
        JiraFields::Changelog { created, .. } => lost(created),
        _ => false,
    };
    if precision {
        c.losses.push("precision_lost".into());
    }
    let mut o = IntegrationObject {
        key: object_key(&p.cloud_id, &c)?,
        version: String::new(),
        text,
        observed_at_utc: source_time(&c.fields),
        metadata: IntegrationMetadata::Jira(JiraMetadata {
            profile_id: p.profile.id,
            settings_revision: p.profile.settings_revision,
            credential_generation: p.profile.credential_generation,
            cloud_id: p.cloud_id.clone(),
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
fn relation(
    v: &Value,
    link: Option<(&str, &Value)>,
    direction: &str,
    p: &JiraScope,
    parent: &str,
    key: &str,
    interval: (i64, i64),
) -> Result<IntegrationObject, Loss> {
    let tag = if link.is_some() {
        "issue_link"
    } else {
        direction
    };
    let link_id = link.map(|(id, _)| id);
    let link_type = link.map(|(_, value)| value);
    let label = if let Some(t) = link_type {
        opaque(t.get(direction).ok_or(Loss::Malformed)?, 512)?
    } else {
        direction.to_owned()
    };
    let target = opaque(v.get("id").ok_or(Loss::Malformed)?, 256)?;
    let mut losses = Vec::new();
    let target_key = field(v, "key", &mut losses, |v| string(v, 256));
    let fields = v.get("fields");
    if fields.is_some_and(|v| !v.is_object()) {
        losses.push("invalid:fields".into());
    }
    if fields.is_none() {
        losses.push("missing:fields".into());
    }
    let f = fields.filter(|v| v.is_object()).unwrap_or(&Value::Null);
    let summary = field(f, "summary", &mut losses, |v| string(v, 4096));
    let status = presence(f.get("status"), |v| named(v, false, &mut losses));
    let lt = presence(link_type, |v| named(v, false, &mut losses));
    let inward = presence(link_type.and_then(|v| v.get("inward")), |v| string(v, 512));
    let outward = presence(link_type.and_then(|v| v.get("outward")), |v| string(v, 512));
    let text = format!(
        "{label} {}\n{}",
        if value_text(&target_key).is_empty() {
            &target
        } else {
            value_text(&target_key)
        },
        value_text(&summary)
    );
    let content = JiraContent {
        tag: tag.into(),
        parent_issue_id: parent.into(),
        object_id: link_id.unwrap_or(&target).into(),
        observed_issue_key: key.into(),
        losses,
        fields: JiraFields::Relation {
            target_id: target,
            target_key,
            summary,
            status,
            direction: direction.into(),
            label,
            link_type: lt,
            inward,
            outward,
        },
    };
    make(p, content, text, interval)
}
pub(crate) fn issue(
    v: &Value,
    p: &JiraScope,
    interval: (i64, i64),
) -> Result<(String, String, Vec<IntegrationObject>, Vec<String>), Loss> {
    let id = opaque(v.get("id").ok_or(Loss::Malformed)?, 256)?;
    let key = opaque(v.get("key").ok_or(Loss::Malformed)?, 256)?;
    let f = v
        .get("fields")
        .filter(|v| v.is_object())
        .ok_or(Loss::Malformed)?;
    let mut losses = Vec::new();
    let summary = field(f, "summary", &mut losses, |v| string(v, 4096));
    let description = body(f.get("description"));
    let status = presence(f.get("status"), |v| named(v, false, &mut losses));
    let issue_type = presence(f.get("issuetype"), |v| named(v, false, &mut losses));
    let project = presence(f.get("project"), |v| named(v, true, &mut losses));
    for name in ["status", "issuetype", "project"] {
        if f.get(name).is_none() {
            losses.push(format!("missing:{name}"));
        } else if f.get(name).is_some_and(|v| !v.is_null() && !v.is_object()) {
            losses.push(format!("invalid:{name}"));
        }
    }
    let created = field(f, "created", &mut losses, date);
    let updated = field(f, "updated", &mut losses, date);
    let mut objects = Vec::new();
    match description {
        Ok(description) => {
            losses.extend(description.losses.clone());
            if !value_text(&summary).is_empty() {
                let text = match &description.text {
                    JiraPresence::Value(s) if !s.is_empty() => {
                        format!("{}\n\n{s}", value_text(&summary))
                    }
                    _ => value_text(&summary).into(),
                };
                let c = JiraContent {
                    tag: "issue".into(),
                    parent_issue_id: id.clone(),
                    object_id: id.clone(),
                    observed_issue_key: key.clone(),
                    fields: JiraFields::Issue {
                        summary,
                        description,
                        status,
                        issue_type,
                        project,
                        created,
                        updated,
                    },
                    losses: losses.clone(),
                };
                match make(p, c, text, interval) {
                    Ok(o) => objects.push(o),
                    Err(_) => losses.push("issue_object_unavailable".into()),
                }
            } else {
                losses.push("issue_summary_unavailable".into());
            }
        }
        Err(_) => losses.push("issue_body_unavailable".into()),
    }
    if let Some(parent) = f.get("parent") {
        if parent.is_null() {
            losses.push("null:parent".into());
        } else {
            match relation(parent, None, "parent", p, &id, &key, interval) {
                Ok(o) => objects.push(o),
                Err(_) => losses.push("invalid:parent".into()),
            }
        }
    } else {
        losses.push("missing:parent".into());
    }
    for name in ["subtasks", "issuelinks"] {
        let Some(array) = f.get(name).and_then(Value::as_array) else {
            losses.push(format!("unsupported_or_missing:{name}"));
            continue;
        };
        if array.len() > 100 {
            losses.push(format!("relation_limit:{name}"));
            continue;
        }
        for v in array {
            let result = if name == "subtasks" {
                relation(v, None, "subtask", p, &id, &key, interval)
            } else {
                (|| {
                    let link_id = opaque(v.get("id").ok_or(Loss::Malformed)?, 256)?;
                    let t = v
                        .get("type")
                        .filter(|t| t.is_object())
                        .ok_or(Loss::Malformed)?;
                    let (endpoint, direction) = match (v.get("inwardIssue"), v.get("outwardIssue"))
                    {
                        (Some(i), None) => (i, "inward"),
                        (None, Some(o)) => (o, "outward"),
                        _ => return Err(Loss::Malformed),
                    };
                    relation(
                        endpoint,
                        Some((&link_id, t)),
                        direction,
                        p,
                        &id,
                        &key,
                        interval,
                    )
                })()
            };
            match result {
                Ok(o) => objects.push(o),
                Err(_) => losses.push(format!("invalid:{name}")),
            }
        }
    }
    if f.get("sub-tasks").is_some() {
        losses.push("unverified_subtasks_shape".into());
    }
    Ok((id, key, objects, losses))
}
pub(crate) fn item(
    v: &Value,
    category: u32,
    p: &JiraScope,
    parent: &str,
    key: &str,
    interval: (i64, i64),
) -> Result<IntegrationObject, Loss> {
    let id = opaque(v.get("id").ok_or(Loss::Malformed)?, 256)?;
    let mut losses = Vec::new();
    let author = presence(v.get("author"), |v| author(v, &mut losses));
    let created = field(v, "created", &mut losses, date);
    let (tag, fields, text) = match category {
        1 => {
            let b = body(v.get("body"))?;
            losses.extend(b.losses.clone());
            let text = value_text(&b.text).to_owned();
            let updated = field(v, "updated", &mut losses, date);
            (
                "comment",
                JiraFields::Comment {
                    body: b,
                    author,
                    created,
                    updated,
                },
                text,
            )
        }
        2 => {
            let a = v
                .get("items")
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty() && a.len() <= 100)
                .ok_or(Loss::Malformed)?;
            let mut items = Vec::new();
            let mut lines = Vec::new();
            for v in a {
                if !v.is_object() {
                    return Err(Loss::Malformed);
                }
                let c = JiraChange {
                    field: field(v, "field", &mut losses, |v| string(v, 256)),
                    field_id: field(v, "fieldId", &mut losses, |v| string(v, 256)),
                    field_type: field(v, "fieldtype", &mut losses, |v| string(v, 256)),
                    from: field(v, "from", &mut losses, |v| string(v, 4096)),
                    to: field(v, "to", &mut losses, |v| string(v, 4096)),
                    from_string: field(v, "fromString", &mut losses, |v| string(v, 4096)),
                    to_string: field(v, "toString", &mut losses, |v| string(v, 4096)),
                };
                if [
                    &c.field,
                    &c.field_id,
                    &c.field_type,
                    &c.from,
                    &c.to,
                    &c.from_string,
                    &c.to_string,
                ]
                .iter()
                .any(|v| matches!(v, JiraPresence::Invalid))
                {
                    return Err(Loss::Malformed);
                }
                let parts = [
                    &c.field,
                    &c.field_id,
                    &c.field_type,
                    &c.from,
                    &c.to,
                    &c.from_string,
                    &c.to_string,
                ]
                .into_iter()
                .map(value_text)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
                if !parts.is_empty() {
                    lines.push(parts.join(" | "));
                }
                items.push(c);
            }
            (
                "changelog",
                JiraFields::Changelog {
                    author,
                    created,
                    items,
                },
                lines.join("\n"),
            )
        }
        _ => return Err(Loss::Malformed),
    };
    make(
        p,
        JiraContent {
            tag: tag.into(),
            parent_issue_id: parent.into(),
            object_id: id,
            observed_issue_key: key.into(),
            fields,
            losses,
        },
        text,
        interval,
    )
}

pub(crate) fn validate_metadata(m: &JiraMetadata) -> Result<(), StoreError> {
    fn id(s: &str, max: usize) -> bool {
        !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
    }
    fn check<T>(p: &JiraPresence<T>, f: impl FnOnce(&T) -> bool) -> bool {
        match p {
            JiraPresence::Value(v) => f(v),
            _ => true,
        }
    }
    fn s(p: &JiraPresence<String>, max: usize) -> bool {
        check(p, |v| v.len() <= max)
    }
    fn d(p: &JiraPresence<JiraDate>) -> bool {
        check(p, |v| {
            time::OffsetDateTime::from_unix_timestamp(v.utc_seconds).is_ok()
        })
    }
    fn n(p: &JiraPresence<JiraNamed>) -> bool {
        check(p, |v| {
            check(&v.id, |s| id(s, 256)) && s(&v.name, 512) && s(&v.key, 256)
        })
    }
    fn a(p: &JiraPresence<JiraAuthor>) -> bool {
        check(p, |v| {
            check(&v.account_id, |s| id(s, 128)) && s(&v.display_name, 512)
        })
    }
    fn body_ok(b: &JiraBody) -> bool {
        s(&b.text, 16384) && b.losses.len() <= 16 && b.losses.iter().all(|s| s.len() <= 128)
    }
    let c = &m.content;
    let good = match &c.fields {
        JiraFields::Issue {
            summary,
            description,
            status,
            issue_type,
            project,
            created,
            updated,
        } => {
            c.tag == "issue"
                && c.object_id == c.parent_issue_id
                && check(summary, |v| !v.is_empty() && v.len() <= 4096)
                && matches!(summary, JiraPresence::Value(_))
                && body_ok(description)
                && n(status)
                && n(issue_type)
                && n(project)
                && d(created)
                && d(updated)
        }
        JiraFields::Comment {
            body,
            author,
            created,
            updated,
        } => {
            c.tag == "comment"
                && body_ok(body)
                && matches!(&body.text,JiraPresence::Value(s) if !s.trim().is_empty())
                && a(author)
                && d(created)
                && d(updated)
        }
        JiraFields::Changelog {
            author,
            created,
            items,
        } => {
            c.tag == "changelog"
                && a(author)
                && d(created)
                && !items.is_empty()
                && items.len() <= 100
                && items.iter().all(|v| {
                    [
                        (&v.field, 256),
                        (&v.field_id, 256),
                        (&v.field_type, 256),
                        (&v.from, 4096),
                        (&v.to, 4096),
                        (&v.from_string, 4096),
                        (&v.to_string, 4096),
                    ]
                    .iter()
                    .all(|(v, max)| !matches!(v, JiraPresence::Invalid) && s(v, *max))
                })
        }
        JiraFields::Relation {
            target_id,
            target_key,
            summary,
            status,
            direction,
            label,
            link_type,
            inward,
            outward,
        } => {
            ["parent", "subtask", "issue_link"].contains(&c.tag.as_str())
                && id(target_id, 256)
                && s(target_key, 256)
                && s(summary, 4096)
                && n(status)
                && n(link_type)
                && s(inward, 512)
                && s(outward, 512)
                && id(label, 512)
                && match c.tag.as_str() {
                    "parent" => direction == "parent" && c.object_id == *target_id,
                    "subtask" => direction == "subtask" && c.object_id == *target_id,
                    _ => {
                        ["inward", "outward"].contains(&direction.as_str())
                            && label
                                == value_text(if direction == "inward" {
                                    inward
                                } else {
                                    outward
                                })
                    }
                }
        }
    };
    if !good
        || [m.profile_id, m.settings_revision, m.credential_generation]
            .iter()
            .any(|id| *id == 0 || *id > i64::MAX as u64)
        || !valid_cloud_id(&m.cloud_id)
        || !id(&c.parent_issue_id, 256)
        || !id(&c.object_id, 256)
        || !id(&c.observed_issue_key, 256)
        || m.interval_end < m.interval_start
        || m.observation != "non_atomic_observation"
        || m.current_state != "current_state_unverified"
        || c.losses.len() > 1024
        || c.losses.iter().any(|s| s.len() > 128)
        || m.coverage.len() != 4
        || m.coverage
            .iter()
            .map(|c| c.category)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != 4
        || m.coverage.iter().any(|c| {
            c.category > 3
                || !["not_read", "read", "partial"].contains(&c.status.as_str())
                || c.objects > 512
                || c.unavailable > 512
                || c.pages > 6
                || c.reasons.len() > 16
                || c.reasons.iter().any(|s| s.len() > 128)
                || c.observed_total.is_some_and(|v| v < 0)
        })
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
pub(crate) fn validate_object(o: &IntegrationObject, edited: bool) -> Result<(), StoreError> {
    let IntegrationMetadata::Jira(m) = &o.metadata else {
        return Err(StoreError::InvalidInput);
    };
    validate_metadata(m)?;
    if object_key(&m.cloud_id, &m.content).map_err(|_| StoreError::InvalidInput)? != o.key
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
                || o.observed_at_utc != source_time(&m.content.fields)))
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn jira_dates_accept_compact_offset_and_report_lost_fraction() -> Result<(), Loss> {
        let d = date(&json!("2021-01-17T12:34:00.123456789+0000"))?;
        assert_eq!(d.utc_seconds, 1610886840);
        assert!(d.precision_lost);
        assert_eq!(
            date(&json!("2021-01-17T13:34:00+01:00"))?.utc_seconds,
            d.utc_seconds
        );
        for raw in [
            "2021-02-29T12:34:00+0000",
            "2021-01-17T12:34:00.1234567890Z",
            "2021-01-17T12:34:00+2500",
        ] {
            assert_eq!(date(&json!(raw)), Err(Loss::Malformed));
        }
        Ok(())
    }
    #[test]
    fn adf_distinguishes_absence_empty_and_projects_visible_text_with_losses() -> Result<(), Loss> {
        assert_eq!(body(None)?.text, JiraPresence::Missing);
        assert_eq!(body(Some(&Value::Null))?.text, JiraPresence::Null);
        assert_eq!(
            body(Some(&json!({"type":"doc","version":1,"content":[]})))?.text,
            JiraPresence::Value(String::new())
        );
        let b = body(Some(&json!({"type":"doc","version":1,"content":[
          {"type":"paragraph","content":[{"type":"text","text":"Hello","marks":[{"type":"link","attrs":{"href":"https://invalid.example/"}}]}, {"type":"mention","attrs":{"id":"opaque"}}]},
          {"type":"future","content":[{"type":"text","text":"Visible"}]}, {"type":"inlineCard","attrs":{"url":"https://invalid.example/"}}
        ]})))?;
        let JiraPresence::Value(text) = b.text else {
            return Err(Loss::Malformed);
        };
        assert!(text.contains("Hello") && text.contains("Visible"));
        assert!(!text.contains("opaque") && !text.contains("https://"));
        for loss in [
            "link_address_omitted",
            "mention_label_missing",
            "unknown_node",
            "media_or_card_omitted",
        ] {
            assert!(b.losses.iter().any(|v| v == loss), "{loss}");
        }
        assert_eq!(
            body(Some(
                &json!({"type":"doc","version":1,"content":[{"type":"text","text":"x".repeat(16385)}]})
            )),
            Err(Loss::Oversized)
        );
        Ok(())
    }
    fn profile() -> JiraScope {
        JiraScope {
            profile: IntegrationProfileVersion {
                id: 1,
                settings_revision: 2,
                credential_generation: 3,
            },
            cloud_id: "Cloud-ID".into(),
        }
    }
    #[test]
    fn issue_presence_relations_and_hash_have_separate_identities() -> Result<(), Loss> {
        let v = json!({"id":"opaque:issue","key":"NEW-2","fields":{"summary":"Title","description":null,"status":{"name":"Custom"},"created":"2021-01-17T12:34:00+0000","subtasks":[{"id":"same","key":"CHILD-1"}],"parent":{"id":"same","key":"PARENT-1"},"issuelinks":[{"id":"same","type":{"name":"Blocks","outward":"blocks"},"outwardIssue":{"id":"target"}}]}});
        let (id, key, objects, losses) = issue(&v, &profile(), (10, 20))?;
        assert_eq!(id, "opaque:issue");
        assert_eq!(key, "NEW-2");
        assert_eq!(objects.len(), 4);
        assert_eq!(
            objects
                .iter()
                .map(|o| &o.key.object_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4
        );
        assert!(losses.iter().any(|l| l.contains("updated")));
        let mut p = profile();
        p.profile.id = 40;
        p.profile.settings_revision = 50;
        p.profile.credential_generation = 60;
        let (_, _, later, _) = issue(&v, &p, (100, 200))?;
        assert_eq!(objects[0].version, later[0].version);
        let IntegrationMetadata::Jira(m) = &objects[0].metadata else {
            return Err(Loss::Malformed);
        };
        let JiraFields::Issue {
            description,
            status,
            updated,
            ..
        } = &m.content.fields
        else {
            return Err(Loss::Malformed);
        };
        assert_eq!(description.text, JiraPresence::Null);
        assert_eq!(*updated, JiraPresence::Missing);
        let JiraPresence::Value(status) = status else {
            return Err(Loss::Malformed);
        };
        assert_eq!(status.id, JiraPresence::Missing);
        let (_, _, _, loss) = issue(
            &json!({"id":"i","key":"K","fields":{"summary":"T","sub-tasks":[]}}),
            &profile(),
            (1, 2),
        )?;
        assert!(loss.iter().any(|l| l.contains("subtasks")));
        Ok(())
    }
    #[test]
    fn changelog_uses_fieldtype_and_rejects_partial_items() -> Result<(), Loss> {
        let v = json!({"id":"change-id","created":"2021-01-17T12:34:00+0000","items":[{"field":"status","fieldtype":"jira","type":"wrong-alias","from":null,"to":"opaque"}]});
        let o = item(&v, 2, &profile(), "issue-id", "KEY-2", (1, 2))?;
        let IntegrationMetadata::Jira(m) = o.metadata else {
            return Err(Loss::Malformed);
        };
        let JiraFields::Changelog { items, .. } = m.content.fields else {
            return Err(Loss::Malformed);
        };
        assert_eq!(items[0].field_type, JiraPresence::Value("jira".into()));
        assert_eq!(items[0].from, JiraPresence::Null);
        let mut bad = v;
        bad["items"][0]["from"] = json!(17);
        assert_eq!(
            item(&bad, 2, &profile(), "issue-id", "KEY-2", (1, 2)),
            Err(Loss::Malformed)
        );
        Ok(())
    }
    #[test]
    fn adf_work_depth_and_opaque_ids_are_bounded() -> Result<(), Loss> {
        let mut node = json!({"type":"text","text":"leaf"});
        for _ in 0..65 {
            node = json!({"type":"paragraph","content":[node]});
        }
        assert_eq!(
            body(Some(&json!({"type":"doc","version":1,"content":[node]}))),
            Err(Loss::Oversized)
        );
        let marks = vec![json!({"type":"strong"}); 10000];
        assert_eq!(
            body(Some(
                &json!({"type":"doc","version":1,"content":[{"type":"text","text":"leaf","marks":marks}]})
            )),
            Err(Loss::Oversized)
        );
        for id in [
            json!(1),
            json!(null),
            json!(""),
            json!("a\u{0000}b"),
            json!("x".repeat(257)),
        ] {
            assert!(
                issue(
                    &json!({"id":id,"key":"KEY-2","fields":{"summary":"Title"}}),
                    &profile(),
                    (1, 2)
                )
                .is_err()
            );
        }
        assert_eq!(
            opaque(&json!("issue/slash:opaque"), 256)?,
            "issue/slash:opaque"
        );
        Ok(())
    }
}
