use super::*;
use serde_json::json;

fn store() -> Result<(Store, std::path::PathBuf), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-loop-time-{}-{:?}.sqlite",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&path);
    let mut s = Store::open(&path)?;
    ready(&mut s)?;
    let files = [
        PluginFile {
            kind: PluginFileKind::Manifest,
            path: "plugin.json",
            bytes: br#"{"name":"Docs"}"#,
        },
        PluginFile {
            kind: PluginFileKind::Mcp,
            path: "mcp.json",
            bytes: br#"{"mcpServers":{"open":{"url":"https://open.example.com/mcp"}}}"#,
        },
    ];
    let id = s.install_plugin(&files, &crate::review_plugin(&files)?.hash, 1)?;
    let t = crate::canonical_tool(&json!({"name":"search","annotations":{"readOnlyHint":true}}))?;
    s.approve_plugin_tool(id, id, "open", &t.definition, &codex_hex(&t.hash))?;
    s.set_plugin_enabled(id, true)?;
    Ok((s, path))
}
/// A confirmed High check makes assistance available.
fn ready(s: &mut Store) -> Result<(), Box<dyn std::error::Error>> {
    s.set_model_session(9, true, true)?;
    let a = s.prepare_check(true, ModelProfile::High)?;
    let flag = a.cancellation();
    let d = s.dispatch_check(a, 1)?;
    s.finish_check(
        d.id(),
        ModelCheckResult {
            outcome: CheckOutcome::Confirmed,
            actual: ActualConfig {
                model: ActualModel::Luna,
                effort: ActualEffort::High,
                tier: ActualTier::Fast,
            },
            usage: None,
        },
    )?;
    s.release_check(&flag);
    Ok(())
}
fn tool_step() -> ModelResponse {
    ModelResponse {
        check: ModelCheckResult {
            outcome: CheckOutcome::Confirmed,
            actual: ActualConfig {
                model: ActualModel::Luna,
                effort: ActualEffort::High,
                tier: ActualTier::Fast,
            },
            usage: None,
        },
        answer: None,
        calls: vec![ToolCall {
            call_id: "c".into(),
            name: "search".into(),
            arguments: serde_json::Map::new(),
        }],
        forwarded: Vec::new(),
    }
}

#[test]
fn the_remaining_loop_time_decides_the_final_post_and_call_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut s, path) = store()?;
    let task = s.create_task("t")?;
    let p = s.prepare_assistance(AssistanceRequest {
        task: TaskVersion {
            task_id: task.id,
            revision: task.revision,
        },
        workspace_revision: s.assistant_workspace_revision(task.id)?,
        mode: AssistanceMode::NewContext,
        selected_evidence: vec![],
        reply: None,
    })?;
    let a = s.begin_assistance(p.request_id)?;
    let mut l = s.begin_model_loop(a, &[])?;
    // 100 s left: tools are still offered, but a call needs 90 s and the POST gets 60 s.
    l.started = Instant::now() - Duration::from_secs(80);
    let step = s.dispatch_model_step(&mut l, 1)?;
    assert_eq!(
        (step.mode, step.timeout),
        (StepMode::Tools, Duration::from_secs(60))
    );
    let id = step.id;
    drop(step);
    s.finish_model_step(&mut l, id, tool_step())?;
    l.started = Instant::now() - Duration::from_secs(95);
    assert!(
        s.permit_tool_call(&mut l, 2)?.is_none(),
        "no call with < 90 s left"
    );
    let step = s.dispatch_model_step(&mut l, 3)?;
    let HistoryItem::Output { output, .. } = &step.history[0] else {
        return Err("output".into());
    };
    assert!(output.contains("time_limit"));
    // 85 s left is not below 70 s: still a tools POST, limited to 60 s.
    assert_eq!(step.mode, StepMode::Tools);
    let id = step.id;
    drop(step);
    s.finish_model_step(&mut l, id, tool_step())?;
    l.pending.clear();
    // 50 s left: the final POST without tools gets the remaining time.
    l.started = Instant::now() - Duration::from_secs(130);
    let step = s.dispatch_model_step(&mut l, 4)?;
    assert_eq!(step.mode, StepMode::Final);
    assert!(step.timeout <= Duration::from_secs(50) && step.timeout > Duration::from_secs(45));
    drop(step);
    s.release_check(&l.cancellation());
    drop(s);
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn the_first_call_of_a_local_server_needs_its_start_time_too()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-loop-local-{}-{:?}.sqlite",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&path);
    let mut s = Store::open(&path)?;
    let files = [
        PluginFile {
            kind: PluginFileKind::Manifest,
            path: "plugin.json",
            bytes: br#"{"name":"Local"}"#,
        },
        PluginFile {
            kind: PluginFileKind::Mcp,
            path: "mcp.json",
            bytes: br#"{"mcpServers":{"local":{"command":"/usr/bin/python3"}}}"#,
        },
    ];
    let id = s.install_plugin(&files, &crate::review_plugin(&files)?.hash, 1)?;
    let t = crate::canonical_tool(&json!({"name":"search","annotations":{"readOnlyHint":true}}))?;
    s.approve_plugin_tool(id, id, "local", &t.definition, &codex_hex(&t.hash))?;
    s.set_plugin_enabled(id, true)?;
    let task = s.create_task("t")?;
    let request = AssistanceRequest {
        task: TaskVersion {
            task_id: task.id,
            revision: task.revision,
        },
        workspace_revision: s.assistant_workspace_revision(task.id)?,
        mode: AssistanceMode::NewContext,
        selected_evidence: vec![],
        reply: None,
    };
    for (elapsed, admitted) in [(70, false), (55, true)] {
        ready(&mut s)?;
        let p = s.prepare_assistance(request.clone())?;
        let a = s.begin_assistance(p.request_id)?;
        let mut l = s.begin_model_loop(a, &[])?;
        let step = s.dispatch_model_step(&mut l, 1)?;
        let id = step.id;
        drop(step);
        s.finish_model_step(&mut l, id, tool_step())?;
        // 110 s left is enough for a remote call (90 s) but not for starting a local server.
        l.started = Instant::now() - Duration::from_secs(elapsed);
        let permit = s.permit_tool_call(&mut l, 2)?;
        assert_eq!(permit.is_some(), admitted, "{elapsed}");
        if let Some(p) = permit {
            assert_eq!(p.timeout, CALL);
        }
        s.release_check(&l.cancellation());
    }
    drop(s);
    let _ = std::fs::remove_file(path);
    Ok(())
}

#[test]
fn the_first_call_of_the_managed_server_needs_its_start_time_too()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-loop-managed-{}-{:?}.sqlite",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&path);
    let mut s = Store::open(&path)?;
    let files = ManagedServerId::GoogleWorkspace.builtin_package();
    let id = s.install_plugin(files, &crate::review_plugin(files)?.hash, 1)?;
    let t =
        crate::canonical_tool(&json!({"name":"search_docs","annotations":{"readOnlyHint":true}}))?;
    s.approve_plugin_tool(id, id, "google", &t.definition, &codex_hex(&t.hash))?;
    s.set_plugin_enabled(id, true)?;
    let key = PluginKeyRef {
        plugin: id,
        server: "google".into(),
        generation: id,
    };
    let task = s.create_task("t")?;
    let request = AssistanceRequest {
        task: TaskVersion {
            task_id: task.id,
            revision: task.revision,
        },
        workspace_revision: s.assistant_workspace_revision(task.id)?,
        mode: AssistanceMode::NewContext,
        selected_evidence: vec![],
        reply: None,
    };
    for (elapsed, admitted) in [(70, false), (55, true)] {
        ready(&mut s)?;
        let p = s.prepare_assistance(request.clone())?;
        let a = s.begin_assistance(p.request_id)?;
        let mut l = s.begin_model_loop(a, std::slice::from_ref(&key))?;
        let step = s.dispatch_model_step(&mut l, 1)?;
        let step_id = step.id;
        drop(step);
        let mut response = tool_step();
        response.calls[0].name = "search_docs".into();
        s.finish_model_step(&mut l, step_id, response)?;
        // The managed server is a process started on its first call, like a local one.
        l.started = Instant::now() - Duration::from_secs(elapsed);
        let permit = s.permit_tool_call(&mut l, 2)?;
        assert_eq!(permit.is_some(), admitted, "{elapsed}");
        s.release_check(&l.cancellation());
    }
    drop(s);
    let _ = std::fs::remove_file(path);
    Ok(())
}
