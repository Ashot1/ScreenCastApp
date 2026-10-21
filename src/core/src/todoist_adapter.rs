//! Todoist REST v1 specifics for the shared integration engine (ADR-018 semantics).
use crate::todoist_parser::{self as parser, Loss};
use crate::*;
use serde_json::Value;

pub(crate) struct Todoist;

/// Error bodies longer than this are cut by the transport and give no usable deadline.
const MAX_ERROR_BODY: usize = 64 * 1024;
const MAX_CURSOR: usize = 4096;
pub(crate) const MAX_COMMENT_PAGES: u32 = 5;

fn metadata(m: &IntegrationMetadata) -> Result<&TodoistMetadata, StoreError> {
    match m {
        IntegrationMetadata::Todoist(m) => Ok(m),
        _ => Err(StoreError::InvalidInput),
    }
}
fn categories(w: &mut IntegrationWindow) -> Result<&mut Vec<TodoistCoverage>, StoreError> {
    match &mut w.categories {
        IntegrationCoverage::Todoist(c) => Ok(c),
        _ => Err(StoreError::InvalidInput),
    }
}
/// The documented opaque cursor form, `<part>.<part>` of URL-safe characters.
fn valid_cursor(s: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty()
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    s.len() <= MAX_CURSOR && s.split_once('.').is_some_and(|(a, b)| part(a) && part(b))
}
/// A deadline from `error_extra.retry_after`, a non-negative integer of seconds.
fn body_deadline(field: Option<&Value>, now: i64) -> Option<i64> {
    field
        .and_then(Value::as_u64)
        .and_then(|n| i64::try_from(n).ok())
        .and_then(|n| now.checked_add(n))
}
impl Adapter for Todoist {
    fn kind(&self) -> IntegrationKind {
        IntegrationKind::Todoist
    }
    fn namespace(&self) -> &'static str {
        TODOIST_NAMESPACE
    }
    /// `/user`, the task, its project and the final task, plus the comment pages: 9.
    fn max_requests(&self) -> u32 {
        4 + MAX_COMMENT_PAGES
    }
    fn parse_target(&self, text: &str) -> Result<IntegrationTarget, StoreError> {
        if !parser::valid_object_id(text) {
            return Err(StoreError::InvalidInput);
        }
        Ok(IntegrationTarget::Todoist(TodoistTarget {
            selection: text.into(),
        }))
    }
    /// The account is unknown until the first `GET /user` writes it once (D65).
    fn validate_scope(&self, scope: Option<&str>) -> Result<(), StoreError> {
        match scope {
            Some(account) if !parser::valid_account(account) => Err(StoreError::InvalidInput),
            _ => Ok(()),
        }
    }
    fn validate_config(&self, config: &IntegrationConfig) -> Result<(), StoreError> {
        match config {
            IntegrationConfig::Todoist(TodoistConfig {}) => Ok(()),
            _ => Err(StoreError::InvalidInput),
        }
    }
    /// Account exclusion is the engine's; only a foreign target is refused here.
    fn admit(&self, _: &IntegrationProfile, target: &IntegrationTarget) -> Result<(), StoreError> {
        match target {
            IntegrationTarget::Todoist(_) => Ok(()),
            _ => Err(StoreError::InvalidInput),
        }
    }
    /// A profile without a written account authorizes nothing.
    fn authorized(
        &self,
        profile: &IntegrationProfile,
        saved: &IntegrationMetadata,
    ) -> Result<(), StoreError> {
        if profile.scope.as_deref() != Some(metadata(saved)?.account.as_str()) {
            return Err(StoreError::StaleRevision);
        }
        Ok(())
    }
    fn same_identity(&self, old: &IntegrationMetadata, new: &IntegrationMetadata) -> bool {
        let (Ok(a), Ok(b)) = (metadata(old), metadata(new)) else {
            return false;
        };
        a.account == b.account
            && std::mem::discriminant(&a.content.fields)
                == std::mem::discriminant(&b.content.fields)
            && a.content.parent_task_id == b.content.parent_task_id
            && a.content.object_id == b.content.object_id
    }
    fn start(
        &self,
        profile: &IntegrationProfile,
        target: &IntegrationTarget,
    ) -> Result<Started, StoreError> {
        let IntegrationTarget::Todoist(target) = target else {
            return Err(StoreError::InvalidInput);
        };
        Ok(Started {
            reader: Box::new(TodoistReader {
                profile: profile.version(),
                account: profile.scope.clone(),
                selection: target.selection.clone(),
                stage: Stage::User,
                anchor: None,
                cursors: Vec::new(),
            }),
            coverage: IntegrationCoverage::Todoist((0..3).map(coverage).collect()),
        })
    }
    /// D66: both the header and `error_extra.retry_after` are read; the later proved future
    /// deadline wins. A 503 pauses only when the server named (or obscured) a retry time.
    fn pause(&self, response: &IntegrationResponse, now: i64) -> Result<Option<Pause>, StoreError> {
        let (status, headers) = (response.status, &response.headers);
        if !matches!(status, 429 | 503) {
            return Ok(None);
        }
        let body = (response.body.len() <= MAX_ERROR_BODY)
            .then(|| serde_json::from_slice::<Value>(&response.body).ok())
            .flatten();
        let field = body
            .as_ref()
            .and_then(|v| v.get("error_extra"))
            .and_then(|e| e.get("retry_after"));
        if status == 503
            && headers.retry_after.is_none()
            && !headers.rate_ambiguous
            && field.is_none()
        {
            return Ok(None);
        }
        // A deadline at or before `now` proves no wait; an unknown one is never declared zero.
        let future = |t: Option<i64>| t.filter(|t| *t > now);
        let until =
            future(headers.retry_after_deadline(now)).max(future(body_deadline(field, now)));
        Ok(Some(Pause {
            until,
            known: until.is_some(),
            failure: u32::from(status),
        }))
    }
    fn validate_metadata(&self, m: &IntegrationMetadata) -> Result<(), StoreError> {
        parser::validate_metadata(metadata(m)?)
    }
    fn validate_object(&self, o: &IntegrationObject, edited: bool) -> Result<(), StoreError> {
        parser::validate_object(o, edited)
    }
}
fn coverage(category: u32) -> TodoistCoverage {
    TodoistCoverage {
        category,
        status: "not_read".into(),
        pages: 0,
        objects: 0,
        unavailable: 0,
        reasons: Vec::new(),
    }
}
fn reason(c: &mut TodoistCoverage, s: &str) {
    c.status = "partial".into();
    if c.reasons.len() < 16 && !c.reasons.iter().any(|r| r == s) {
        c.reasons.push(s.into());
    }
}
fn unavailable(w: &mut IntegrationWindow, category: usize, s: &str) -> Result<(), StoreError> {
    let c = &mut categories(w)?[category];
    c.unavailable += 1;
    reason(c, s);
    Ok(())
}
fn category(m: &TodoistMetadata) -> usize {
    match m.content.fields {
        TodoistFields::Task(_) => 0,
        TodoistFields::Project { .. } => 1,
        TodoistFields::Comment { .. } => 2,
    }
}
fn loss_reason(loss: Loss) -> &'static str {
    match loss {
        Loss::Malformed => "malformed_object",
        Loss::Oversized => "oversized_object",
        Loss::Empty => "empty_object",
        Loss::Deleted => "deleted_object",
        Loss::AttachmentOnly => "attachment_only",
    }
}
/// A repeated ID keeps one object; differing content makes it unsafe to bind.
fn add_object(w: &mut IntegrationWindow, o: IntegrationObject) -> Result<(), StoreError> {
    let at = category(metadata(&o.metadata)?);
    if let Some(old) = w.objects.iter_mut().find(|old| old.key == o.key) {
        let conflicting = old.version != o.version;
        if conflicting {
            old.bindable = false;
        }
        reason(
            &mut categories(w)?[at],
            if conflicting {
                "conflicting_duplicate"
            } else {
                "duplicate_object"
            },
        );
        return Ok(());
    }
    if w.objects.len() >= 512 {
        reason(&mut categories(w)?[at], "object_limit");
        return Ok(());
    }
    w.objects.push(o);
    if serde_json::to_vec(&w)
        .map_err(|_| StoreError::InvalidInput)?
        .len()
        > MAX_INTEGRATION_WINDOW
    {
        w.objects.pop();
        reason(&mut categories(w)?[at], "window_byte_limit");
        return Ok(());
    }
    categories(w)?[at].objects += 1;
    Ok(())
}
/// Stamps the final interval and coverage into each object, dropping what no longer fits.
fn finalize_window(w: &mut IntegrationWindow, now: i64) -> Result<(), StoreError> {
    loop {
        let snapshot = categories(w)?.clone();
        let mut removed = false;
        let mut retained = Vec::new();
        for mut o in std::mem::take(&mut w.objects) {
            let IntegrationMetadata::Todoist(m) = &mut o.metadata else {
                return Err(StoreError::InvalidInput);
            };
            m.interval_end = now;
            m.coverage = snapshot.clone();
            let at = category(m);
            if combined_bytes(&o)? > MAX_TEXT_BYTES {
                let c = &mut categories(w)?[at];
                reason(c, "metadata_limit");
                c.unavailable += 1;
                c.objects -= 1;
                removed = true;
            } else {
                retained.push(o);
            }
        }
        w.objects = retained;
        if serde_json::to_vec(&w)
            .map_err(|_| StoreError::InvalidInput)?
            .len()
            > MAX_INTEGRATION_WINDOW
        {
            let o = w.objects.pop().ok_or(StoreError::ResourceLimit)?;
            let c = &mut categories(w)?[category(metadata(&o.metadata)?)];
            reason(c, "window_byte_limit");
            c.unavailable += 1;
            c.objects -= 1;
            removed = true;
        }
        if !removed {
            break;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    User,
    Task,
    Project,
    Comments,
    FinalTask,
    Done,
}
/// The selected task's identity: its ID and project ID must not change during the read.
struct Anchor {
    task: String,
    project: String,
}
struct TodoistReader {
    profile: IntegrationProfileVersion,
    /// The profile's account: written before the read, or pinned by its first `/user`.
    account: Option<String>,
    selection: String,
    stage: Stage,
    anchor: Option<Anchor>,
    /// Comment cursors already requested; the last one is the next page's.
    cursors: Vec<String>,
}
impl TodoistReader {
    fn scope(&self) -> Result<TodoistScope, StoreError> {
        Ok(TodoistScope {
            profile: self.profile,
            account: self.account.clone().ok_or(StoreError::InvalidInput)?,
        })
    }
    fn anchor(&self) -> Result<&Anchor, StoreError> {
        self.anchor.as_ref().ok_or(StoreError::InvalidInput)
    }
    /// The initial and final task must be the selected, active task of the same project; the
    /// error is the operation outcome.
    fn task(&self, v: &Value, expected: &str) -> Result<(TodoistContent, String), u32> {
        let c = parser::task(v).map_err(|_| 422_u32)?;
        let TodoistFields::Task(t) = &c.fields else {
            return Err(422);
        };
        let project = t.project_id.clone();
        if c.object_id != expected || self.anchor.as_ref().is_some_and(|a| a.project != project) {
            return Err(409);
        }
        match (&t.checked, &t.is_deleted) {
            (TodoistPresence::Value(false), TodoistPresence::Value(false)) => Ok((c, project)),
            (TodoistPresence::Value(_), TodoistPresence::Value(_)) => Err(409),
            _ => Err(422),
        }
    }
    fn receive(
        &mut self,
        response: &IntegrationResponse,
        now: i64,
        w: &mut IntegrationWindow,
    ) -> Result<Step, StoreError> {
        let parsed = if response.status == 200 {
            serde_json::from_slice::<Value>(&response.body).ok()
        } else {
            None
        };
        match self.stage {
            Stage::User => {
                if response.status != 200 {
                    return Ok(Step::Fail(u32::from(response.status)));
                }
                // Only the ID is used; the rest of `/user` (token, email, settings) is dropped here.
                let Some(id) = parsed
                    .as_ref()
                    .and_then(|v| v.get("id"))
                    .and_then(Value::as_str)
                else {
                    return Ok(Step::Fail(422));
                };
                self.stage = Stage::Task;
                match self.account.as_deref() {
                    // A pinned profile accepts only its own account.
                    Some(account) if account != id => Ok(Step::Fail(409)),
                    Some(_) => Ok(Step::Continue),
                    None => {
                        self.account = Some(id.into());
                        Ok(Step::Pin(id.into()))
                    }
                }
            }
            Stage::Task | Stage::FinalTask => {
                if response.status != 200 {
                    return Ok(Step::Fail(u32::from(response.status)));
                }
                let Some(v) = parsed else {
                    return Ok(Step::Fail(422));
                };
                let expected = if self.stage == Stage::Task {
                    self.selection.clone()
                } else {
                    self.anchor()?.task.clone()
                };
                let (c, project) = match self.task(&v, &expected) {
                    Ok(task) => task,
                    Err(code) => return Ok(Step::Fail(code)),
                };
                if self.stage == Stage::Task {
                    self.anchor = Some(Anchor {
                        task: c.object_id.clone(),
                        project,
                    });
                    self.stage = Stage::Project;
                    return Ok(Step::Continue);
                }
                // The final task replaces the initial one; project and comments stay separate observations.
                categories(w)?[0].status = "read".into();
                categories(w)?[0].pages = 1;
                match parser::object(&self.scope()?, c, (w.interval_start, now)) {
                    Ok(o) => add_object(w, o)?,
                    Err(loss) => unavailable(w, 0, loss_reason(loss))?,
                }
                finalize_window(w, now)?;
                self.stage = Stage::Done;
                Ok(Step::Done)
            }
            Stage::Project => {
                self.stage = Stage::Comments;
                if self.category_refused(response.status, 1, w)? {
                    return Ok(Step::Continue);
                }
                if response.status != 200 {
                    return Ok(Step::Fail(u32::from(response.status)));
                }
                let anchor = self.anchor()?;
                let c = &mut categories(w)?[1];
                c.status = "read".into();
                c.pages = 1;
                let content = match parsed.as_ref().map(|v| parser::project(v, &anchor.task)) {
                    Some(Ok(content)) => content,
                    _ => {
                        unavailable(w, 1, "malformed_object")?;
                        return Ok(Step::Continue);
                    }
                };
                if content.object_id != anchor.project {
                    unavailable(w, 1, "identity_mismatch")?;
                    return Ok(Step::Continue);
                }
                match parser::object(&self.scope()?, content, (w.interval_start, now)) {
                    Ok(o) => add_object(w, o)?,
                    Err(loss) => unavailable(w, 1, loss_reason(loss))?,
                }
                Ok(Step::Continue)
            }
            Stage::Comments => self.comments(response, parsed, now, w),
            Stage::Done => Ok(Step::Fail(422)),
        }
    }
    /// 403/404 of a category leaves the other categories usable; it does not prove deletion.
    fn category_refused(
        &mut self,
        status: u16,
        category: usize,
        w: &mut IntegrationWindow,
    ) -> Result<bool, StoreError> {
        if !matches!(status, 403 | 404) {
            return Ok(false);
        }
        reason(
            &mut categories(w)?[category],
            if status == 403 {
                "forbidden"
            } else {
                "unavailable"
            },
        );
        Ok(true)
    }
    fn comments(
        &mut self,
        response: &IntegrationResponse,
        parsed: Option<Value>,
        now: i64,
        w: &mut IntegrationWindow,
    ) -> Result<Step, StoreError> {
        if self.category_refused(response.status, 2, w)? {
            self.stage = Stage::FinalTask;
            return Ok(Step::Continue);
        }
        if response.status != 200 {
            return Ok(Step::Fail(u32::from(response.status)));
        }
        let Some(results) = parsed
            .as_ref()
            .and_then(|v| v.get("results"))
            .and_then(Value::as_array)
        else {
            reason(&mut categories(w)?[2], "malformed_page");
            self.stage = Stage::FinalTask;
            return Ok(Step::Continue);
        };
        let c = &mut categories(w)?[2];
        c.pages += 1;
        if c.status == "not_read" {
            c.status = "read".into();
        }
        let task = self.anchor()?.task.clone();
        let scope = self.scope()?;
        let mut repeated = 0;
        for value in results {
            match parser::comment(value, &task)
                .and_then(|content| parser::object(&scope, content, (w.interval_start, now)))
            {
                Ok(o) => {
                    if w.objects.iter().any(|old| old.key == o.key) {
                        repeated += 1;
                    }
                    add_object(w, o)?;
                }
                Err(loss) => unavailable(w, 2, loss_reason(loss))?,
            }
        }
        let next = parsed.as_ref().and_then(|v| v.get("next_cursor"));
        let c = &mut categories(w)?[2];
        let stop = match next {
            Some(Value::Null) => None,
            Some(Value::String(cursor)) if valid_cursor(cursor) => {
                if self.cursors.contains(cursor) {
                    Some("repeated_cursor")
                } else if results.is_empty() {
                    Some("empty_page")
                } else if repeated == results.len() {
                    Some("repeated_page")
                } else if c.pages >= MAX_COMMENT_PAGES {
                    Some("page_limit")
                } else {
                    self.cursors.push(cursor.clone());
                    return Ok(Step::Continue);
                }
            }
            _ => Some("invalid_pagination"),
        };
        if let Some(stop) = stop {
            reason(c, stop);
        }
        self.stage = Stage::FinalTask;
        Ok(Step::Continue)
    }
}
impl Reader for TodoistReader {
    fn next(
        &mut self,
        reserved: bool,
        w: &mut IntegrationWindow,
    ) -> Result<IntegrationRequest, StoreError> {
        // Category steps stop at the engine's reserve; the final task check still runs.
        if reserved && matches!(self.stage, Stage::Project | Stage::Comments) {
            let c = if self.stage == Stage::Project { 1 } else { 2 };
            reason(&mut categories(w)?[c], "read_limit");
            self.stage = Stage::FinalTask;
        }
        Ok(IntegrationRequest::Todoist(match self.stage {
            Stage::User => TodoistRequest::User,
            Stage::Task => TodoistRequest::Task {
                id: self.selection.clone(),
            },
            Stage::Project => TodoistRequest::Project {
                id: self.anchor()?.project.clone(),
            },
            Stage::Comments => TodoistRequest::Comments {
                task_id: self.anchor()?.task.clone(),
                cursor: self.cursors.last().cloned(),
            },
            Stage::FinalTask => TodoistRequest::Task {
                id: self.anchor()?.task.clone(),
            },
            Stage::Done => return Err(StoreError::InvalidInput),
        }))
    }
    /// Internal invariant failures map to 422, as for the other adapters.
    fn accept(
        &mut self,
        response: &IntegrationResponse,
        now: i64,
        w: &mut IntegrationWindow,
    ) -> Step {
        self.receive(response, now, w).unwrap_or(Step::Fail(422))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(
        status: u16,
        retry: Option<&str>,
        ambiguous: bool,
        body: &[u8],
    ) -> IntegrationResponse {
        IntegrationResponse {
            status,
            body: body.to_vec(),
            headers: IntegrationHeaders {
                retry_after: retry.map(str::to_owned),
                rate_ambiguous: ambiguous,
                ..IntegrationHeaders::default()
            },
        }
    }
    fn extra(value: &str) -> Vec<u8> {
        format!(r#"{{"error":"x","error_extra":{{"retry_after":{value}}}}}"#).into_bytes()
    }
    fn pause(r: &IntegrationResponse) -> Result<Option<(Option<i64>, bool, u32)>, StoreError> {
        Ok(Todoist
            .pause(r, 1000)?
            .map(|p| (p.until, p.known, p.failure)))
    }

    /// D66: header and body are both read and the later proved future deadline wins.
    #[test]
    fn rate_limit_pause_takes_the_later_future_deadline_of_header_and_body()
    -> Result<(), StoreError> {
        let date = "Thu, 01 Jan 1970 00:20:00 GMT"; // 1200
        let past = "Thu, 01 Jan 1970 00:10:00 GMT"; // 600
        for (r, expected) in [
            (response(429, Some("30"), false, b""), (Some(1030), true)),
            (response(429, None, false, &extra("60")), (Some(1060), true)),
            (
                response(429, Some("90"), false, &extra("60")),
                (Some(1090), true),
            ),
            (
                response(429, Some("30"), false, &extra("60")),
                (Some(1060), true),
            ),
            (
                response(429, Some(date), false, &extra("60")),
                (Some(1200), true),
            ),
            (response(429, None, false, b""), (None, false)),
            // A deadline at or before now proves nothing; the other source still counts.
            (response(429, Some("0"), false, b""), (None, false)),
            (response(429, Some(past), false, &extra("0")), (None, false)),
            (
                response(429, Some("0"), false, &extra("45")),
                (Some(1045), true),
            ),
            // An unusable source never cancels the other one.
            (
                response(429, Some("soon"), false, &extra("45")),
                (Some(1045), true),
            ),
            (
                response(429, Some("45"), false, &extra("\"60\"")),
                (Some(1045), true),
            ),
            (
                response(429, Some("45"), false, &extra("-5")),
                (Some(1045), true),
            ),
            (
                response(429, Some("45"), false, &extra("1.5")),
                (Some(1045), true),
            ),
            (
                response(429, Some("45"), false, &extra("99999999999999999999")),
                (Some(1045), true),
            ),
            (
                response(429, Some("45"), false, b"{not json"),
                (Some(1045), true),
            ),
            (response(429, None, true, &extra("45")), (Some(1045), true)),
            (response(429, None, true, b""), (None, false)),
            // Overflow of the body deadline leaves it unknown, as for Jira's header.
            (
                response(429, None, false, &extra(&i64::MAX.to_string())),
                (None, false),
            ),
        ] {
            assert_eq!(
                pause(&r)?,
                Some((expected.0, expected.1, 429)),
                "{:?}",
                r.headers
            );
        }
        Ok(())
    }

    #[test]
    fn unavailable_pauses_only_with_a_header_an_ambiguous_header_or_a_body_key()
    -> Result<(), StoreError> {
        for (r, expected) in [
            (
                response(503, Some("30"), false, b""),
                Some((Some(1030), true, 503)),
            ),
            (response(503, None, true, b""), Some((None, false, 503))),
            (
                response(503, None, false, &extra("20")),
                Some((Some(1020), true, 503)),
            ),
            // The key alone makes it a pause, even with an unusable value.
            (
                response(503, None, false, &extra("null")),
                Some((None, false, 503)),
            ),
            (
                response(503, None, false, &extra("\"x\"")),
                Some((None, false, 503)),
            ),
            (response(503, None, false, br#"{"error":"down"}"#), None),
            (response(503, None, false, b""), None),
            (response(500, Some("30"), false, &extra("30")), None),
            (response(200, Some("30"), false, &extra("30")), None),
        ] {
            assert_eq!(pause(&r)?, expected, "{:?}", r.headers);
        }
        Ok(())
    }

    #[test]
    fn an_error_body_longer_than_64_kib_gives_no_deadline() -> Result<(), StoreError> {
        let mut body = extra("60");
        body.resize(MAX_ERROR_BODY + 1, b' ');
        assert_eq!(
            pause(&response(429, Some("10"), false, &body))?,
            Some((Some(1010), true, 429))
        );
        assert_eq!(pause(&response(503, None, false, &body))?, None);
        body.truncate(MAX_ERROR_BODY);
        assert_eq!(
            pause(&response(429, Some("10"), false, &body))?,
            Some((Some(1060), true, 429))
        );
        Ok(())
    }
}
