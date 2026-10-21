//! GitHub.com specifics for the shared integration engine (ADR-016 semantics).
use crate::github_parser::{self as parser, Loss};
use crate::*;
use serde_json::Value;

pub(crate) struct GitHub;

fn metadata(m: &IntegrationMetadata) -> Result<&GitHubMetadata, StoreError> {
    match m {
        IntegrationMetadata::GitHub(m) => Ok(m),
        _ => Err(StoreError::InvalidInput),
    }
}
fn repositories(c: &IntegrationConfig) -> Result<&[GitHubRepository], StoreError> {
    match c {
        IntegrationConfig::GitHub(r) => Ok(r),
        _ => Err(StoreError::InvalidInput),
    }
}
fn target(t: &IntegrationTarget) -> Result<&GitHubTarget, StoreError> {
    match t {
        IntegrationTarget::GitHub(t) => Ok(t),
        _ => Err(StoreError::InvalidInput),
    }
}
fn categories(w: &mut IntegrationWindow) -> Result<&mut Vec<GitHubCoverage>, StoreError> {
    match &mut w.categories {
        IntegrationCoverage::GitHub(c) => Ok(c),
        _ => Err(StoreError::InvalidInput),
    }
}
fn allowlisted(repositories: &[GitHubRepository], owner: &str, repository: &str) -> bool {
    repositories.iter().any(|r| {
        r.owner.eq_ignore_ascii_case(owner) && r.repository.eq_ignore_ascii_case(repository)
    })
}
/// Only `https://github.com/{owner}/{repository}/pull/{positive number}` in canonical form.
fn parse_url(raw: &str) -> Result<GitHubTarget, StoreError> {
    let u = url::Url::parse(raw).map_err(|_| StoreError::InvalidInput)?;
    if raw.len() > 512
        || u.scheme() != "https"
        || u.host_str() != Some("github.com")
        || u.port().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err(StoreError::InvalidInput);
    }
    let p = u.path().split('/').collect::<Vec<_>>();
    if p.len() != 5
        || !p[0].is_empty()
        || p[3] != "pull"
        || !parser::valid_segment(p[1])
        || !parser::valid_segment(p[2])
    {
        return Err(StoreError::InvalidInput);
    }
    let n = p[4]
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or(StoreError::InvalidInput)?;
    if p[4] != n.to_string() || raw != format!("https://github.com/{}/{}/pull/{n}", p[1], p[2]) {
        return Err(StoreError::InvalidInput);
    }
    Ok(GitHubTarget {
        repository: GitHubRepository {
            owner: p[1].into(),
            repository: p[2].into(),
        },
        number: n,
    })
}
impl Adapter for GitHub {
    fn kind(&self) -> IntegrationKind {
        IntegrationKind::GitHub
    }
    fn namespace(&self) -> &'static str {
        GITHUB_NAMESPACE
    }
    fn max_requests(&self) -> u32 {
        17
    }
    fn parse_target(&self, text: &str) -> Result<IntegrationTarget, StoreError> {
        parse_url(text).map(IntegrationTarget::GitHub)
    }
    fn validate_scope(&self, scope: Option<&str>) -> Result<(), StoreError> {
        match scope {
            None => Ok(()),
            Some(_) => Err(StoreError::InvalidInput),
        }
    }
    fn validate_config(&self, config: &IntegrationConfig) -> Result<(), StoreError> {
        let repositories = repositories(config)?;
        if repositories.len() > 32 {
            return Err(StoreError::InvalidInput);
        }
        let mut seen = std::collections::BTreeSet::new();
        for r in repositories {
            if !parser::valid_segment(&r.owner)
                || !parser::valid_segment(&r.repository)
                || !seen.insert(format!("{}/{}", r.owner, r.repository).to_ascii_lowercase())
            {
                return Err(StoreError::InvalidInput);
            }
        }
        Ok(())
    }
    fn admit(
        &self,
        profile: &IntegrationProfile,
        target: &IntegrationTarget,
    ) -> Result<(), StoreError> {
        let t = self::target(target)?;
        if t.number <= 0
            || !allowlisted(
                repositories(&profile.config)?,
                &t.repository.owner,
                &t.repository.repository,
            )
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(())
    }
    fn authorized(
        &self,
        profile: &IntegrationProfile,
        saved: &IntegrationMetadata,
    ) -> Result<(), StoreError> {
        let full = &metadata(saved)?.repository_full_name;
        let allowed = repositories(&profile.config)?
            .iter()
            .any(|r| format!("{}/{}", r.owner, r.repository).eq_ignore_ascii_case(full));
        if !allowed {
            return Err(StoreError::StaleRevision);
        }
        Ok(())
    }
    fn same_identity(&self, old: &IntegrationMetadata, new: &IntegrationMetadata) -> bool {
        let (Ok(a), Ok(b)) = (metadata(old), metadata(new)) else {
            return false;
        };
        std::mem::discriminant(&a.fields) == std::mem::discriminant(&b.fields)
            && a.repository_full_name == b.repository_full_name
            && a.parent_pr_id == b.parent_pr_id
            && a.pr_number == b.pr_number
    }
    fn start(
        &self,
        profile: &IntegrationProfile,
        target: &IntegrationTarget,
    ) -> Result<Started, StoreError> {
        let t = self::target(target)?;
        Ok(Started {
            reader: Box::new(GitHubReader {
                profile: profile.version(),
                repository: t.repository.clone(),
                number: t.number,
                stage: 0,
                page: 1,
                anchor: None,
            }),
            coverage: IntegrationCoverage::GitHub(
                (1..=3)
                    .map(|category| GitHubCoverage {
                        category,
                        status: "not_read".into(),
                        pages: 0,
                        objects: 0,
                        malformed: 0,
                        empty: 0,
                        oversized: 0,
                        ambiguous: 0,
                    })
                    .collect(),
            ),
        })
    }
    /// Any 403/429 or an exhausted quota stops the read with at least a 60 second pause; a
    /// secondary limit does not always carry distinguishing headers.
    fn pause(&self, response: &IntegrationResponse, now: i64) -> Result<Option<Pause>, StoreError> {
        let (status, headers) = (response.status, &response.headers);
        if !matches!(status, 403 | 429) && headers.remaining.as_deref().and_then(decimal) != Some(0)
        {
            return Ok(None);
        }
        let (until, known) = cooldown(headers, now)?;
        Ok(Some(Pause {
            until: Some(until),
            known,
            failure: if status == 403 { 403 } else { 429 },
        }))
    }
    fn validate_metadata(&self, m: &IntegrationMetadata) -> Result<(), StoreError> {
        parser::validate_metadata(metadata(m)?)
    }
    fn validate_object(&self, o: &IntegrationObject, edited: bool) -> Result<(), StoreError> {
        parser::validate_object(o, edited)
    }
}
fn decimal(raw: &str) -> Option<i64> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}
/// Only an overflow of the 60 second minimum is an error; an unusable header value keeps the
/// minimum and marks the deadline as not known.
fn cooldown(h: &IntegrationHeaders, now: i64) -> Result<(i64, bool), StoreError> {
    let mut until = now.checked_add(60).ok_or(StoreError::InvalidInput)?;
    let mut has_deadline = false;
    let mut trustworthy = !h.rate_ambiguous;
    if h.retry_after.is_some() {
        if let Some(t) = h.retry_after_deadline(now) {
            until = until.max(t);
            has_deadline = true;
        } else {
            trustworthy = false;
        }
    }
    let remaining = h.remaining.as_deref().and_then(decimal);
    let reset = h.reset.as_deref().and_then(decimal);
    if (h.remaining.is_some() && remaining.is_none()) || (h.reset.is_some() && reset.is_none()) {
        trustworthy = false;
    }
    // Each accepted deadline stands independently; an unrelated invalid header must never shorten it.
    if remaining == Some(0)
        && let Some(t) = reset
    {
        until = until.max(t);
        has_deadline = true;
    }
    Ok((until, has_deadline && trustworthy))
}
/// Validates `rel=next` against the exact current category path; the next page is rebuilt
/// from validated segments, never sent as a raw URL.
fn next_page(
    link: Option<&str>,
    ambiguous: bool,
    repo: &GitHubRepository,
    number: i64,
    category: u32,
    page: u32,
) -> Result<Option<u32>, StoreError> {
    if ambiguous {
        return Err(StoreError::InvalidInput);
    }
    let Some(link) = link else {
        return Ok(None);
    };
    if link.len() > 8192 {
        return Err(StoreError::InvalidInput);
    }
    let mut next = None;
    for part in link.split(',') {
        let mut pieces = part.trim().split(';');
        let target = pieces.next().ok_or(StoreError::InvalidInput)?.trim();
        let rels = pieces
            .filter_map(|s| s.trim().strip_prefix("rel="))
            .collect::<Vec<_>>();
        if !rels
            .iter()
            .any(|s| s.trim_matches('"').split_whitespace().any(|r| r == "next"))
        {
            continue;
        }
        if next.is_some() || rels.len() != 1 {
            return Err(StoreError::InvalidInput);
        }
        let raw = target
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .ok_or(StoreError::InvalidInput)?;
        let u = url::Url::parse(raw).map_err(|_| StoreError::InvalidInput)?;
        let path = if category == 3 {
            format!(
                "/repos/{}/{}/issues/{number}/comments",
                repo.owner, repo.repository
            )
        } else {
            format!(
                "/repos/{}/{}/pulls/{number}/{}",
                repo.owner,
                repo.repository,
                if category == 1 { "reviews" } else { "comments" }
            )
        };
        if u.scheme() != "https"
            || u.host_str() != Some("api.github.com")
            || u.port_or_known_default() != Some(443)
            || !u.username().is_empty()
            || u.password().is_some()
            || u.fragment().is_some()
            || u.path() != path
        {
            return Err(StoreError::InvalidInput);
        }
        let pairs = u.query_pairs().collect::<Vec<_>>();
        if pairs.len() != 2
            || pairs
                .iter()
                .filter(|(k, v)| k == "per_page" && v == "100")
                .count()
                != 1
            || pairs
                .iter()
                .filter(|(k, v)| k == "page" && v == &(page + 1).to_string())
                .count()
                != 1
        {
            return Err(StoreError::InvalidInput);
        }
        next = Some(page + 1);
    }
    Ok(next)
}

/// Stage 0 initial PR, 1 reviews, 2 inline comments, 3 general comments, 4 final PR, 5 done.
struct GitHubReader {
    profile: IntegrationProfileVersion,
    repository: GitHubRepository,
    number: i64,
    stage: u32,
    page: u32,
    anchor: Option<GitHubAnchor>,
}
impl GitHubReader {
    fn category_done(&mut self) {
        self.stage += 1;
        self.page = 1;
    }
    /// Window limits stop further category reads; only the final identity GET remains.
    fn limit(&mut self, c: &mut GitHubCoverage) {
        c.status = "limit".into();
        self.stage = 4;
        self.page = 1;
    }
    fn receive(
        &mut self,
        response: &IntegrationResponse,
        now: i64,
        w: &mut IntegrationWindow,
    ) -> Result<Step, StoreError> {
        let interval = (w.interval_start, now);
        if self.stage == 0 || self.stage == 4 {
            if response.status != 200 {
                return Ok(Step::Fail(u32::from(response.status)));
            }
            let Ok(v) = serde_json::from_slice::<Value>(&response.body) else {
                return Ok(Step::Fail(422));
            };
            let Ok((o, anchor)) = parser::normalize(
                &v,
                0,
                self.profile,
                &self.repository,
                self.number,
                None,
                interval,
            ) else {
                return Ok(Step::Fail(422));
            };
            if self.stage == 0 {
                self.anchor = anchor;
                w.objects.push(o);
                self.stage = 1;
                return Ok(Step::Continue);
            }
            if self.anchor != anchor {
                return Ok(Step::Fail(409));
            }
            // The description comes from the final response, not the first anchor read.
            *w.objects.first_mut().ok_or(StoreError::InvalidInput)? = o;
            self.stage = 5;
            for o in &mut w.objects {
                let IntegrationMetadata::GitHub(m) = &mut o.metadata else {
                    return Err(StoreError::InvalidInput);
                };
                m.interval_end = now;
            }
            if serde_json::to_vec(&w)
                .map_err(|_| StoreError::InvalidInput)?
                .len()
                > MAX_INTEGRATION_WINDOW
            {
                return Ok(Step::Fail(413));
            }
            return Ok(Step::Done);
        }
        let index = (self.stage - 1) as usize;
        if response.status == 404 {
            categories(w)?[index].status = "unavailable".into();
            self.category_done();
            return Ok(Step::Continue);
        }
        if response.status != 200 {
            return Ok(Step::Fail(u32::from(response.status)));
        }
        let parsed = serde_json::from_slice::<Value>(&response.body);
        let values = match &parsed {
            Ok(Value::Array(v)) if v.len() <= 100 => v,
            _ => {
                let c = &mut categories(w)?[index];
                c.status = "malformed".into();
                c.malformed += 1;
                self.category_done();
                return Ok(Step::Continue);
            }
        };
        let c = &mut categories(w)?[index];
        c.pages += 1;
        if c.status == "not_read" {
            c.status = "read".into();
        }
        for v in values {
            let item = parser::normalize(
                v,
                self.stage,
                self.profile,
                &self.repository,
                self.number,
                self.anchor.as_ref(),
                interval,
            );
            let o = match item {
                Err(loss) => {
                    let c = &mut categories(w)?[index];
                    match loss {
                        Loss::Malformed => c.malformed += 1,
                        Loss::Empty => c.empty += 1,
                        Loss::Oversized => c.oversized += 1,
                    }
                    c.status = "partial".into();
                    continue;
                }
                Ok((o, _)) => o,
            };
            if let Some(prior) = w.objects.iter_mut().find(|old| old.key == o.key) {
                if prior.version != o.version && prior.bindable {
                    prior.bindable = false;
                    let c = &mut categories(w)?[index];
                    c.ambiguous += 1;
                    c.status = "partial".into();
                }
                continue;
            }
            if w.objects.len() >= 512 {
                self.limit(&mut categories(w)?[index]);
                break;
            }
            w.objects.push(o);
            if serde_json::to_vec(&w)
                .map_err(|_| StoreError::InvalidInput)?
                .len()
                > MAX_INTEGRATION_WINDOW
            {
                w.objects.pop();
                self.limit(&mut categories(w)?[index]);
                break;
            }
            categories(w)?[index].objects += 1;
        }
        if self.stage == 4 {
            return Ok(Step::Continue);
        }
        let next = next_page(
            response.headers.link.as_deref(),
            response.headers.link_ambiguous,
            &self.repository,
            self.number,
            self.stage,
            self.page,
        );
        let c = &mut categories(w)?[index];
        match next {
            Ok(Some(next)) if self.page < 5 => self.page = next,
            Ok(Some(_)) => {
                c.status = "page_limit".into();
                self.category_done();
            }
            Ok(None) => self.category_done(),
            Err(_) => {
                c.status = "pagination".into();
                self.category_done();
            }
        }
        Ok(Step::Continue)
    }
}
impl Reader for GitHubReader {
    fn next(
        &mut self,
        reserved: bool,
        w: &mut IntegrationWindow,
    ) -> Result<IntegrationRequest, StoreError> {
        if (1..=3).contains(&self.stage) && reserved {
            let index = (self.stage - 1) as usize;
            self.limit(&mut categories(w)?[index]);
        }
        if self.stage > 4 {
            return Err(StoreError::InvalidInput);
        }
        Ok(IntegrationRequest::GitHub(GitHubRequest {
            owner: self.repository.owner.clone(),
            repository: self.repository.repository.clone(),
            number: self.number,
            category: if self.stage == 4 { 0 } else { self.stage },
            page: self.page,
        }))
    }
    /// Internal invariant failures map to 422, the outcome reported for them before.
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

    fn version() -> IntegrationProfileVersion {
        IntegrationProfileVersion {
            id: 1,
            settings_revision: 1,
            credential_generation: 1,
        }
    }
    fn profile(config: IntegrationConfig, scope: Option<&str>) -> IntegrationProfile {
        IntegrationProfile {
            id: 1,
            kind: config.kind(),
            settings_revision: 1,
            credential_generation: 1,
            name: "Work".into(),
            scope: scope.map(str::to_owned),
            config,
            not_before_utc: None,
            server_deadline_known: false,
        }
    }
    fn window(categories: IntegrationCoverage) -> IntegrationWindow {
        IntegrationWindow {
            observation_id: 1,
            interval_start: 1,
            interval_end: 1,
            categories,
            objects: Vec::new(),
        }
    }
    struct Sample {
        profile: IntegrationProfile,
        target: IntegrationTarget,
        object: IntegrationObject,
        coverage: IntegrationCoverage,
        /// Responses that take a reader past its identity reads.
        initial: Vec<serde_json::Value>,
    }
    fn github() -> Result<Sample, Box<dyn std::error::Error>> {
        let repo = GitHubRepository {
            owner: "Example".into(),
            repository: "Project".into(),
        };
        let pr = serde_json::json!({"id":20,"number":3,"title":"T","body":null,"user":{"login":"a"},"created_at":"2026-10-20T01:00:00Z","updated_at":"2026-10-20T02:00:00Z","state":"open","merged":false,"head":{"sha":"h"},"base":{"sha":"b","repo":{"id":10,"full_name":"Example/Project"}}});
        let (object, _) = parser::normalize(&pr, 0, version(), &repo, 3, None, (1, 2))
            .map_err(|_| "github object")?;
        let profile = profile(IntegrationConfig::GitHub(vec![repo]), None);
        let target = GitHub.parse_target("https://github.com/Example/Project/pull/3")?;
        let coverage = GitHub.start(&profile, &target)?.coverage;
        Ok(Sample {
            profile,
            target,
            object,
            coverage,
            initial: vec![pr],
        })
    }
    fn jira() -> Result<Sample, Box<dyn std::error::Error>> {
        let issue = serde_json::json!({"id":"issue-id","key":"K-1","fields":{"summary":"S","description":null,"subtasks":[],"issuelinks":[],"parent":null}});
        let scope = JiraScope {
            profile: version(),
            cloud_id: "Site".into(),
        };
        let (_, _, objects, _) =
            crate::jira_parser::issue(&issue, &scope, (1, 2)).map_err(|_| "jira object")?;
        let object = objects.into_iter().next().ok_or("jira object")?;
        let profile = profile(IntegrationConfig::Jira(JiraConfig::default()), Some("Site"));
        let target = Jira.parse_target("K-1")?;
        let coverage = Jira.start(&profile, &target)?.coverage;
        Ok(Sample {
            profile,
            target,
            object,
            coverage,
            initial: vec![issue],
        })
    }
    fn todoist() -> Result<Sample, Box<dyn std::error::Error>> {
        let task = serde_json::json!({"id":"task-id","project_id":"project-id","content":"C","checked":false,"is_deleted":false});
        let content = crate::todoist_parser::task(&task).map_err(|_| "todoist task")?;
        let scope = TodoistScope {
            profile: version(),
            account: "account".into(),
        };
        let object =
            crate::todoist_parser::object(&scope, content, (1, 2)).map_err(|_| "todoist object")?;
        let profile = profile(
            IntegrationConfig::Todoist(TodoistConfig::default()),
            Some("account"),
        );
        let target = Todoist.parse_target("task-id")?;
        let coverage = Todoist.start(&profile, &target)?.coverage;
        Ok(Sample {
            profile,
            target,
            object,
            coverage,
            initial: vec![serde_json::json!({"id":"account"}), task],
        })
    }

    /// Every adapter method refuses the other adapter's variants with an error, never a panic.
    #[test]
    fn adapter_methods_reject_the_other_adapters_variants() -> Result<(), Box<dyn std::error::Error>>
    {
        let samples = [
            (&GitHub as &dyn Adapter, github()?),
            (&Jira as &dyn Adapter, jira()?),
            (&Todoist as &dyn Adapter, todoist()?),
        ];
        let pairs = samples.iter().flat_map(|own| {
            samples
                .iter()
                .filter(move |other| other.0.kind() != own.0.kind())
                .map(move |other| (own.0, &own.1, &other.1))
        });
        for (adapter, own, foreign) in pairs {
            let mixed = profile(own.profile.config.clone(), own.profile.scope.as_deref());
            assert!(adapter.validate_config(&foreign.profile.config).is_err());
            assert!(adapter.admit(&own.profile, &foreign.target).is_err());
            assert!(adapter.start(&own.profile, &foreign.target).is_err());
            assert!(
                adapter
                    .authorized(&mixed, &foreign.object.metadata)
                    .is_err()
            );
            assert!(!adapter.same_identity(&own.object.metadata, &foreign.object.metadata));
            assert!(!adapter.same_identity(&foreign.object.metadata, &own.object.metadata));
            assert!(adapter.validate_metadata(&foreign.object.metadata).is_err());
            assert!(adapter.validate_object(&foreign.object, false).is_err());
            assert!(adapter.validate_object(&foreign.object, true).is_err());
            // A reader past its identity read, given a window of the other adapter's coverage,
            // fails instead of panicking.
            let mut reader = adapter.start(&own.profile, &own.target)?.reader;
            let mut w = window(own.coverage.clone());
            for initial in &own.initial {
                reader.next(false, &mut w)?;
                let initial = IntegrationResponse {
                    status: 200,
                    body: serde_json::to_vec(initial)?,
                    headers: IntegrationHeaders::default(),
                };
                assert!(matches!(reader.accept(&initial, 2, &mut w), Step::Continue));
            }
            w.categories = foreign.coverage.clone();
            let unavailable = IntegrationResponse {
                status: 404,
                body: Vec::new(),
                headers: IntegrationHeaders::default(),
            };
            assert!(matches!(
                reader.accept(&unavailable, 2, &mut w),
                Step::Fail(422)
            ));
            assert!(reader.next(true, &mut w).is_err());
            // The own variants are accepted, so the refusals above are about the variant.
            assert!(adapter.validate_config(&own.profile.config).is_ok());
            assert!(adapter.same_identity(&own.object.metadata, &own.object.metadata));
        }
        Ok(())
    }

    /// Kind text is persisted in SQL, envelopes, 08B grounds and model context; serde and
    /// `as_str` must agree for every built-in adapter.
    #[test]
    fn kind_text_is_the_same_for_serde_sql_and_namespaces() -> Result<(), Box<dyn std::error::Error>>
    {
        // Exhaustive: a new kind fails to compile here until it is listed, and then fails this
        // test until its adapter joins the namespace table.
        let all = |k: IntegrationKind| match k {
            IntegrationKind::GitHub | IntegrationKind::Jira | IntegrationKind::Todoist => k,
        };
        for kind in [
            all(IntegrationKind::GitHub),
            all(IntegrationKind::Jira),
            all(IntegrationKind::Todoist),
        ] {
            assert!(
                crate::integration_types::ADAPTERS
                    .iter()
                    .any(|a| a.kind() == kind)
            );
        }
        for a in crate::integration_types::ADAPTERS {
            let kind = a.kind();
            assert_eq!(
                serde_json::to_string(&kind)?,
                format!("\"{}\"", kind.as_str())
            );
            assert_eq!(IntegrationKind::parse(kind.as_str()), Ok(kind));
            assert_eq!(IntegrationKind::from_namespace(a.namespace()), Some(kind));
        }
        Ok(())
    }
}
