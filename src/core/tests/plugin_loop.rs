#[path = "support/assistance.rs"]
pub mod assistance_support;
#[path = "support/plugins.rs"]
pub mod plugin_support;
use ambient_core::*;
use assistance_support::*;
use plugin_support::*;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "ambient-plugin-loop-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Run {
    task: TaskId,
    preview: AssistancePreview,
    l: ModelLoop,
}
fn start(s: &mut Store, keys: &[PluginKeyRef]) -> Result<Run> {
    let task = s.create_task("Find the launch plan")?;
    start_for(s, task.id, keys)
}
fn start_for(s: &mut Store, task: TaskId, keys: &[PluginKeyRef]) -> Result<Run> {
    // Any failed attempt withdraws High; a later request renews it like the app would.
    if !s.assistance_available()? {
        ready(s)?;
    }
    let revision = s.task(task)?.revision;
    let preview = prepare_new(s, task, revision, &[])?;
    let a = s.begin_assistance(preview.request_id)?;
    let l = s.begin_model_loop(a, keys)?;
    Ok(Run { task, preview, l })
}
/// One POST: returns its mode, offered function names and history length, then accounts it.
fn post(
    s: &mut Store,
    r: &mut Run,
    response: ModelResponse,
) -> Result<(StepMode, Vec<String>, usize, StepNext)> {
    let step = s.dispatch_model_step(&mut r.l, 10)?;
    let (id, mode) = (step.id, step.mode);
    let names = step.functions.iter().map(|f| f.name.clone()).collect();
    let history = step.history.len();
    drop(step);
    let next = s.finish_model_step(&mut r.l, id, response)?;
    Ok((mode, names, history, next))
}
fn post_answer(
    s: &mut Store,
    r: &mut Run,
    evidence: &[&str],
) -> Result<(StepMode, Vec<String>, usize, StepNext)> {
    let task = r.task;
    post(s, r, answered(task, evidence))
}
fn finish(s: &mut Store, r: &Run) {
    s.release_check(&r.l.cancellation());
}
fn source_of(s: &Store, task: TaskId) -> Result<Evidence> {
    s.evidence(task, 0, 50)?
        .items
        .into_iter()
        .find(|e| matches!(&e.origin, Origin::External { key, .. } if key.namespace == PLUGIN_NAMESPACE))
        .ok_or_else(|| "saved result".into())
}

#[test]
fn without_enabled_plugins_the_request_is_one_plain_post_with_the_old_context() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    // Installed but disabled plugins change nothing either.
    let f = files();
    s.install_plugin(&f, &review_plugin(&f)?.hash, 1)?;
    let mut r = start(&mut s, &[])?;
    let context: Value = serde_json::from_str(&r.preview.context)?;
    assert!(context.get("plugins").is_none());
    let (mode, names, history, next) = post_answer(&mut s, &mut r, &[])?;
    assert_eq!((mode, names.len(), history), (StepMode::Plain, 0, 0));
    assert!(matches!(next, StepNext::Done(Some(_))));
    assert_eq!(
        s.dispatch_model_step(&mut r.l, 11).err(),
        Some(StoreError::InvalidInput)
    );
    finish(&mut s, &r);
    assert_eq!(s.check_summary()?.attempts, 2);
    Ok(())
}

#[test]
fn a_cited_tool_result_is_saved_with_provenance_and_survives_continue_and_restart() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let search = raw_tool("search", "Search documents");
    let id = installed(&mut s, "docs", std::slice::from_ref(&search))?;
    let mut r = {
        let k = key(&s, id, "docs")?;
        start(&mut s, &[k])
    }?;
    let context: Value = serde_json::from_str(&r.preview.context)?;
    assert_eq!(context["plugins"]["offered"][0]["name"], "search");
    assert_eq!(context["plugins"]["skills"][0]["skill"], "search-docs");
    let before = s.task(r.task)?.revision;

    let (mode, names, _, next) = post(
        &mut s,
        &mut r,
        calls(&[("search", json!({"z":1,"q":"launch plan"}))]),
    )?;
    assert_eq!((mode, names), (StepMode::Tools, vec!["search".to_owned()]));
    assert!(matches!(next, StepNext::Tools));
    let permit = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
    assert_eq!(
        permit.target,
        Target::Remote {
            url: "https://mcp.example.com/mcp".into()
        }
    );
    assert_eq!((permit.plugin, permit.server.as_str()), (id, "docs"));
    assert_eq!(permit.tool, "search");
    assert_eq!(permit.arguments, r#"{"q":"launch plan","z":1}"#);
    assert!(permit.list_first);
    assert_eq!(permit.key.as_ref().map(|k| k.plugin), Some(id));
    let calls_log: Value = serde_json::from_str(&s.plugin_calls_json()?)?;
    assert_eq!(calls_log[0]["stage"], 1);
    s.accept_tool_result(
        &mut r.l,
        permit,
        completed(
            "Launch plan v3: ship Friday",
            Some(std::slice::from_ref(&search)),
        )?,
    )?;
    assert!(s.permit_tool_call(&mut r.l, 21)?.is_none());
    let calls_log: Value = serde_json::from_str(&s.plugin_calls_json()?)?;
    assert_eq!(calls_log[0]["stage"], 2);
    assert_eq!(calls_log[0]["outcome"], PluginCallOutcome::Completed as u32);
    assert_eq!(calls_log[0]["arguments"], r#"{"q":"launch plan","z":1}"#);

    let step = s.dispatch_model_step(&mut r.l, 30)?;
    assert_eq!(step.mode, StepMode::Tools);
    // Reasoning is forwarded unchanged, then the Core-built envelope.
    assert!(matches!(&step.history[0], HistoryItem::Forwarded(v) if v["id"] == "rs_1"));
    let HistoryItem::Output { call_id, output } = &step.history[1] else {
        return Err("output".into());
    };
    assert_eq!(call_id, "call_0");
    let envelope: Value = serde_json::from_str(output)?;
    assert_eq!(
        envelope,
        json!({"tool_result_id":"tool:1","tool":"search","untrusted":true,"complete":true,"citable":true,"text":"Launch plan v3: ship Friday"})
    );
    let id2 = step.id;
    drop(step);
    let StepNext::Done(Some(v)) =
        s.finish_model_step(&mut r.l, id2, answered(r.task, &["tool:1"]))?
    else {
        return Err("published".into());
    };
    finish(&mut s, &r);
    // Saving grew the task revision; publication returns and stores the final one.
    assert!(v.task.revision > before);
    assert_eq!(s.task(r.task)?.revision, v.task.revision);
    let saved = source_of(&s, r.task)?;
    assert_eq!(saved.text, "Launch plan v3: ship Friday");
    assert_eq!(saved.kind, AssertionKind::SourceFact);
    assert_eq!(saved.completeness, Completeness::Complete);
    assert_eq!(saved.observed_at_utc, Some(20));
    let Origin::External { key: k, .. } = &saved.origin else {
        return Err("origin".into());
    };
    assert_eq!(k.scope, id.to_string());
    assert!(k.object_id.starts_with("docs/search:"));
    let p = s
        .proposal_feed()?
        .proposals
        .into_iter()
        .next()
        .ok_or("proposal")?;
    assert!(p.citations.contains(&ProposalCitationRef::Source(saved.id)));
    let d = s.proposal_details(p.target())?;
    let c = d
        .citations
        .iter()
        .find(|c| c.reference == ProposalCitationRef::Source(saved.id))
        .ok_or("citation")?;
    let cj = serde_json::to_value(c)?;
    assert_eq!(
        cj["plugin"],
        json!({"plugin":"Docs","server":"docs","tool":"search","called_at_utc":20,"complete":true})
    );
    // Continue sees the saved result as a source; the restart keeps the discussion.
    let w = s.assistant_workspace_revision(r.task)?;
    let cont = s.prepare_assistance(AssistanceRequest {
        task: v.task,
        workspace_revision: w,
        mode: AssistanceMode::Continue,
        selected_evidence: vec![],
        reply: None,
    })?;
    let ctx: Value = serde_json::from_str(&cont.context)?;
    assert_eq!(ctx["sources"][0]["plugin"]["tool"], "search");
    s.clear_assistance();
    drop(s);
    let mut s = Store::open(&db.0)?;
    assert_eq!(s.proposal_feed()?.proposals.len(), 1);
    // An ordinary edit changes the task revision; open-time `same_ground` keeps the discussion.
    let t = s.task(r.task)?;
    s.add_note(t.id, t.revision, "a later note", 40)?;
    drop(s);
    let s = Store::open(&db.0)?;
    assert_eq!(s.proposal_feed()?.proposals.len(), 1);
    Ok(())
}

#[test]
fn local_errors_need_no_network_and_a_changed_or_unlisted_tool_is_never_sent() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let search = raw_tool("search", "Search");
    let read = raw_tool("read", "Read");
    let id = installed(&mut s, "docs", &[search.clone(), read.clone()])?;
    // Without a key for an auth server: "login required" without a call-log row.
    let mut r = start(&mut s, &[])?;
    post(
        &mut s,
        &mut r,
        calls(&[
            ("search", json!({"q":"x"})),
            ("nope", json!({})),
            ("read", json!({"q":"y".repeat(2100)})),
        ]),
    )?;
    assert!(s.permit_tool_call(&mut r.l, 20)?.is_none());
    let step = s.dispatch_model_step(&mut r.l, 30)?;
    let errors = step
        .history
        .iter()
        .filter_map(|h| match h {
            HistoryItem::Output { output, .. } => serde_json::from_str::<Value>(output).ok(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        errors[0],
        json!({"tool_result_id":"tool:1","tool":"search","error":"login_required"})
    );
    assert_eq!(errors[1], json!({"tool":"nope","error":"unknown_tool"}));
    assert_eq!(errors[2]["error"], "arguments_too_long");
    let id2 = step.id;
    drop(step);
    s.finish_model_step(&mut r.l, id2, answered(r.task, &[]))?;
    finish(&mut s, &r);
    assert_eq!(
        serde_json::from_str::<Value>(&s.plugin_calls_json()?)?,
        json!([])
    );

    // A changed live definition is not called; later calls to that server stay local.
    let mut r = {
        let k = key(&s, id, "docs")?;
        start(&mut s, &[k])
    }?;
    post(
        &mut s,
        &mut r,
        calls(&[("search", json!({"q":"x"})), ("read", json!({"q":"y"}))]),
    )?;
    let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
    let mut live = search.clone();
    live["annotations"]["readOnlyHint"] = json!(false);
    s.accept_tool_result(
        &mut r.l,
        p,
        ToolCallResult {
            outcome: PluginCallOutcome::Changed,
            text: String::new(),
            listed: Some(listed(&[live, raw_tool("read", "Read v2")])?),
        },
    )?;
    assert!(
        s.permit_tool_call(&mut r.l, 21)?.is_none(),
        "read changed too: local error"
    );
    let step = s.dispatch_model_step(&mut r.l, 30)?;
    let outs = step
        .history
        .iter()
        .filter_map(|h| match h {
            HistoryItem::Output { output, .. } => serde_json::from_str::<Value>(output).ok(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(outs[0]["error"], "tool_changed");
    assert_eq!(outs[1]["error"], "tool_changed");
    let id2 = step.id;
    drop(step);
    // The model cannot cite an error result.
    assert_eq!(
        s.finish_model_step(&mut r.l, id2, answered(r.task, &["tool:1"]))
            .err(),
        Some(StoreError::InvalidInput)
    );
    finish(&mut s, &r);
    let log: Value = serde_json::from_str(&s.plugin_calls_json()?)?;
    assert_eq!(log.as_array().map(Vec::len), Some(1));
    assert_eq!(log[0]["outcome"], PluginCallOutcome::Changed as u32);
    Ok(())
}

#[test]
fn every_post_is_admitted_and_the_fifth_or_after_eight_calls_has_no_tools() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let search = raw_tool("search", "Search");
    installed(&mut s, "open", std::slice::from_ref(&search))?;
    let mut r = start(&mut s, &[])?;
    let attempts = s.check_summary()?.attempts;
    for i in 0..4 {
        let (mode, names, _, next) = post(&mut s, &mut r, calls(&[("search", json!({"q": i}))]))?;
        assert_eq!(mode, StepMode::Tools, "post {i}");
        assert_eq!(names, vec!["search".to_owned()]);
        assert!(matches!(next, StepNext::Tools));
        let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
        assert!(p.key.is_none(), "a server without sign-in needs no key");
        s.accept_tool_result(
            &mut r.l,
            p,
            completed("r", (i == 0).then_some(std::slice::from_ref(&search)))?,
        )?;
    }
    // Calls in the final POST, which offered no tools, are an invalid answer.
    let (mode, names, history, next) = post(&mut s, &mut r, calls(&[("search", json!({}))]))?;
    assert_eq!((mode, names.len(), history), (StepMode::Final, 0, 8));
    assert!(matches!(next, StepNext::Done(None)));
    finish(&mut s, &r);
    let summary = s.check_summary()?;
    assert_eq!(summary.attempts, attempts + 5);
    assert_eq!(
        summary.latest.map(|l| l.result.outcome),
        Some(CheckOutcome::Invalid)
    );

    // Eight calls exhaust the call budget: the next POST is final.
    let mut r = start(&mut s, &[])?;
    let eight = (0..8)
        .map(|i| ("search", json!({"q": i})))
        .collect::<Vec<_>>();
    post(&mut s, &mut r, calls(&eight))?;
    while let Some(p) = s.permit_tool_call(&mut r.l, 20)? {
        s.accept_tool_result(&mut r.l, p, completed("r", None)?)?;
    }
    let (mode, ..) = post_answer(&mut s, &mut r, &["tool:8"])?;
    assert_eq!(mode, StepMode::Final);
    finish(&mut s, &r);
    Ok(())
}

#[test]
fn disable_revoke_unapprove_and_401_cancel_the_running_loop_but_saved_results_stay_usable() -> Result
{
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let search = raw_tool("search", "Search");
    let id = installed(&mut s, "docs", std::slice::from_ref(&search))?;
    // Save one result first.
    let mut r = {
        let k = key(&s, id, "docs")?;
        start(&mut s, &[k])
    }?;
    post(&mut s, &mut r, calls(&[("search", json!({"q":"a"}))]))?;
    let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
    s.accept_tool_result(
        &mut r.l,
        p,
        completed("saved text", Some(std::slice::from_ref(&search)))?,
    )?;
    post_answer(&mut s, &mut r, &["tool:1"])?;
    finish(&mut s, &r);
    let task = r.task;
    let saved = source_of(&s, task)?;

    // 401 revokes exactly the sent generation and cancels the request.
    let mut r = {
        let k = key(&s, id, "docs")?;
        start_for(&mut s, task, &[k])
    }?;
    post(&mut s, &mut r, calls(&[("search", json!({"q":"b"}))]))?;
    let g = generation(&s, id, "docs")?;
    let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
    assert_eq!(
        s.accept_tool_result(
            &mut r.l,
            p,
            ToolCallResult {
                outcome: PluginCallOutcome::LoginRequired,
                text: String::new(),
                listed: None
            }
        )
        .err(),
        Some(StoreError::Cancelled)
    );
    assert!(generation(&s, id, "docs")? > g);
    assert!(s.dispatch_model_step(&mut r.l, 30).is_err());
    finish(&mut s, &r);
    // A new key and the saved result is still a valid ground: a new preview selects it.
    let t = s.task(task)?;
    let preview = prepare_new(&mut s, task, t.revision, &[saved.id])?;
    s.discard_assistance_preview(preview.request_id)?;

    // Disable cancels a running loop; unapprove and re-pin do too.
    for change in 0..3 {
        let mut r = {
            let k = key(&s, id, "docs")?;
            start_for(&mut s, task, &[k])
        }?;
        post(&mut s, &mut r, calls(&[("search", json!({"q": change}))]))?;
        let rev = revision(&s, id)?;
        match change {
            0 => s.set_plugin_enabled(id, false)?,
            1 => {
                s.unapprove_plugin_tool(id, rev, "docs", "search")?;
            }
            _ => {
                let v2 = canonical_tool(&raw_tool("search", "Search v2"))?;
                let hex = v2
                    .hash
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                s.approve_plugin_tool(id, rev, "docs", &v2.definition, &hex)?;
            }
        }
        assert_eq!(
            s.permit_tool_call(&mut r.l, 20).err(),
            Some(StoreError::Cancelled),
            "{change}"
        );
        finish(&mut s, &r);
        if change == 0 {
            s.set_plugin_enabled(id, true)?;
        }
        if change == 1 {
            let t = canonical_tool(&search)?;
            let hex = t
                .hash
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            s.approve_plugin_tool(id, revision(&s, id)?, "docs", &t.definition, &hex)?;
        }
    }
    Ok(())
}

#[test]
fn disable_and_removal_clear_dependent_assistance_and_enable_restores_saved_results() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let search = raw_tool("search", "Search");
    let id = installed(&mut s, "docs", std::slice::from_ref(&search))?;
    let mut r = {
        let k = key(&s, id, "docs")?;
        start(&mut s, &[k])
    }?;
    post(&mut s, &mut r, calls(&[("search", json!({"q":"a"}))]))?;
    let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
    s.accept_tool_result(
        &mut r.l,
        p,
        completed("saved text", Some(std::slice::from_ref(&search)))?,
    )?;
    post_answer(&mut s, &mut r, &["tool:1"])?;
    finish(&mut s, &r);
    assert_eq!(s.proposal_feed()?.proposals.len(), 1);
    s.set_plugin_enabled(id, false)?;
    assert!(
        s.proposal_feed()?.proposals.is_empty(),
        "disable clears 08B in the same change"
    );
    let saved = source_of(&s, r.task)?;
    let t = s.task(r.task)?;
    assert_eq!(
        prepare_new(&mut s, r.task, t.revision, &[saved.id]).err(),
        Some(StoreError::StaleRevision)
    );
    s.set_plugin_enabled(id, true)?;
    let p = prepare_new(&mut s, r.task, t.revision, &[saved.id])?;
    s.discard_assistance_preview(p.request_id)?;
    // Removal keeps the saved fact as history, but it can no longer be selected.
    s.set_plugin_enabled(id, false)?;
    let rev = s.begin_plugin_removal(id, revision(&s, id)?)?;
    s.finish_plugin_removal(id, rev)?;
    assert_eq!(source_of(&s, r.task)?.text, "saved text");
    assert_eq!(
        prepare_new(&mut s, r.task, t.revision, &[saved.id]).err(),
        Some(StoreError::StaleRevision)
    );
    drop(s);
    Store::open(&db.0)?;
    Ok(())
}

#[test]
fn citability_is_decided_at_receipt_and_publication_rejects_late_forgetting_or_edits() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let search = raw_tool("search", "Search");
    installed(&mut s, "open", std::slice::from_ref(&search))?;
    let task = s.create_task("Find it")?.id;
    let save = |s: &mut Store, q: &str| -> Result<Evidence> {
        let mut r = start_for(s, task, &[])?;
        post(s, &mut r, calls(&[("search", json!({"q": q}))]))?;
        let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
        s.accept_tool_result(
            &mut r.l,
            p,
            completed(&format!("result {q}"), Some(std::slice::from_ref(&search)))?,
        )?;
        post(s, &mut r, answered(task, &["tool:1"]))?;
        finish(s, &r);
        Ok(s.evidence(task, 0, 50)?
            .items
            .into_iter()
            .rev()
            .find(|e| e.text == format!("result {q}"))
            .ok_or("saved")?)
    };
    let edited = save(&mut s, "edited")?;
    let t = s.task(task)?;
    s.edit_evidence(edited.id, task, t.revision, "my correction", 50)?;
    let forgotten = save(&mut s, "forgotten")?;
    let t = s.task(task)?;
    s.delete_evidence(forgotten.id, task, t.revision, &[])?;
    // The same call again: user-edited and forgotten keys are not citable at receipt.
    for q in ["edited", "forgotten"] {
        let mut r = start_for(&mut s, task, &[])?;
        post(&mut s, &mut r, calls(&[("search", json!({"q": q}))]))?;
        let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
        s.accept_tool_result(
            &mut r.l,
            p,
            completed(&format!("result {q}"), Some(std::slice::from_ref(&search)))?,
        )?;
        let step = s.dispatch_model_step(&mut r.l, 30)?;
        let HistoryItem::Output { output, .. } = &step.history[1] else {
            return Err("output".into());
        };
        assert_eq!(
            serde_json::from_str::<Value>(output)?["citable"],
            false,
            "{q}"
        );
        let id2 = step.id;
        drop(step);
        assert_eq!(
            s.finish_model_step(&mut r.l, id2, answered(task, &["tool:1"]))
                .err(),
            Some(StoreError::InvalidInput)
        );
        finish(&mut s, &r);
    }
    // Between receipt and publication another task forgets the same key: the whole publication
    // fails with StaleRevision and the previous set stays.
    let other = s.create_task("Other task")?.id;
    let shared = {
        let mut r = start_for(&mut s, other, &[])?;
        post(&mut s, &mut r, calls(&[("search", json!({"q":"shared"}))]))?;
        let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
        s.accept_tool_result(
            &mut r.l,
            p,
            completed("result shared", Some(std::slice::from_ref(&search)))?,
        )?;
        post(&mut s, &mut r, answered(other, &["tool:1"]))?;
        finish(&mut s, &r);
        source_of(&s, other)?
    };
    let late = |s: &mut Store, q: &str| -> Result<(Run, AttemptId)> {
        let mut r = start_for(s, task, &[])?;
        post(s, &mut r, calls(&[("search", json!({"q": q}))]))?;
        let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
        s.accept_tool_result(
            &mut r.l,
            p,
            completed(&format!("result {q}"), Some(std::slice::from_ref(&search)))?,
        )?;
        let step = s.dispatch_model_step(&mut r.l, 30)?;
        let id2 = step.id;
        drop(step);
        Ok((r, id2))
    };
    let of_task = |s: &Store| -> Result<Vec<Proposal>> {
        Ok(s.proposal_feed()?
            .proposals
            .into_iter()
            .filter(|p| p.task.task_id == task)
            .collect())
    };
    let feed = of_task(&s)?;
    let (mut r, id2) = late(&mut s, "shared")?;
    let o = s.task(other)?;
    s.delete_evidence(shared.id, other, o.revision, &[])?;
    assert_eq!(
        s.finish_model_step(&mut r.l, id2, answered(task, &["tool:1"]))
            .err(),
        Some(StoreError::StaleRevision)
    );
    finish(&mut s, &r);
    assert_eq!(of_task(&s)?, feed);
    let attempts = s.check_summary()?;
    assert_eq!(
        attempts.latest.map(|a| a.result.outcome),
        Some(CheckOutcome::Confirmed)
    );

    // Memory filled by another task meanwhile: ResourceLimit, nothing published.
    let (mut r, id2) = late(&mut s, "space")?;
    let c = Connection::open(&db.0)?;
    let filler = "f".repeat(16 * 1024);
    let tx = c.unchecked_transaction()?;
    for _ in 0..(64 * 1024 / 16) {
        tx.execute(
            "INSERT INTO evidence(task_id,text,origin,observed_at_utc,completeness,kind) VALUES(?1,?2,0,1,2,2)",
            rusqlite::params![other.0, filler],
        )?;
    }
    tx.commit()?;
    drop(c);
    assert_eq!(
        s.finish_model_step(&mut r.l, id2, answered(task, &["tool:1"]))
            .err(),
        Some(StoreError::ResourceLimit)
    );
    finish(&mut s, &r);
    assert_eq!(of_task(&s)?, feed);
    Ok(())
}

#[test]
fn results_near_the_context_budget_make_the_next_post_final() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let search = raw_tool("search", "Search");
    installed(&mut s, "open", std::slice::from_ref(&search))?;
    let mut r = start(&mut s, &[])?;
    let seven = (0..7)
        .map(|i| ("search", json!({"q": i})))
        .collect::<Vec<_>>();
    post(&mut s, &mut r, calls(&seven))?;
    let long = "é".repeat(7 * 1024);
    while let Some(p) = s.permit_tool_call(&mut r.l, 20)? {
        s.accept_tool_result(
            &mut r.l,
            p,
            completed(&long, Some(std::slice::from_ref(&search)))?,
        )?;
    }
    let step = s.dispatch_model_step(&mut r.l, 30)?;
    assert_eq!(step.mode, StepMode::Final);
    let HistoryItem::Output { output, .. } = &step.history[1] else {
        return Err("output".into());
    };
    let e: Value = serde_json::from_str(output)?;
    // 12 KiB UTF-8 prefix, marked partial.
    assert_eq!(e["complete"], false);
    assert_eq!(e["text"].as_str().map(str::len), Some(12 * 1024));
    let id = step.id;
    drop(step);
    let task = r.task;
    s.finish_model_step(&mut r.l, id, answered(task, &["tool:2"]))?;
    finish(&mut s, &r);
    let saved = source_of(&s, r.task)?;
    assert_eq!(saved.completeness, Completeness::Partial);
    Ok(())
}

#[test]
fn a_new_key_keeps_saved_results_and_the_same_call_updates_them_in_place() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let search = raw_tool("search", "Search");
    let id = installed(&mut s, "docs", std::slice::from_ref(&search))?;
    let task = s.create_task("Plan")?.id;
    let mut ids = Vec::new();
    for text in ["first account", "second account"] {
        let k = key(&s, id, "docs")?;
        let mut r = start_for(&mut s, task, &[k])?;
        post(&mut s, &mut r, calls(&[("search", json!({"q":"plan"}))]))?;
        let p = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
        s.accept_tool_result(
            &mut r.l,
            p,
            completed(text, Some(std::slice::from_ref(&search)))?,
        )?;
        post_answer(&mut s, &mut r, &["tool:1"])?;
        finish(&mut s, &r);
        ids.push(source_of(&s, task)?);
        // Sign out and in with another key: the generation moves, saved results stay.
        let rev = revision(&s, id)?;
        s.revoke_plugin_credential(id, rev, "docs")?;
    }
    assert_eq!(ids[0].id, ids[1].id, "the same call updates the same fact");
    assert_eq!(ids[1].text, "second account");
    let all = s.evidence(task, 0, 50)?.items;
    assert_eq!(all.len(), 1);
    Ok(())
}

#[test]
fn pause_cancel_and_session_end_stop_the_loop_at_every_phase() -> Result {
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let search = raw_tool("search", "Search");
    installed(&mut s, "open", std::slice::from_ref(&search))?;
    for phase in 0..6 {
        let mut r = start(&mut s, &[])?;
        let stop = |s: &mut Store, r: &Run, which: usize| -> Result {
            match which % 3 {
                0 => s.set_manual_pause(true)?,
                1 => {
                    r.l.cancellation().store(true, Ordering::Release);
                    s.clear_assistance();
                }
                _ => s.set_model_session(77, true, true)?,
            }
            Ok(())
        };
        if phase < 3 {
            // Before a POST.
            stop(&mut s, &r, phase)?;
            assert!(s.dispatch_model_step(&mut r.l, 10).is_err(), "{phase}");
        } else {
            // Between the model step and the tool call.
            post(&mut s, &mut r, calls(&[("search", json!({"q":"x"}))]))?;
            stop(&mut s, &r, phase)?;
            assert!(s.permit_tool_call(&mut r.l, 20).is_err(), "{phase}");
        }
        finish(&mut s, &r);
        s.set_manual_pause(false)?;
        s.set_model_session(9, true, true)?;
    }
    Ok(())
}

#[test]
fn a_local_server_is_permitted_by_target_needs_its_values_and_marks_an_unconfirmed_stop() -> Result
{
    let db = Database::new();
    let mut s = Store::open(&db.0)?;
    ready(&mut s)?;
    let echo = raw_tool("echo", "Echo");
    let id = installed_local(&mut s, &["API_TOKEN"], std::slice::from_ref(&echo))?;
    let server = &plugin(&s, id)?["servers"][0];
    assert_eq!(server["kind"]["kind"], "local");
    assert_eq!(server["kind"]["env"], json!(["API_TOKEN"]));
    assert_eq!(server["needs_key"], true);
    assert_eq!(server["downloads"], false);
    // Without the variable values: "not configured", no network and no call-log row.
    let mut r = start(&mut s, &[])?;
    post(&mut s, &mut r, calls(&[("echo", json!({"q":"x"}))]))?;
    assert!(s.permit_tool_call(&mut r.l, 20)?.is_none());
    let step = s.dispatch_model_step(&mut r.l, 30)?;
    let HistoryItem::Output { output, .. } = &step.history[1] else {
        return Err("output".into());
    };
    assert_eq!(
        serde_json::from_str::<Value>(output)?,
        json!({"tool_result_id":"tool:1","tool":"echo","error":"not_configured"})
    );
    let id2 = step.id;
    drop(step);
    s.finish_model_step(&mut r.l, id2, answered(r.task, &[]))?;
    finish(&mut s, &r);
    assert_eq!(
        serde_json::from_str::<Value>(&s.plugin_calls_json()?)?,
        json!([])
    );

    let mut r = {
        let k = key(&s, id, "local")?;
        start(&mut s, &[k])
    }?;
    post(&mut s, &mut r, calls(&[("echo", json!({"q":"x"}))]))?;
    let permit = s.permit_tool_call(&mut r.l, 20)?.ok_or("permit")?;
    assert_eq!(
        permit.target,
        Target::Local {
            command: "/usr/bin/python3".into(),
            args: vec!["/opt/server.py".into()],
            env_names: vec!["API_TOKEN".into()],
        }
    );
    assert_eq!((permit.plugin, permit.server.as_str()), (id, "local"));
    assert!(permit.list_first);
    s.accept_tool_result(
        &mut r.l,
        permit,
        ToolCallResult {
            outcome: PluginCallOutcome::Completed,
            text: "echo".into(),
            listed: Some(listed(std::slice::from_ref(&echo))?),
        },
    )?;
    finish(&mut s, &r);
    // After the request the host could not confirm the process group gone.
    s.record_unconfirmed_stops(&[(id, "local".into())])?;
    let log: Value = serde_json::from_str(&s.plugin_calls_json()?)?;
    assert_eq!(log.as_array().map(Vec::len), Some(1));
    assert_eq!(log[0]["outcome"], PluginCallOutcome::StopUnconfirmed as u32);
    drop(s);
    let mut s = Store::open(&db.0)?;
    let log: Value = serde_json::from_str(&s.plugin_calls_json()?)?;
    assert_eq!(log[0]["outcome"], 11);
    // Cancelled during the call: the result is never accepted, the row is still stage 1, and the
    // unconfirmed stop must still reach the log instead of «interrupted» at the next open.
    let mut r = {
        let k = key(&s, id, "local")?;
        start(&mut s, &[k])
    }?;
    post(&mut s, &mut r, calls(&[("echo", json!({"q":"y"}))]))?;
    let _permit = s.permit_tool_call(&mut r.l, 40)?.ok_or("permit")?;
    s.clear_assistance();
    finish(&mut s, &r);
    s.record_unconfirmed_stops(&[(id, "local".into())])?;
    let log: Value = serde_json::from_str(&s.plugin_calls_json()?)?;
    assert_eq!(
        (log[0]["stage"].clone(), log[0]["outcome"].clone()),
        (json!(2), json!(11))
    );
    assert_eq!(log[0]["result_bytes"], 0);
    Ok(())
}
