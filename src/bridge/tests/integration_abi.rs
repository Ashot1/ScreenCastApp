use ambient_bridge::*;
fn text(s: &str) -> AmbientText {
    AmbientText {
        bytes: s.as_ptr(),
        length: s.len() as u32,
    }
}
#[test]
fn integration_abi_profiles_use_same_handle_and_reject_wrong_kind_or_scope()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(std::mem::size_of::<AmbientIntegrationProfileRef>(), 24);
    assert_eq!(std::mem::size_of::<AmbientIntegrationRead>(), 64);
    assert_eq!(std::mem::size_of::<AmbientIntegrationBatch>(), 280);
    let path = std::env::temp_dir().join(format!(
        "ambient-integration-abi-{}.sqlite3",
        std::process::id()
    ));
    let bytes = path.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    unsafe {
        assert_eq!(
            ambient_v3_open(bytes.as_ptr(), bytes.len() as u32, &mut h),
            0
        );
    }
    let mut p = AmbientIntegrationProfileRef::default();
    unsafe {
        assert_eq!(
            ambient_integration_v1_jira_profile_save(
                h,
                0,
                0,
                text("Work"),
                text("Site-ID"),
                &mut p
            ),
            0
        );
    }
    assert!(p.id > 0);
    let id = p.id;
    let revision = p.settings_revision;
    unsafe {
        assert_ne!(
            ambient_integration_v1_jira_profile_save(
                h,
                id,
                revision,
                text("Move"),
                text("Other-ID"),
                &mut p
            ),
            0
        );
    }
    assert_eq!(p.id, id);
    assert_eq!(p.settings_revision, revision);
    let mut bytes = vec![0x5a; 1024 * 1024];
    let mut used = 0xdeadbeef;
    unsafe {
        assert_eq!(
            ambient_integration_v1_profiles(
                h,
                AMBIENT_INTEGRATION_JIRA,
                bytes.as_mut_ptr(),
                1024 * 1024 - 1,
                &mut used
            ),
            6
        );
        assert_eq!(used, 0xdeadbeef);
        assert!(bytes.iter().all(|b| *b == 0x5a));
        assert_eq!(
            ambient_integration_v1_profiles(
                h,
                AMBIENT_INTEGRATION_JIRA,
                std::ptr::null_mut(),
                1024 * 1024,
                &mut used
            ),
            1
        );
        // Local projects and Codex keep their own ABIs; they are not engine kinds.
        for kind in [0, 1, 2, 6] {
            assert_eq!(
                ambient_integration_v1_profiles(
                    h,
                    kind,
                    bytes.as_mut_ptr(),
                    1024 * 1024,
                    &mut used
                ),
                1
            );
            assert_eq!(
                ambient_integration_v1_scope_excluded(h, kind, text("Site-ID"), 1),
                1
            );
        }
        assert_eq!(used, 0xdeadbeef);
        let invalid = [0xff];
        assert_eq!(
            ambient_integration_v1_jira_profile_save(
                h,
                0,
                0,
                AmbientText {
                    bytes: invalid.as_ptr(),
                    length: 1
                },
                text("Other"),
                &mut p
            ),
            2
        );
        assert_eq!((p.id, p.settings_revision), (id, revision));
        assert_eq!(
            ambient_integration_v1_scope_excluded(h, AMBIENT_INTEGRATION_JIRA, text("Site-ID"), 2),
            1
        );
        assert_eq!(
            ambient_integration_v1_scope_excluded(h, AMBIENT_INTEGRATION_JIRA, text("bad site"), 1),
            1
        );
        assert_eq!(
            ambient_integration_v1_profiles(
                h,
                AMBIENT_INTEGRATION_JIRA,
                bytes.as_mut_ptr(),
                1024 * 1024,
                &mut used
            ),
            0
        );
    }
    let settings = std::str::from_utf8(&bytes[..used as usize])?;
    assert!(settings.starts_with("{\"exclusions\":[],\"instance\":\""));
    let instance = settings
        .split("\"instance\":\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .ok_or("instance")?;
    assert_eq!(instance.len(), 32);
    assert!(
        instance
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert!(settings.contains("\"kind\":\"jira\""));
    assert_eq!(settings.matches("\"scope\":\"Site-ID\"").count(), 1);
    assert_eq!(ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
#[test]
fn github_profile_save_is_typed_and_github_has_no_scope_exclusion()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(std::mem::size_of::<AmbientGitHubRepository>(), 32);
    assert_eq!(std::mem::align_of::<AmbientGitHubRepository>(), 8);
    let path = std::env::temp_dir().join(format!(
        "ambient-integration-github-abi-{}.sqlite3",
        std::process::id()
    ));
    let bytes = path.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    unsafe {
        assert_eq!(
            ambient_v3_open(bytes.as_ptr(), bytes.len() as u32, &mut h),
            0
        );
    }
    let repository = |owner, repository| AmbientGitHubRepository {
        owner: text(owner),
        repository: text(repository),
    };
    let repositories = [repository("Example", "Project")];
    let mut p = AmbientIntegrationProfileRef::default();
    unsafe {
        assert_eq!(
            ambient_integration_v1_github_profile_save(
                h,
                0,
                0,
                text("Work"),
                repositories.as_ptr(),
                1,
                &mut p
            ),
            0
        );
    }
    assert!(p.id > 0);
    let saved = (p.id, p.settings_revision);
    let many = vec![repository("Example", "Project"); 33];
    let invalid = [0xff];
    let bad_utf8 = [AmbientGitHubRepository {
        owner: AmbientText {
            bytes: invalid.as_ptr(),
            length: 1,
        },
        repository: text("Project"),
    }];
    let dot = [repository("..", "Project")];
    let duplicate = [
        repository("Example", "Project"),
        repository("example", "PROJECT"),
    ];
    unsafe {
        for (list, count, code) in [
            (many.as_ptr(), 33, 1),
            (std::ptr::null(), 1, 1),
            (bad_utf8.as_ptr(), 1, 2),
            (dot.as_ptr(), 1, 1),
            (duplicate.as_ptr(), 2, 1),
        ] {
            assert_eq!(
                ambient_integration_v1_github_profile_save(
                    h,
                    0,
                    0,
                    text("Bad"),
                    list,
                    count,
                    &mut p
                ),
                code
            );
            assert_eq!((p.id, p.settings_revision), saved);
        }
        // An explicit empty allowlist is a valid stored setting.
        assert_eq!(
            ambient_integration_v1_github_profile_save(
                h,
                0,
                0,
                text("Empty"),
                std::ptr::null(),
                0,
                &mut p
            ),
            0
        );
        assert_eq!(
            ambient_integration_v1_scope_excluded(h, AMBIENT_INTEGRATION_GITHUB, text("10"), 1),
            1
        );
    }
    let mut out = vec![0; 1024 * 1024];
    let mut used = 0;
    unsafe {
        assert_eq!(
            ambient_integration_v1_profiles(
                h,
                AMBIENT_INTEGRATION_GITHUB,
                out.as_mut_ptr(),
                1024 * 1024,
                &mut used
            ),
            0
        );
    }
    let settings = std::str::from_utf8(&out[..used as usize])?;
    assert!(settings.starts_with("{\"exclusions\":[],\"instance\":\""));
    assert!(settings.contains("\"kind\":\"github\""));
    assert!(settings.contains("\"config\":[{\"owner\":\"Example\",\"repository\":\"Project\"}]"));
    assert!(settings.contains("\"config\":[]"));
    assert_eq!(settings.matches("\"scope\":null").count(), 2);
    assert_eq!(ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
#[test]
fn integration_switches_accept_all_four_kinds_and_a_closed_boolean()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-integration-switch-abi-{}.sqlite3",
        std::process::id()
    ));
    let bytes = path.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    unsafe {
        assert_eq!(
            ambient_v3_open(bytes.as_ptr(), bytes.len() as u32, &mut h),
            0
        );
    }
    let kinds = [
        AMBIENT_INTEGRATION_LOCAL_PROJECTS,
        AMBIENT_INTEGRATION_CODEX,
        AMBIENT_INTEGRATION_GITHUB,
        AMBIENT_INTEGRATION_JIRA,
        AMBIENT_INTEGRATION_TODOIST,
    ];
    for kind in kinds {
        let mut enabled = 7;
        unsafe {
            assert_eq!(ambient_integration_v1_enabled(h, kind, &mut enabled), 0);
        }
        assert_eq!(enabled, 1);
        assert_eq!(ambient_integration_v1_set_enabled(h, kind, 0), 0);
        // The same value is accepted and changes nothing.
        assert_eq!(ambient_integration_v1_set_enabled(h, kind, 0), 0);
        unsafe {
            assert_eq!(ambient_integration_v1_enabled(h, kind, &mut enabled), 0);
        }
        assert_eq!(enabled, 0);
    }
    // Profile exports still accept only the HTTP kinds.
    let mut used = 0;
    let mut out = vec![0u8; 1024 * 1024];
    unsafe {
        assert_eq!(
            ambient_integration_v1_profiles(
                h,
                AMBIENT_INTEGRATION_CODEX,
                out.as_mut_ptr(),
                1024 * 1024,
                &mut used
            ),
            INVALID_ARGUMENT
        );
    }
    let mut enabled = 7;
    for kind in [0, 6] {
        unsafe {
            assert_eq!(
                ambient_integration_v1_enabled(h, kind, &mut enabled),
                INVALID_ARGUMENT
            );
        }
        assert_eq!(
            ambient_integration_v1_set_enabled(h, kind, 1),
            INVALID_ARGUMENT
        );
    }
    assert_eq!(enabled, 7);
    assert_eq!(
        ambient_integration_v1_set_enabled(h, AMBIENT_INTEGRATION_JIRA, 2),
        INVALID_ARGUMENT
    );
    unsafe {
        assert_eq!(
            ambient_integration_v1_enabled(h, AMBIENT_INTEGRATION_JIRA, std::ptr::null_mut()),
            INVALID_ARGUMENT
        );
    }
    for kind in kinds {
        assert_eq!(ambient_integration_v1_set_enabled(h, kind, 1), 0);
    }
    assert_eq!(ambient_v3_close(h), 0);
    // Switch state survives reopen.
    unsafe {
        assert_eq!(
            ambient_v3_open(bytes.as_ptr(), bytes.len() as u32, &mut h),
            0
        );
        assert_eq!(
            ambient_integration_v1_enabled(h, AMBIENT_INTEGRATION_CODEX, &mut enabled),
            0
        );
    }
    assert_eq!(enabled, 1);
    assert_eq!(ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn todoist_profile_save_has_no_scope_argument_and_keeps_the_stored_account()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "ambient-integration-abi-todoist-{}.sqlite3",
        std::process::id()
    ));
    let bytes = path.to_str().ok_or("path")?.as_bytes();
    let mut h = 0;
    unsafe {
        assert_eq!(
            ambient_v3_open(bytes.as_ptr(), bytes.len() as u32, &mut h),
            0
        );
    }
    assert_eq!(AMBIENT_INTEGRATION_TODOIST, 5);
    let mut p = AmbientIntegrationProfileRef::default();
    let mut jira = AmbientIntegrationProfileRef::default();
    unsafe {
        assert_eq!(
            ambient_integration_v1_todoist_profile_save(h, 0, 0, text("Personal"), &mut p),
            0
        );
        assert_eq!(
            ambient_integration_v1_jira_profile_save(
                h,
                0,
                0,
                text("Work"),
                text("Site"),
                &mut jira
            ),
            0
        );
        // A profile ID of another kind is never saved as Todoist.
        let mut out = AmbientIntegrationProfileRef::default();
        assert_eq!(
            ambient_integration_v1_todoist_profile_save(
                h,
                jira.id,
                jira.settings_revision,
                text("Other"),
                &mut out
            ),
            INVALID_ARGUMENT
        );
        assert_eq!(out.id, 0);
        let mut renamed = AmbientIntegrationProfileRef::default();
        assert_eq!(
            ambient_integration_v1_todoist_profile_save(
                h,
                p.id,
                p.settings_revision,
                text("Renamed"),
                &mut renamed
            ),
            0
        );
        assert_eq!(renamed.id, p.id);
        assert!(renamed.settings_revision > p.settings_revision);
        assert_eq!(renamed.credential_generation, p.credential_generation);
        let mut out = vec![0u8; 1024 * 1024];
        let mut used = 0;
        assert_eq!(
            ambient_integration_v1_profiles(
                h,
                AMBIENT_INTEGRATION_TODOIST,
                out.as_mut_ptr(),
                1024 * 1024,
                &mut used
            ),
            0
        );
        let settings: serde_json::Value = serde_json::from_slice(&out[..used as usize])?;
        assert_eq!(settings["kind"], "todoist");
        assert_eq!(settings["profiles"][0]["name"], "Renamed");
        assert_eq!(settings["profiles"][0]["scope"], serde_json::Value::Null);
        assert_eq!(settings["profiles"][0]["config"], serde_json::json!({}));
        assert_eq!(
            ambient_integration_v1_scope_excluded(
                h,
                AMBIENT_INTEGRATION_TODOIST,
                text("account-1"),
                1
            ),
            0
        );
    }
    assert_eq!(ambient_v3_close(h), 0);
    std::fs::remove_file(path)?;
    Ok(())
}
