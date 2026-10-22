use ambient_core::{Store, StoreError};
use rusqlite::Connection;
use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};

fn wal_only_fixture(
    name: &str,
    version: i32,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("ambient-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let path = root.join("memory.sqlite3");
    let c = Connection::open(&path)?;
    c.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    )?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")?;
    c.execute_batch(if version == 7 {
        include_str!("fixtures/schema-seven.sql")
    } else if version == 8 {
        include_str!("fixtures/schema-eight.sql")
    } else if version == 9 {
        include_str!("fixtures/schema-nine.sql")
    } else if version == 10 {
        include_str!("fixtures/schema-ten.sql")
    } else if version == 11 {
        include_str!("fixtures/schema-eleven.sql")
    } else if version == 12 {
        include_str!("fixtures/schema-twelve.sql")
    } else if version == 13 {
        include_str!("fixtures/schema-thirteen.sql")
    } else if version == 14 {
        include_str!("fixtures/schema-fourteen.sql")
    } else if version == 15 {
        include_str!("fixtures/schema-fifteen.sql")
    } else if version == 16 {
        include_str!("fixtures/schema-sixteen.sql")
    } else {
        include_str!("../src/schema.sql")
    })?;
    c.execute(
        if version > 16 {
            "INSERT INTO settings VALUES(1,0,0,20,0,0,0,0)"
        } else {
            "INSERT INTO settings VALUES(1,0)"
        },
        [],
    )?;
    c.execute_batch("INSERT INTO local_source_clock VALUES(1,0); PRAGMA application_id=1095582257; INSERT INTO tasks(revision,text) VALUES(1,'preserve WAL memory');")?;
    if version > 10 {
        c.execute("INSERT INTO store_identity VALUES(1,randomblob(16))", [])?;
    }
    if version > 15 {
        c.execute_batch("INSERT INTO integration_switches VALUES('local-projects',1),('codex',1),('github',1),('jira',1),('todoist',1);")?;
    } else if version > 12 {
        c.execute_batch("INSERT INTO integration_switches VALUES('local-projects',1),('codex',1),('github',1),('jira',1);")?;
    }
    c.pragma_update(None, "user_version", version)?;
    drop(c);
    let main = std::fs::read(&path)?;
    assert_eq!(
        &main[60..64],
        &[0, 0, 0, 0],
        "fixture schema must exist only in WAL"
    );
    Ok(path)
}

fn database_bytes(path: &std::path::Path) -> Vec<Option<Vec<u8>>> {
    ["", "-wal", "-shm", "-journal"]
        .map(|suffix| std::fs::read(format!("{}{suffix}", path.display())).ok())
        .into()
}

#[test]
fn legacy_wal_only_schema_is_rejected_without_writing_any_file()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, replacement) in [
        ("wal-old-intact", None),
        ("wal-old-empty", Some(Vec::new())),
        ("wal-old-truncated", Some(vec![0x5a; 7])),
        ("wal-old-corrupt", Some(vec![0x5a; 32768])),
    ] {
        let path = wal_only_fixture(name, 7)?;
        if let Some(bytes) = replacement {
            std::fs::write(format!("{}-shm", path.display()), bytes)?;
        }
        let before = database_bytes(&path);
        assert!(
            matches!(Store::open(&path), Err(StoreError::UnsupportedSchema)),
            "{name}"
        );
        assert!(
            database_bytes(&path) == before,
            "{name}: rejected database files changed"
        );
        std::fs::remove_dir_all(path.parent().ok_or("fixture parent")?)?;
    }
    Ok(())
}

#[test]
fn schema_eight_wal_only_is_rejected_without_writing_any_file()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, replacement) in [
        ("wal-eight-intact", None),
        ("wal-eight-empty", Some(Vec::new())),
        ("wal-eight-truncated", Some(vec![0x5a; 7])),
        ("wal-eight-corrupt", Some(vec![0x5a; 32768])),
    ] {
        let path = wal_only_fixture(name, 8)?;
        if let Some(bytes) = replacement {
            std::fs::write(format!("{}-shm", path.display()), bytes)?;
        }
        let before = database_bytes(&path);
        assert!(
            matches!(Store::open(&path), Err(StoreError::UnsupportedSchema)),
            "{name}"
        );
        assert!(
            database_bytes(&path) == before,
            "{name}: rejected database files changed"
        );
        std::fs::remove_dir_all(path.parent().ok_or("fixture parent")?)?;
    }
    Ok(())
}

#[test]
fn schema_nine_through_sixteen_wal_only_are_rejected_without_writing_any_file()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, replacement, version) in [
        ("wal-nine-intact", None, 9),
        ("wal-nine-empty", Some(Vec::new()), 9),
        ("wal-nine-truncated", Some(vec![0x5a; 7]), 9),
        ("wal-nine-corrupt", Some(vec![0x5a; 32768]), 9),
        ("wal-ten-intact", None, 10),
        ("wal-ten-empty", Some(Vec::new()), 10),
        ("wal-ten-truncated", Some(vec![0x5a; 7]), 10),
        ("wal-ten-corrupt", Some(vec![0x5a; 32768]), 10),
        ("wal-eleven-intact", None, 11),
        ("wal-eleven-empty", Some(Vec::new()), 11),
        ("wal-eleven-truncated", Some(vec![0x5a; 7]), 11),
        ("wal-eleven-corrupt", Some(vec![0x5a; 32768]), 11),
        ("wal-twelve-intact", None, 12),
        ("wal-twelve-empty", Some(Vec::new()), 12),
        ("wal-twelve-truncated", Some(vec![0x5a; 7]), 12),
        ("wal-twelve-corrupt", Some(vec![0x5a; 32768]), 12),
        ("wal-thirteen-intact", None, 13),
        ("wal-thirteen-empty", Some(Vec::new()), 13),
        ("wal-thirteen-truncated", Some(vec![0x5a; 7]), 13),
        ("wal-thirteen-corrupt", Some(vec![0x5a; 32768]), 13),
        ("wal-fourteen-intact", None, 14),
        ("wal-fourteen-empty", Some(Vec::new()), 14),
        ("wal-fourteen-truncated", Some(vec![0x5a; 7]), 14),
        ("wal-fourteen-corrupt", Some(vec![0x5a; 32768]), 14),
        ("wal-fifteen-intact", None, 15),
        ("wal-fifteen-empty", Some(Vec::new()), 15),
        ("wal-fifteen-truncated", Some(vec![0x5a; 7]), 15),
        ("wal-fifteen-corrupt", Some(vec![0x5a; 32768]), 15),
        ("wal-sixteen-intact", None, 16),
        ("wal-sixteen-empty", Some(Vec::new()), 16),
        ("wal-sixteen-truncated", Some(vec![0x5a; 7]), 16),
        ("wal-sixteen-corrupt", Some(vec![0x5a; 32768]), 16),
    ] {
        let path = wal_only_fixture(name, version)?;
        if let Some(bytes) = replacement {
            std::fs::write(format!("{}-shm", path.display()), bytes)?;
        }
        let before = database_bytes(&path);
        assert!(
            matches!(Store::open(&path), Err(StoreError::UnsupportedSchema)),
            "{name}"
        );
        assert!(
            database_bytes(&path) == before,
            "{name}: rejected database files changed"
        );
        std::fs::remove_dir_all(path.parent().ok_or("fixture parent")?)?;
    }
    Ok(())
}

#[test]
fn current_wal_only_schema_opens_with_existing_shm_and_reopens_normally()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, replacement) in [
        ("wal-current-intact", None),
        ("wal-current-empty", Some(Vec::new())),
        ("wal-current-truncated", Some(vec![0x5a; 7])),
        ("wal-current-corrupt", Some(vec![0x5a; 32768])),
    ] {
        let path = wal_only_fixture(name, 17)?;
        if let Some(bytes) = replacement {
            std::fs::write(format!("{}-shm", path.display()), bytes)?;
        }
        let mut store = Store::open(&path)?;
        let old = store.tasks(0, 50)?;
        assert_eq!(old.items.len(), 1, "{name}");
        assert_eq!(old.items[0].text, "preserve WAL memory", "{name}");
        let new = store.create_task("ordinary reopen")?;
        drop(store);
        let reopened = Store::open(&path)?;
        assert_eq!(reopened.task(new.id)?.text, "ordinary reopen", "{name}");
        drop(reopened);
        std::fs::remove_dir_all(path.parent().ok_or("fixture parent")?)?;
    }
    Ok(())
}

#[test]
fn missing_shm_is_storage_failure_without_recovery_or_new_sidecars()
-> Result<(), Box<dyn std::error::Error>> {
    for (name, version) in [
        ("wal-old-missing", 7),
        ("wal-eight-missing", 8),
        ("wal-nine-missing", 9),
        ("wal-ten-missing", 10),
        ("wal-eleven-missing", 11),
        ("wal-twelve-missing", 12),
        ("wal-thirteen-missing", 13),
        ("wal-fourteen-missing", 14),
        ("wal-fifteen-missing", 15),
        ("wal-sixteen-missing", 16),
        ("wal-current-missing", 17),
    ] {
        let path = wal_only_fixture(name, version)?;
        std::fs::remove_file(format!("{}-shm", path.display()))?;
        let before = database_bytes(&path);
        assert!(
            matches!(Store::open(&path), Err(StoreError::Storage)),
            "{name}"
        );
        assert!(
            database_bytes(&path) == before,
            "{name}: failed open changed files"
        );
        std::fs::remove_dir_all(path.parent().ok_or("fixture parent")?)?;
    }
    Ok(())
}

#[test]
fn schema_six_wal_is_rejected_without_changing_any_existing_sidecar()
-> Result<(), Box<dyn std::error::Error>> {
    const CHILD_PATH: &str = "AMBIENT_SCHEMA_SIX_CHILD";
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        let c = Connection::open(path)?;
        c.execute_batch(include_str!("fixtures/schema-seven.sql"))?;
        c.execute_batch("INSERT INTO settings VALUES(1,0); INSERT INTO local_source_clock VALUES(1,0); PRAGMA application_id=1095582257; PRAGMA user_version=7; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO tasks(revision,text) VALUES(1,'preserve old memory');")?;
        println!("legacy-writer-ready");
        std::io::stdout().flush()?;
        // The previous app is another process; keep its connection alive without more SQL.
        std::io::stdin().read_to_end(&mut Vec::new())?;
        drop(c);
        return Ok(());
    }
    let path = std::env::temp_dir().join(format!(
        "ambient-schema-six-wal-{}.sqlite3",
        std::process::id()
    ));
    let mut child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "schema_six_wal_is_rejected_without_changing_any_existing_sidecar",
            "--nocapture",
        ])
        .env(CHILD_PATH, &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut output = std::io::BufReader::new(child.stdout.take().ok_or("child stdout")?);
    let observation = (|| -> Result<_, Box<dyn std::error::Error>> {
        let mut ready = false;
        for line in (&mut output).lines() {
            if line?.contains("legacy-writer-ready") {
                ready = true;
                break;
            }
        }
        if !ready {
            return Err("legacy writer did not become ready".into());
        }
        let before = database_bytes(&path);
        let rejected = matches!(Store::open(&path), Err(StoreError::UnsupportedSchema));
        let after = database_bytes(&path);
        Ok((before, after, rejected, child.try_wait()?.is_none()))
    })();
    drop(child.stdin.take());
    output.read_to_end(&mut Vec::new())?;
    let exited = child.wait()?;
    let (before, after, rejected, writer_alive) = observation?;
    assert!(
        writer_alive && exited.success(),
        "previous app writer lifecycle"
    );
    assert!(
        before[..3].iter().all(Option::is_some),
        "DB/WAL/SHM must exist"
    );
    assert!(rejected, "legacy schema must be rejected");
    assert!(
        before == after,
        "rejected DB/WAL/SHM/journal bytes or existence changed"
    );
    std::fs::remove_file(path)?;
    Ok(())
}
#[test]
fn schema_one_is_rejected_without_changing_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let path =
        std::env::temp_dir().join(format!("ambient-schema-one-{}.sqlite3", std::process::id()));
    let connection = Connection::open(&path)?;
    connection.execute_batch("CREATE TABLE assistant_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    intent_state INTEGER NOT NULL CHECK (intent_state IN (0, 1, 2)),
    text TEXT,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    manual_pause INTEGER NOT NULL CHECK (manual_pause IN (0, 1)),
    CHECK ((intent_state = 0 AND text IS NULL AND revision = 0)
        OR (intent_state = 1 AND text IS NOT NULL AND revision > 0)
        OR (intent_state = 2 AND text IS NULL AND revision > 0))
) STRICT; INSERT INTO assistant_state VALUES (1,1,'Preserve old memory',1,0); PRAGMA application_id=1095582257; PRAGMA user_version=1;")?;
    drop(connection);
    let before = std::fs::read(&path)?;
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&path)?, before);
    std::fs::remove_file(path)?;
    Ok(())
}
#[test]
fn rejected_wal_schema_does_not_checkpoint_legacy_data() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("ambient-wal-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let source = root.join("source.sqlite3");
    let target = root.join("legacy.sqlite3");
    let c = Connection::open(&source)?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE private(value TEXT); INSERT INTO private VALUES('legacy'); PRAGMA user_version=1;")?;
    std::fs::copy(&source, &target)?;
    let wal = root.join("legacy.sqlite3-wal");
    std::fs::copy(root.join("source.sqlite3-wal"), &wal)?;
    let before = std::fs::read(&target)?;
    let wal_before = std::fs::read(&wal)?;
    assert!(matches!(Store::open(&target), Err(StoreError::Storage)));
    assert_eq!(std::fs::read(&target)?, before);
    assert_eq!(std::fs::read(&wal)?, wal_before);
    drop(c);
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn hot_rollback_journal_fails_without_recovery_writes() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("ambient-hot-journal-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let source = root.join("source.sqlite3");
    let path = root.join("crashed.sqlite3");
    let c = Connection::open(&source)?;
    c.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA cache_size=4; CREATE TABLE private(value TEXT); PRAGMA user_version=7; WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100) INSERT INTO private SELECT printf('%08192d',x) FROM n; BEGIN IMMEDIATE; UPDATE private SET value=printf('%08192d',42);")?;
    // Copy a synthetic spilled transaction while its writer is alive to model a crash.
    std::fs::copy(&source, &path)?;
    std::fs::copy(
        format!("{}-journal", source.display()),
        format!("{}-journal", path.display()),
    )?;
    c.execute_batch("ROLLBACK;")?;
    drop(c);
    let journal = std::fs::read(format!("{}-journal", path.display()))?;
    assert_eq!(
        &journal[..8],
        &[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]
    );
    let before = database_bytes(&path);
    assert!(matches!(Store::open(&path), Err(StoreError::Storage)));
    assert!(
        database_bytes(&path) == before,
        "hot journal recovery changed rejected files"
    );
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn schema_six_is_rejected_without_writes_or_sidecars() -> Result<(), Box<dyn std::error::Error>> {
    let path =
        std::env::temp_dir().join(format!("ambient-schema-six-{}.sqlite3", std::process::id()));
    let c = rusqlite::Connection::open(&path)?;
    c.execute_batch(include_str!("fixtures/schema-seven.sql"))?;
    c.execute_batch("INSERT INTO settings VALUES(1,0); INSERT INTO local_source_clock VALUES(1,0); PRAGMA application_id=1095582257; PRAGMA user_version=7;")?;
    drop(c);
    let before = std::fs::read(&path)?;
    assert!(matches!(
        ambient_core::Store::open(&path),
        Err(ambient_core::StoreError::UnsupportedSchema)
    ));
    assert_eq!(std::fs::read(&path)?, before);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!std::path::Path::new(&format!("{}{suffix}", path.display())).exists());
    }
    std::fs::remove_file(path)?;
    Ok(())
}
