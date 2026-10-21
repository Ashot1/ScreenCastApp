//! Jira Cloud specifics for the shared integration engine (ADR-017 semantics).
use crate::jira_parser as parser;
use crate::*;
use serde_json::Value;

pub(crate) struct Jira;

fn metadata(m: &IntegrationMetadata) -> Result<&JiraMetadata, StoreError> {
    match m {
        IntegrationMetadata::Jira(m) => Ok(m),
        _ => Err(StoreError::InvalidInput),
    }
}
fn categories(w: &mut IntegrationWindow) -> Result<&mut Vec<JiraCoverage>, StoreError> {
    match &mut w.categories {
        IntegrationCoverage::Jira(c) => Ok(c),
        _ => Err(StoreError::InvalidInput),
    }
}
impl Adapter for Jira {
    fn kind(&self) -> IntegrationKind {
        IntegrationKind::Jira
    }
    fn namespace(&self) -> &'static str {
        JIRA_NAMESPACE
    }
    fn max_requests(&self) -> u32 {
        13
    }
    fn parse_target(&self, text: &str) -> Result<IntegrationTarget, StoreError> {
        parser::opaque(&Value::String(text.into()), 256).map_err(|_| StoreError::InvalidInput)?;
        Ok(IntegrationTarget::Jira(JiraTarget {
            selection: text.into(),
        }))
    }
    fn validate_scope(&self, scope: Option<&str>) -> Result<(), StoreError> {
        match scope {
            Some(cloud) if parser::valid_cloud_id(cloud) => Ok(()),
            _ => Err(StoreError::InvalidInput),
        }
    }
    fn validate_config(&self, config: &IntegrationConfig) -> Result<(), StoreError> {
        match config {
            IntegrationConfig::Jira(JiraConfig {}) => Ok(()),
            _ => Err(StoreError::InvalidInput),
        }
    }
    /// Site exclusion is the engine's; only a foreign target is refused here.
    fn admit(&self, _: &IntegrationProfile, target: &IntegrationTarget) -> Result<(), StoreError> {
        match target {
            IntegrationTarget::Jira(_) => Ok(()),
            _ => Err(StoreError::InvalidInput),
        }
    }
    fn authorized(
        &self,
        profile: &IntegrationProfile,
        saved: &IntegrationMetadata,
    ) -> Result<(), StoreError> {
        if profile.scope.as_deref() != Some(metadata(saved)?.cloud_id.as_str()) {
            return Err(StoreError::StaleRevision);
        }
        Ok(())
    }
    fn same_identity(&self, old: &IntegrationMetadata, new: &IntegrationMetadata) -> bool {
        let (Ok(a), Ok(b)) = (metadata(old), metadata(new)) else {
            return false;
        };
        a.cloud_id == b.cloud_id
            && a.content.tag == b.content.tag
            && a.content.parent_issue_id == b.content.parent_issue_id
            && a.content.object_id == b.content.object_id
    }
    fn start(
        &self,
        profile: &IntegrationProfile,
        target: &IntegrationTarget,
    ) -> Result<Started, StoreError> {
        let IntegrationTarget::Jira(target) = target else {
            return Err(StoreError::InvalidInput);
        };
        Ok(Started {
            reader: Box::new(JiraReader {
                scope: JiraScope {
                    profile: profile.version(),
                    cloud_id: profile.scope.clone().ok_or(StoreError::UnsupportedSchema)?,
                },
                selection: target.selection.clone(),
                stage: 0,
                start_at: 0,
                pages: 0,
                last_order_time: None,
                anchor: None,
                issue_key: None,
            }),
            coverage: IntegrationCoverage::Jira((0..4).map(coverage).collect()),
        })
    }
    fn pause(&self, response: &IntegrationResponse, now: i64) -> Result<Option<Pause>, StoreError> {
        let (status, headers) = (response.status, &response.headers);
        // 429 always stops; 503 stops only when the server announced (or obscured) a retry time.
        if status == 429
            || (status == 503 && (headers.retry_after.is_some() || headers.rate_ambiguous))
        {
            let until = headers.retry_after_deadline(now);
            return Ok(Some(Pause {
                until,
                known: until.is_some() && !headers.rate_ambiguous,
                failure: u32::from(status),
            }));
        }
        Ok(None)
    }
    fn validate_metadata(&self, m: &IntegrationMetadata) -> Result<(), StoreError> {
        parser::validate_metadata(metadata(m)?)
    }
    fn validate_object(&self, o: &IntegrationObject, edited: bool) -> Result<(), StoreError> {
        parser::validate_object(o, edited)
    }
}
fn coverage(category: u32) -> JiraCoverage {
    JiraCoverage {
        category,
        status: "not_read".into(),
        pages: 0,
        objects: 0,
        unavailable: 0,
        reasons: Vec::new(),
        observed_total: None,
        omitted_prefix: false,
    }
}
fn reason(c: &mut JiraCoverage, s: &str) {
    c.status = "partial".into();
    if c.reasons.len() < 16 && !c.reasons.iter().any(|r| r == s) {
        c.reasons.push(s.into());
    }
}
fn tag_category(m: &JiraMetadata) -> usize {
    match m.content.tag.as_str() {
        "issue" => 0,
        "comment" => 1,
        "changelog" => 2,
        _ => 3,
    }
}
fn add_object(
    w: &mut IntegrationWindow,
    o: IntegrationObject,
    category: usize,
) -> Result<bool, StoreError> {
    if let Some(old) = w.objects.iter_mut().find(|old| old.key == o.key) {
        let conflicting = old.version != o.version;
        if conflicting {
            old.bindable = false;
        }
        reason(
            &mut categories(w)?[category],
            if conflicting {
                "conflicting_duplicate"
            } else {
                "duplicate_object"
            },
        );
        return Ok(false);
    }
    if w.objects.len() >= 512 {
        reason(&mut categories(w)?[category], "object_limit");
        return Ok(false);
    }
    w.objects.push(o);
    if serde_json::to_vec(&w)
        .map_err(|_| StoreError::InvalidInput)?
        .len()
        > MAX_INTEGRATION_WINDOW
    {
        w.objects.pop();
        reason(&mut categories(w)?[category], "window_byte_limit");
        return Ok(false);
    }
    categories(w)?[category].objects += 1;
    Ok(true)
}
fn page(v: &Value, category: u32, start: i64, max: i32) -> Option<(&[Value], i64)> {
    let at = v.get("startAt")?.as_i64()?;
    let size = v.get("maxResults")?.as_i64()?;
    let total = v.get("total")?.as_i64()?;
    if at != start || at < 0 || size <= 0 || size > i64::from(max) || total < 0 {
        return None;
    }
    let values = v
        .get(if category == 1 { "comments" } else { "values" })?
        .as_array()?;
    if values.len() > size as usize {
        return None;
    }
    Some((values, total))
}
fn finalize_window(w: &mut IntegrationWindow, now: i64) -> Result<(), StoreError> {
    loop {
        let snapshot = categories(w)?.clone();
        let mut removed = false;
        let mut retained = Vec::new();
        for mut o in std::mem::take(&mut w.objects) {
            let IntegrationMetadata::Jira(m) = &mut o.metadata else {
                return Err(StoreError::InvalidInput);
            };
            m.interval_end = now;
            m.coverage = snapshot.clone();
            if combined_bytes(&o)? > MAX_TEXT_BYTES {
                let c = &mut categories(w)?[tag_category(metadata(&o.metadata)?)];
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
            let c = &mut categories(w)?[tag_category(metadata(&o.metadata)?)];
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

/// Stage 0 initial issue, 1 comments, 2 history probe, 3 history tail, 4 final issue.
struct JiraReader {
    scope: JiraScope,
    selection: String,
    stage: u32,
    start_at: i64,
    pages: u32,
    last_order_time: Option<i64>,
    anchor: Option<String>,
    issue_key: Option<String>,
}
impl JiraReader {
    fn category_done(&mut self) {
        self.stage = if self.stage == 1 { 2 } else { 4 };
        self.start_at = 0;
        self.pages = 0;
        self.last_order_time = None;
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
        if self.stage == 0 || self.stage == 4 {
            if response.status != 200 {
                return Ok(Step::Fail(u32::from(response.status)));
            }
            let Some(v) = parsed else {
                return Ok(Step::Fail(422));
            };
            let Ok((anchor, key, objects, losses)) =
                parser::issue(&v, &self.scope, (w.interval_start, now))
            else {
                return Ok(Step::Fail(422));
            };
            if self.stage == 0 {
                self.anchor = Some(anchor);
                self.issue_key = Some(key);
                self.stage = 1;
                return Ok(Step::Continue);
            }
            if self.anchor.as_deref() != Some(&anchor) {
                return Ok(Step::Fail(409));
            }
            categories(w)?[0].status = "read".into();
            categories(w)?[3].status = "read".into();
            for loss in losses {
                let category = if loss.contains("parent")
                    || loss.contains("subtask")
                    || loss.contains("issuelink")
                    || loss.contains("relation")
                {
                    3
                } else {
                    0
                };
                reason(&mut categories(w)?[category], &loss);
            }
            for o in objects {
                let category = if metadata(&o.metadata)?.content.tag == "issue" {
                    0
                } else {
                    3
                };
                add_object(w, o, category)?;
            }
            finalize_window(w, now)?;
            self.stage = 5;
            return Ok(Step::Done);
        }
        let category = if self.stage == 1 { 1 } else { 2 };
        if matches!(response.status, 403 | 404) {
            reason(
                &mut categories(w)?[category],
                if response.status == 403 {
                    "forbidden"
                } else {
                    "unavailable"
                },
            );
            self.category_done();
            return Ok(Step::Continue);
        }
        if response.status != 200 {
            return Ok(Step::Fail(u32::from(response.status)));
        }
        let Some(v) = parsed else {
            reason(&mut categories(w)?[category], "malformed_page");
            self.category_done();
            return Ok(Step::Continue);
        };
        let requested = if self.stage == 2 { 1 } else { 50 };
        let Some((values, total)) = page(&v, category as u32, self.start_at, requested) else {
            reason(&mut categories(w)?[category], "invalid_pagination");
            self.category_done();
            return Ok(Step::Continue);
        };
        let c = &mut categories(w)?[category];
        c.pages += 1;
        if c.status == "not_read" {
            c.status = "read".into();
        }
        if c.observed_total.is_some_and(|t| t != total) {
            reason(c, "total_changed");
        } else if c.observed_total.is_none() {
            c.observed_total = Some(total);
        }
        if values.is_empty() && self.start_at < total {
            reason(c, "empty_page");
            self.category_done();
            return Ok(Step::Continue);
        }
        if self.stage == 2 && total > 1 {
            let offset = (total - 250).max(0);
            c.omitted_prefix = offset > 0;
            if c.omitted_prefix {
                reason(c, "omitted_prefix");
            }
            if offset > i64::from(i32::MAX) {
                reason(c, "offset_limit");
                self.stage = 4;
            } else {
                self.stage = 3;
                self.start_at = offset;
                self.pages = 0;
            }
            return Ok(Step::Continue);
        }
        let mut duplicates = 0;
        let mut valid = 0;
        let parent = self.anchor.clone().ok_or(StoreError::InvalidInput)?;
        let key = self.issue_key.clone().ok_or(StoreError::InvalidInput)?;
        for value in values {
            match parser::item(
                value,
                category as u32,
                &self.scope,
                &parent,
                &key,
                (w.interval_start, now),
            ) {
                Ok(o) => {
                    let created = match &metadata(&o.metadata)?.content.fields {
                        JiraFields::Comment { created, .. }
                        | JiraFields::Changelog { created, .. } => created,
                        _ => return Err(StoreError::InvalidInput),
                    };
                    if let JiraPresence::Value(date) = created {
                        if date.precision_lost {
                            reason(&mut categories(w)?[category], "order_unknown");
                        }
                        if let Some(previous) = self.last_order_time {
                            if (category == 1 && date.utc_seconds > previous)
                                || (category == 2 && date.utc_seconds < previous)
                            {
                                reason(&mut categories(w)?[category], "unstable_order");
                            }
                            if date.utc_seconds == previous {
                                reason(&mut categories(w)?[category], "order_unknown");
                            }
                        }
                        self.last_order_time = Some(date.utc_seconds);
                    } else {
                        reason(&mut categories(w)?[category], "order_unknown");
                    }
                    if w.objects.iter().any(|old| old.key == o.key) {
                        duplicates += 1;
                        reason(&mut categories(w)?[category], "duplicate_order");
                    }
                    valid += 1;
                    add_object(w, o, category)?;
                }
                Err(loss) => {
                    let c = &mut categories(w)?[category];
                    c.unavailable += 1;
                    reason(
                        c,
                        match loss {
                            parser::Loss::Malformed => "malformed_object",
                            parser::Loss::Oversized => "oversized_object",
                            parser::Loss::Empty => "empty_object",
                        },
                    );
                }
            }
        }
        self.pages += 1;
        let Some(next) = self.start_at.checked_add(values.len() as i64) else {
            reason(&mut categories(w)?[category], "offset_limit");
            self.category_done();
            return Ok(Step::Continue);
        };
        if next > total {
            reason(&mut categories(w)?[category], "invalid_pagination");
        }
        if (category == 2 && next > i64::from(i32::MAX))
            || (values.is_empty() && next < total)
            || (valid > 0 && duplicates == valid)
        {
            reason(
                &mut categories(w)?[category],
                if values.is_empty() {
                    "empty_page"
                } else if valid > 0 && duplicates == valid {
                    "repeated_page"
                } else {
                    "offset_limit"
                },
            );
            self.category_done();
        } else if next >= total {
            self.category_done();
        } else if self.pages >= 5 {
            reason(&mut categories(w)?[category], "page_limit");
            self.category_done();
        } else {
            self.start_at = next;
        }
        Ok(Step::Continue)
    }
}
impl Reader for JiraReader {
    fn next(
        &mut self,
        reserved: bool,
        w: &mut IntegrationWindow,
    ) -> Result<IntegrationRequest, StoreError> {
        if (1..=3).contains(&self.stage) && reserved {
            let c = if self.stage == 1 { 1 } else { 2 };
            reason(&mut categories(w)?[c], "read_limit");
            self.stage = 4;
        }
        let category = match self.stage {
            0 | 4 => 0,
            1 => 1,
            _ => 2,
        };
        let issue = if self.stage == 0 {
            self.selection.clone()
        } else {
            self.anchor.clone().ok_or(StoreError::InvalidInput)?
        };
        let max_results = if self.stage == 2 { 1 } else { 50 };
        if self.start_at < 0 || (category == 2 && self.start_at > i64::from(i32::MAX)) {
            return Err(StoreError::InvalidInput);
        }
        Ok(IntegrationRequest::Jira(JiraRequest {
            cloud_id: self.scope.cloud_id.clone(),
            issue,
            category,
            start_at: if category == 0 { 0 } else { self.start_at },
            max_results,
        }))
    }
    /// Internal invariant failures map to 422, the outcome the bridge reported for them before.
    fn accept(
        &mut self,
        response: &IntegrationResponse,
        now: i64,
        w: &mut IntegrationWindow,
    ) -> Step {
        self.receive(response, now, w).unwrap_or(Step::Fail(422))
    }
}
