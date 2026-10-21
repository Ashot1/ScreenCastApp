use super::*;
use crate::codex_types::codex_hex;
use crate::plugin_package::{ParsedPackage, parse_package, pinned_definition, valid_server_key};
use integrations::{clock, sql_id};
use serde_json::{Value, json};

/// One installed plugin row; the package was validated at install and is re-validated at open.
pub(super) struct PluginRow {
    pub id: u64,
    pub revision: u64,
    pub enabled: bool,
    pub name: String,
    pub version: String,
    pub hash: [u8; 32],
    pub installed_at_utc: i64,
    pub package: PluginPackage,
}
const PLUGIN_SELECT: &str =
    "SELECT id,revision,enabled,name,version,package_hash,installed_at_utc,package FROM plugins";
fn read_row(r: &Row<'_>) -> rusqlite::Result<Result<PluginRow, StoreError>> {
    let id: i64 = r.get(0)?;
    let revision: i64 = r.get(1)?;
    let hash: Vec<u8> = r.get(5)?;
    let package: String = r.get(7)?;
    let enabled: i64 = r.get(2)?;
    let (name, version, installed_at_utc) = (r.get(3)?, r.get(4)?, r.get(6)?);
    Ok((|| {
        Ok(PluginRow {
            id: positive(id)?,
            revision: positive(revision)?,
            enabled: enabled == 1,
            name,
            version,
            hash: hash.try_into().map_err(|_| StoreError::UnsupportedSchema)?,
            installed_at_utc,
            package: serde_json::from_str(&package).map_err(|_| StoreError::UnsupportedSchema)?,
        })
    })())
}
fn positive(v: i64) -> Result<u64, StoreError> {
    u64::try_from(v)
        .ok()
        .filter(|v| *v > 0)
        .ok_or(StoreError::UnsupportedSchema)
}
type ToolRow = (String, String, String, Vec<u8>);
/// Pinned tools of one plugin as (server, name, definition, hash), by server and name.
fn tool_rows(c: &Connection, id: u64) -> Result<Vec<ToolRow>, StoreError> {
    let mut q = c.prepare(
        "SELECT server,name,definition,hash FROM plugin_tools WHERE plugin_id=?1 ORDER BY server,name",
    )?;
    let rows = q
        .query_map([sql_id(id)?], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
pub(super) fn plugins(c: &Connection) -> Result<Vec<PluginRow>, StoreError> {
    let mut q = c.prepare(&format!("{PLUGIN_SELECT} ORDER BY id LIMIT 17"))?;
    let rows = q
        .query_map([], read_row)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > MAX_PLUGINS {
        return Err(StoreError::UnsupportedSchema);
    }
    Ok(rows)
}
pub(super) fn plugin(c: &Connection, id: u64) -> Result<PluginRow, StoreError> {
    c.query_row(
        &format!("{PLUGIN_SELECT} WHERE id=?1"),
        [sql_id(id)?],
        read_row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)?
}
/// The plugin at the revision the caller saw; any other revision means it changed meanwhile.
fn current(c: &Connection, id: u64, expected: u64) -> Result<PluginRow, StoreError> {
    let p = plugin(c, id)?;
    if p.revision != expected {
        return Err(StoreError::StaleRevision);
    }
    Ok(p)
}
pub(super) fn generation(c: &Connection, id: u64, server: &str) -> Result<u64, StoreError> {
    let g: i64 = c
        .query_row(
            "SELECT generation FROM plugin_servers WHERE plugin_id=?1 AND key=?2",
            params![sql_id(id)?, server],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::NotFound)?;
    positive(g)
}
fn set_revision(c: &Connection, id: u64) -> Result<u64, StoreError> {
    let next = clock(c)?;
    c.execute(
        "UPDATE plugins SET revision=?1 WHERE id=?2",
        params![sql_id(next)?, sql_id(id)?],
    )?;
    Ok(next)
}
/// Logout, key replacement and 401 all move the server generation and the plugin revision to one
/// new clock value; saved results are untouched.
pub(super) fn revoke(c: &Connection, id: u64, server: &str) -> Result<u64, StoreError> {
    generation(c, id, server)?;
    let next = set_revision(c, id)?;
    c.execute(
        "UPDATE plugin_servers SET generation=?1 WHERE plugin_id=?2 AND key=?3",
        params![sql_id(next)?, sql_id(id)?, server],
    )?;
    Ok(next)
}
fn hash_hex(text: &str) -> Result<[u8; 32], StoreError> {
    let bytes = text.as_bytes();
    if bytes.len() != 64
        || !bytes
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return Err(StoreError::InvalidInput);
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[2 * i..2 * i + 2], 16)
            .map_err(|_| StoreError::InvalidInput)?;
    }
    Ok(out)
}
/// A stored server passes the same validation as at install.
fn stored_kind_valid(kind: &ServerKind) -> bool {
    match kind {
        ServerKind::Remote { url, .. } => {
            crate::plugin_package::remote_url(url).as_deref() == Some(url)
        }
        ServerKind::Local { command, args, env } => {
            crate::plugin_package::local_kind(command, args, env.clone())
                .ok()
                .flatten()
                .as_ref()
                == Some(kind)
        }
        // The serde tag already parsed the id. Its origin was checked at install, so a newer
        // built-in package in a later release never makes the installed plugin unopenable.
        ServerKind::Managed { .. } => true,
    }
}
/// Open-time validation: bounded rows, the package and every pin re-validated, values not
/// above the clock. Saved results stay valid history even after their plugin is removed.
pub(super) fn validate_plugins(c: &Connection) -> Result<(), StoreError> {
    let clock: i64 = c.query_row(
        "SELECT generation FROM local_source_clock WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let above = |v: u64| v > clock as u64;
    for p in plugins(c)? {
        if above(p.id) || above(p.revision) || p.package.servers.len() > MAX_PLUGIN_SERVERS {
            return Err(StoreError::UnsupportedSchema);
        }
        let mut q =
            c.prepare("SELECT key,generation FROM plugin_servers WHERE plugin_id=?1 ORDER BY key")?;
        let servers = q
            .query_map([sql_id(p.id)?], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut keys = p
            .package
            .servers
            .iter()
            .map(|s| s.key.clone())
            .collect::<Vec<_>>();
        keys.sort();
        if servers.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>() != keys
            || p.package
                .servers
                .iter()
                .any(|s| !valid_server_key(&s.key) || !stored_kind_valid(&s.kind))
        {
            return Err(StoreError::UnsupportedSchema);
        }
        for (_, g) in servers {
            if above(positive(g)?) {
                return Err(StoreError::UnsupportedSchema);
            }
        }
        let tools = tool_rows(c, p.id)?;
        let mut per_server = std::collections::BTreeMap::<String, usize>::new();
        for (server, name, definition, hash) in tools {
            let t = pinned_definition(&definition).map_err(|_| StoreError::UnsupportedSchema)?;
            let count = per_server.entry(server).or_default();
            *count += 1;
            if t.name != name || t.hash.as_slice() != hash || *count > MAX_LISTED_TOOLS {
                return Err(StoreError::UnsupportedSchema);
            }
        }
    }
    let calls: i64 = c.query_row("SELECT count(*) FROM plugin_calls", [], |r| r.get(0))?;
    if calls as usize > MAX_CALL_LOG {
        return Err(StoreError::UnsupportedSchema);
    }
    validate_results(c)
}

impl Store {
    /// Install re-parses exactly the reviewed bytes: a different hash means the folder changed.
    pub fn install_plugin(
        &mut self,
        files: &[PluginFile<'_>],
        expected_hash: &str,
        at_utc: i64,
    ) -> Result<u64, StoreError> {
        let expected = hash_hex(expected_hash)?;
        let ParsedPackage {
            name,
            version,
            hash,
            package,
            ..
        } = parse_package(files)?;
        if hash != expected {
            return Err(StoreError::StaleRevision);
        }
        let json = serde_json::to_string(&package).map_err(|_| StoreError::InvalidInput)?;
        self.change(|c| {
            let count: i64 = c.query_row("SELECT count(*) FROM plugins", [], |r| r.get(0))?;
            if count as usize >= MAX_PLUGINS {
                return Err(StoreError::ResourceLimit);
            }
            byte_room(c, json.len() as i64)?;
            let id = sql_id(clock(c)?)?;
            c.execute(
                "INSERT INTO plugins VALUES(?1,?1,0,?2,?3,?4,?5,?6)",
                params![id, name, version, hash.to_vec(), at_utc, json],
            )?;
            for s in &package.servers {
                c.execute(
                    "INSERT INTO plugin_servers VALUES(?1,?2,?1)",
                    params![id, s.key],
                )?;
            }
            Ok(id as u64)
        })
    }
    pub fn plugins_json(&self) -> Result<String, StoreError> {
        let c = &self.connection;
        let mut out = Vec::new();
        for p in plugins(c)? {
            let servers = p
                .package
                .servers
                .iter()
                .map(|s| {
                    let mut v = crate::plugin_package::server_json(s)?;
                    v["generation"] = json!(generation(c, p.id, &s.key)?);
                    Ok(v)
                })
                .collect::<Result<Vec<_>, StoreError>>()?;
            let mut q = c.prepare(
                "SELECT server,name,hash FROM plugin_tools WHERE plugin_id=?1 ORDER BY server,name",
            )?;
            let tools = q
                .query_map([sql_id(p.id)?], |r| {
                    Ok(json!({"server":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"hash":codex_hex(&r.get::<_,Vec<u8>>(2)?)}))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            out.push(json!({"id":p.id,"revision":p.revision,"enabled":p.enabled,"name":p.name,"version":p.version,"hash":codex_hex(&p.hash),"installed_at_utc":p.installed_at_utc,"servers":servers,"skills":p.package.skills,"tools":tools}));
        }
        Ok(json!({"instance":integrations::instance(c)?,"plugins":out}).to_string())
    }
    /// The user-visible call log, newest first.
    pub fn plugin_calls_json(&self) -> Result<String, StoreError> {
        let mut q = self.connection.prepare("SELECT id,at_utc,plugin_id,plugin_name,server,tool,arguments,stage,outcome,result_bytes FROM plugin_calls ORDER BY id DESC LIMIT ?1")?;
        let rows = q
            .query_map([MAX_CALL_LOG as i64], |r| {
                Ok(json!({"id":r.get::<_,i64>(0)?,"at_utc":r.get::<_,i64>(1)?,"plugin_id":r.get::<_,i64>(2)?,"plugin_name":r.get::<_,String>(3)?,"server":r.get::<_,String>(4)?,"tool":r.get::<_,String>(5)?,"arguments":r.get::<_,String>(6)?,"stage":r.get::<_,i64>(7)?,"outcome":r.get::<_,i64>(8)?,"result_bytes":r.get::<_,i64>(9)?}))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Value::Array(rows).to_string())
    }
    /// The target a blocking discovery may contact with the key of exactly this generation.
    pub fn plugin_server(
        &self,
        id: u64,
        server: &str,
        expected: u64,
    ) -> Result<Target, StoreError> {
        let p = plugin(&self.connection, id)?;
        if generation(&self.connection, id, server)? != expected {
            return Err(StoreError::StaleRevision);
        }
        p.package
            .servers
            .into_iter()
            .find(|s| s.key == server)
            .map(|s| s.kind.target())
            .ok_or(StoreError::UnsupportedSchema)
    }
    /// After a request: a server whose process group was not confirmed gone gets «stop not
    /// confirmed» on its newest call row. A started server always has one, committed before the
    /// call. After a cancellation that row may still be stage 1 (the result was never accepted),
    /// so it is completed here too; this needs no active request.
    pub fn record_unconfirmed_stops(
        &mut self,
        servers: &[(u64, String)],
    ) -> Result<(), StoreError> {
        if servers.is_empty() {
            return Ok(());
        }
        self.change(|c| {
            for (plugin, server) in servers {
                c.execute(
                    "UPDATE plugin_calls SET stage=2,outcome=?1,result_bytes=CASE WHEN stage=1 THEN 0 ELSE result_bytes END WHERE id=(SELECT max(id) FROM plugin_calls WHERE plugin_id=?2 AND server=?3)",
                    params![PluginCallOutcome::StopUnconfirmed as u32, sql_id(*plugin)?, server],
                )?;
            }
            Ok(())
        })
    }
    /// Pins one declared read-only tool. A new pin keeps the revision; re-pinning a different
    /// definition bumps it, so a running request offered with the old pin is cancelled.
    pub fn approve_plugin_tool(
        &mut self,
        id: u64,
        expected: u64,
        server: &str,
        definition: &str,
        expected_hash: &str,
    ) -> Result<u64, StoreError> {
        let t = pinned_definition(definition)?;
        if t.hash != hash_hex(expected_hash)? {
            return Err(StoreError::InvalidInput);
        }
        self.change(|c| {
            let p = current(c, id, expected)?;
            generation(c, id, server)?;
            if let Some(ServerKind::Managed { id: managed }) = p
                .package
                .servers
                .iter()
                .find(|s| s.key == server)
                .map(|s| &s.kind)
                && !managed.allows(&t.name)
            {
                return Err(StoreError::InvalidInput);
            }
            let old: Option<String> = c
                .query_row(
                    "SELECT definition FROM plugin_tools WHERE plugin_id=?1 AND server=?2 AND name=?3",
                    params![sql_id(id)?, server, t.name],
                    |r| r.get(0),
                )
                .optional()?;
            match old {
                Some(old) if old == t.definition => Ok(expected),
                Some(_) => {
                    byte_room(c, t.definition.len() as i64)?;
                    c.execute(
                        "UPDATE plugin_tools SET definition=?1,hash=?2 WHERE plugin_id=?3 AND server=?4 AND name=?5",
                        params![t.definition, t.hash.to_vec(), sql_id(id)?, server, t.name],
                    )?;
                    set_revision(c, id)
                }
                None => {
                    let count: i64 = c.query_row(
                        "SELECT count(*) FROM plugin_tools WHERE plugin_id=?1 AND server=?2",
                        params![sql_id(id)?, server],
                        |r| r.get(0),
                    )?;
                    if count as usize >= MAX_LISTED_TOOLS {
                        return Err(StoreError::ResourceLimit);
                    }
                    byte_room(c, t.definition.len() as i64)?;
                    c.execute(
                        "INSERT INTO plugin_tools VALUES(?1,?2,?3,?4,?5)",
                        params![sql_id(id)?, server, t.name, t.definition, t.hash.to_vec()],
                    )?;
                    Ok(expected)
                }
            }
        })
    }
    pub fn unapprove_plugin_tool(
        &mut self,
        id: u64,
        expected: u64,
        server: &str,
        name: &str,
    ) -> Result<u64, StoreError> {
        self.change(|c| {
            current(c, id, expected)?;
            let removed = c.execute(
                "DELETE FROM plugin_tools WHERE plugin_id=?1 AND server=?2 AND name=?3",
                params![sql_id(id)?, server, name],
            )?;
            if removed != 1 {
                return Err(StoreError::NotFound);
            }
            set_revision(c, id)
        })
    }
    pub fn set_plugin_enabled(&mut self, id: u64, enabled: bool) -> Result<(), StoreError> {
        // The same value must not reach `change`, which always runs `clear_revoked`.
        if plugin(&self.connection, id)?.enabled == enabled {
            return Ok(());
        }
        self.change(|c| {
            c.execute(
                "UPDATE plugins SET enabled=?1 WHERE id=?2",
                params![enabled, sql_id(id)?],
            )?;
            // Disabling bumps the revision: the snapshot fence cancels a running request.
            if !enabled {
                set_revision(c, id)?;
            }
            Ok(())
        })
    }
    pub fn revoke_plugin_credential(
        &mut self,
        id: u64,
        expected: u64,
        server: &str,
    ) -> Result<u64, StoreError> {
        self.change(|c| {
            current(c, id, expected)?;
            revoke(c, id, server)
        })
    }
    /// Removal step 1: disable and revoke every server key at one clock value; native then
    /// deletes the Keychain entries and finishes with that value.
    pub fn begin_plugin_removal(&mut self, id: u64, expected: u64) -> Result<u64, StoreError> {
        self.change(|c| {
            current(c, id, expected)?;
            let next = set_revision(c, id)?;
            c.execute("UPDATE plugins SET enabled=0 WHERE id=?1", [sql_id(id)?])?;
            c.execute(
                "UPDATE plugin_servers SET generation=?1 WHERE plugin_id=?2",
                params![sql_id(next)?, sql_id(id)?],
            )?;
            Ok(next)
        })
    }
    pub fn finish_plugin_removal(&mut self, id: u64, revision: u64) -> Result<(), StoreError> {
        self.change(|c| {
            let p = current(c, id, revision)?;
            let stale: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM plugin_servers WHERE plugin_id=?1 AND generation!=?2)",
                params![sql_id(id)?, sql_id(revision)?],
                |r| r.get(0),
            )?;
            if p.enabled || stale {
                return Err(StoreError::StaleRevision);
            }
            c.execute("DELETE FROM plugins WHERE id=?1", [sql_id(id)?])?;
            Ok(())
        })
    }
}

/// Offer limits per assistance request (ADR-021).
/// One result in the model context: 12 KiB of text plus its envelope.
pub(super) const RESULT_TEXT: usize = 12 * 1024;
pub(super) const RESULT_ROOM: usize = RESULT_TEXT + 1024;

/// A saved result stays usable while its plugin exists and is enabled; key changes, re-login and
/// re-pinning never withdraw facts already saved (ADR-021).
pub(super) fn enabled(c: &Connection, id: u64) -> Result<(), StoreError> {
    match plugin(c, id) {
        Ok(p) if p.enabled => Ok(()),
        Ok(_) | Err(StoreError::NotFound) => Err(StoreError::StaleRevision),
        Err(e) => Err(e),
    }
}
pub(super) fn authorized(c: &Connection, m: &PluginSavedMetadata) -> Result<(), StoreError> {
    enabled(c, m.metadata.plugin_id)
}
pub(super) fn check_saved(c: &Connection, m: &PluginSavedMetadata) -> Result<(), StoreError> {
    authorized(c, m)?;
    if metadata(c, m.binding_id)? != *m {
        return Err(StoreError::StaleRevision);
    }
    Ok(())
}
/// Open-time comparison after the task revision changed: the old identity must still hold.
pub(super) fn same_ground(
    c: &Connection,
    old: &PluginSavedMetadata,
    now: &PluginSavedMetadata,
) -> Result<(), StoreError> {
    authorized(c, old)?;
    validate_result(&old.metadata)?;
    let (a, b) = (&old.metadata, &now.metadata);
    if old.binding_id != now.binding_id
        || old.evidence_id != now.evidence_id
        || a.plugin_id != b.plugin_id
        || a.server != b.server
        || a.tool != b.tool
        || a.arguments != b.arguments
    {
        return Err(StoreError::UnsupportedSchema);
    }
    Ok(())
}
pub(super) fn result_key(m: &PluginResultMetadata) -> SourceKey {
    SourceKey {
        namespace: PLUGIN_NAMESPACE.into(),
        scope: m.plugin_id.to_string(),
        object_id: format!(
            "{}/{}:{}",
            m.server,
            m.tool,
            codex_hex(&crate::plugin_package::sha256(m.arguments.as_bytes()))
        ),
    }
}
/// Text, source key, version and metadata of one saved result share the 16 KiB object limit;
/// receipt promises and open-time validation use this one count.
pub(super) fn result_bytes(m: &PluginResultMetadata, text: usize) -> Result<usize, StoreError> {
    let data = serde_json::to_string(m).map_err(|_| StoreError::InvalidInput)?;
    Ok(text + result_key(m).bytes() + m.content_hash.len() + data.len())
}
/// The plugin exactly at an offered revision and still enabled; anything else is stale.
pub(super) fn at_revision(c: &Connection, v: PluginVersion) -> Result<(), StoreError> {
    match plugin(c, v.id) {
        Ok(p) if p.enabled && p.revision == v.revision => Ok(()),
        Ok(_) | Err(StoreError::NotFound) => Err(StoreError::StaleRevision),
        Err(e) => Err(e),
    }
}
pub(super) fn validate_result(m: &PluginResultMetadata) -> Result<(), StoreError> {
    let args: Value =
        serde_json::from_str(&m.arguments).map_err(|_| StoreError::UnsupportedSchema)?;
    if m.plugin_id == 0
        || m.plugin_id > i64::MAX as u64
        || !valid_server_key(&m.server)
        || !crate::plugin_package::valid_tool_name(&m.tool)
        || !(1..=256).contains(&m.plugin_name.len())
        || m.plugin_version.len() > 64
        || m.arguments.len() > MAX_TOOL_ARGUMENTS
        || !args.is_object()
        || serde_json::to_string(&args).ok().as_deref() != Some(m.arguments.as_str())
        || hash_hex(&m.content_hash).is_err()
    {
        return Err(StoreError::UnsupportedSchema);
    }
    Ok(())
}
pub(super) fn metadata(c: &Connection, binding: i64) -> Result<PluginSavedMetadata, StoreError> {
    let (evidence, namespace, data): (i64, String, String) = c.query_row(
        "SELECT b.evidence_id,b.namespace,m.data FROM source_bindings b JOIN integration_metadata m ON m.binding_id=b.id WHERE b.id=?1 AND b.namespace IS NOT NULL",
        [binding],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if namespace != PLUGIN_NAMESPACE {
        return Err(StoreError::UnsupportedSchema);
    }
    let metadata: PluginResultMetadata =
        serde_json::from_str(&data).map_err(|_| StoreError::UnsupportedSchema)?;
    validate_result(&metadata)?;
    Ok(PluginSavedMetadata {
        binding_id: binding,
        evidence_id: EvidenceId(evidence),
        metadata,
    })
}
pub(super) fn stored_metadata(
    c: &Connection,
    e: EvidenceId,
) -> Result<PluginSavedMetadata, StoreError> {
    let b = c.query_row(
        "SELECT id FROM source_bindings WHERE evidence_id=?1 AND namespace IS NOT NULL",
        [e.0],
        |r| r.get(0),
    )?;
    metadata(c, b)
}
/// Every saved plugin result has metadata naming its own source key and content version.
pub(super) fn validate_results(c: &Connection) -> Result<(), StoreError> {
    let missing: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM source_bindings b WHERE b.namespace=?1 AND NOT EXISTS(SELECT 1 FROM integration_metadata m WHERE m.binding_id=b.id))",
        [PLUGIN_NAMESPACE],
        |r| r.get(0),
    )?;
    if missing {
        return Err(StoreError::UnsupportedSchema);
    }
    let clock: i64 = c.query_row(
        "SELECT generation FROM local_source_clock WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let mut q = c.prepare("SELECT id FROM source_bindings WHERE namespace=?1 ORDER BY id")?;
    let ids = q
        .query_map([PLUGIN_NAMESPACE], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for b in ids {
        let m = metadata(c, b)?;
        let e = read_evidence(c, m.evidence_id)?;
        let Origin::External { key, version } = e.origin else {
            return Err(StoreError::UnsupportedSchema);
        };
        if key != result_key(&m.metadata)
            || version.as_deref() != Some(m.metadata.content_hash.as_str())
            || m.metadata.plugin_id > clock as u64
            || result_bytes(&m.metadata, e.text.len())? > MAX_TEXT_BYTES
        {
            return Err(StoreError::UnsupportedSchema);
        }
    }
    Ok(())
}
/// The plugin part of a new assistance preview: enabled plugins with approved tools, in install
/// order, within the definition and skill budgets. `None` keeps the request exactly as before.
pub(super) fn offer(
    c: &Connection,
    selected: usize,
    context: usize,
) -> Result<Option<(PluginOffer, Value)>, StoreError> {
    let mut candidates = Vec::new();
    for p in plugins(c)?.into_iter().filter(|p| p.enabled) {
        let mut tools = tool_rows(c, p.id)?;
        // Servers in package order, tools by name within a server.
        tools.sort_by_key(|t| {
            p.package
                .servers
                .iter()
                .position(|s| s.key == t.0)
                .unwrap_or(usize::MAX)
        });
        if !tools.is_empty() {
            candidates.push((p, tools));
        }
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    let citable = 32usize.saturating_sub(selected);
    let room = citable > 0 && context + RESULT_ROOM <= MAX_ASSISTANCE_CONTEXT;
    let mut offer = PluginOffer {
        citable,
        ..PluginOffer::default()
    };
    let (mut offered, mut excluded, mut skills) = (Vec::new(), Vec::new(), Vec::new());
    let (mut definitions, mut skill_bytes) = (0usize, 0usize);
    for (p, tools) in &candidates {
        let mut any = false;
        for (server, name, definition, hash) in tools {
            let spec = p
                .package
                .servers
                .iter()
                .find(|s| s.key == *server)
                .ok_or(StoreError::UnsupportedSchema)?;
            let reason = if !room {
                Some("no_room_for_results")
            } else if definitions + definition.len() > MAX_OFFERED_DEFINITIONS {
                Some("definitions_limit")
            } else {
                None
            };
            if let Some(reason) = reason {
                excluded.push(json!({"plugin":p.name,"server":server,"tool":name,"reason":reason}));
                continue;
            }
            definitions += definition.len();
            any = true;
            offer.tools.push(PinnedTool {
                plugin: p.id,
                plugin_name: p.name.clone(),
                plugin_version: p.version.clone(),
                server: server.clone(),
                kind: spec.kind.clone(),
                name: name.clone(),
                model_name: String::new(),
                definition: definition.clone(),
                hash: hash
                    .clone()
                    .try_into()
                    .map_err(|_| StoreError::UnsupportedSchema)?,
            });
        }
        if !any {
            continue;
        }
        offer.versions.push(PluginVersion {
            id: p.id,
            revision: p.revision,
        });
        // Skills of offered plugins only, in install order and file order.
        for s in &p.package.skills {
            let fits = skill_bytes + s.text.len() <= MAX_OFFERED_SKILLS;
            skills.push(json!({"plugin":p.name,"skill":s.name,"offered":fits}));
            if fits {
                skill_bytes += s.text.len();
                offer.skills.push(PinnedSkill {
                    plugin: p.name.clone(),
                    skill: s.clone(),
                });
            }
        }
    }
    model_names(&mut offer.tools);
    for t in &offer.tools {
        offered.push(
            json!({"plugin":t.plugin_name,"server":t.server,"tool":t.name,"name":t.model_name}),
        );
    }
    let report = json!({"offered":offered,"excluded":excluded,"skills":skills,"citable_results":if offer.tools.is_empty() {0} else {citable}});
    Ok(Some((offer, report)))
}
/// Short names unique in the request: the tool name when unique and valid, otherwise `t<N>`.
fn model_names(tools: &mut [PinnedTool]) {
    let valid = |n: &str| {
        (1..=64).contains(&n.len())
            && n.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    };
    let names = tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
    let mut used = std::collections::BTreeSet::new();
    for t in tools.iter_mut() {
        if valid(&t.name) && names.iter().filter(|n| **n == t.name).count() == 1 {
            t.model_name = t.name.clone();
            used.insert(t.name.clone());
        } else {
            t.model_name.clear();
        }
    }
    let mut n = 1;
    for t in tools.iter_mut().filter(|t| t.model_name.is_empty()) {
        while used.contains(&format!("t{n}")) {
            n += 1;
        }
        t.model_name = format!("t{n}");
        used.insert(t.model_name.clone());
    }
}
