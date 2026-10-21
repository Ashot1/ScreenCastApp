use crate::*;
use gix_ref::{FullName, Target};
use std::collections::{BTreeMap, BTreeSet};
fn reason(reasons: &mut Vec<LocalReadReason>, value: LocalReadReason) {
    if !reasons.contains(&value) {
        reasons.push(value);
    }
}
fn config(bytes: &[u8]) -> Result<gix_hash::Kind, LocalReadReason> {
    let c = gix_config::File::from_bytes_no_includes(
        bytes,
        gix_config::file::Metadata::api(),
        Default::default(),
    )
    .map_err(|_| LocalReadReason::Malformed)?;
    for s in c.sections() {
        let name = s.header().name();
        if name.eq_ignore_ascii_case(b"include") || name.eq_ignore_ascii_case(b"includeif") {
            return Err(LocalReadReason::Unsupported);
        }
        if name.eq_ignore_ascii_case(b"extensions")
            && (s.header().subsection_name().is_some()
                || s.value_names().any(|n| {
                    !["objectformat", "refstorage"].contains(&n.to_ascii_lowercase().as_str())
                }))
        {
            return Err(LocalReadReason::Unsupported);
        }
    }
    let format = c
        .integer("core.repositoryformatversion")
        .map_err(|_| LocalReadReason::Malformed)?
        .ok_or(LocalReadReason::Malformed)?;
    if !(0..=1).contains(&format)
        || c.boolean("core.bare")
            .map_err(|_| LocalReadReason::Malformed)?
            .unwrap_or(false)
    {
        return Err(LocalReadReason::Unsupported);
    }
    let object = c.string("extensions.objectformat");
    let storage = c.string("extensions.refstorage");
    if format == 0 && (object.is_some() || storage.is_some()) {
        return Err(LocalReadReason::Unsupported);
    }
    if storage.as_ref().is_some_and(|v| v.as_slice() != b"files") {
        return Err(LocalReadReason::Unsupported);
    }
    match object.as_ref().map(|v| v.as_slice()) {
        None | Some(b"sha1") => Ok(gix_hash::Kind::Sha1),
        Some(b"sha256") if format == 1 => Ok(gix_hash::Kind::Sha256),
        _ => Err(LocalReadReason::Unsupported),
    }
}
pub(crate) fn local_ref(name: &str) -> Result<FullName, StoreError> {
    if !name.starts_with("refs/heads/") || name.len() > 2043 {
        return Err(StoreError::InvalidInput);
    }
    FullName::try_from(name).map_err(|_| StoreError::InvalidInput)
}
fn target_name(target: &FullName) -> Option<&str> {
    std::str::from_utf8(target.as_bstr().as_ref()).ok()
}
fn resolve(name: &str, refs: &BTreeMap<String, Target>) -> Result<Option<String>, ()> {
    let mut seen = BTreeSet::new();
    let mut name = name;
    for _ in 0..16 {
        if !seen.insert(name) {
            return Err(());
        }
        match refs.get(name) {
            Some(Target::Object(id)) => return Ok(Some(id.to_string())),
            Some(Target::Symbolic(t)) => {
                name = target_name(t)
                    .filter(|n| n.starts_with("refs/heads/"))
                    .ok_or(())?;
            }
            None => return Ok(None),
        }
    }
    Err(())
}
pub fn parse_local_git(
    root: &LocalRootSettings,
    files: &[LocalGitFile],
    start: i64,
    end: i64,
    reasons: &[LocalReadReason],
) -> Result<LocalObservation, StoreError> {
    parse_with_entries(root, files, start, end, reasons, 0)
}
pub(crate) fn parse_with_entries(
    root: &LocalRootSettings,
    files: &[LocalGitFile],
    start: i64,
    end: i64,
    reasons: &[LocalReadReason],
    entries: usize,
) -> Result<LocalObservation, StoreError> {
    validate_root(&root.path, &root.exclusions)?;
    if end < start || files.len() > MAX_SOURCE_FILES || reasons.len() > 8 {
        return Err(StoreError::InvalidInput);
    }
    let mut total = 0usize;
    let mut seen = BTreeSet::new();
    let mut rs = Vec::new();
    for r in reasons {
        reason(&mut rs, *r);
    }
    for f in files {
        relative_path(&f.path, 2048)?;
        let cap = match f.path.as_str() {
            ".git/config" => 65536,
            ".git/HEAD" => 4096,
            ".git/packed-refs" => 524288,
            p if p.starts_with(".git/refs/heads/") => {
                local_ref(&p[5..])?;
                4096
            }
            _ => return Err(StoreError::InvalidInput),
        };
        total = total
            .checked_add(f.bytes.len())
            .ok_or(StoreError::ResourceLimit)?;
        if f.bytes.len() > cap || total > MAX_SOURCE_BYTES {
            return Err(StoreError::ResourceLimit);
        }
        if !seen.insert(&f.path)
            || root.excluded(&f.path)
            || (f.path == ".git/packed-refs" && root.skip_packed())
        {
            return Err(StoreError::InvalidInput);
        }
    }
    let mut o = LocalObservation {
        started_at_utc: start,
        ended_at_utc: end,
        completeness: Completeness::Partial,
        reasons: rs,
        branches: Vec::new(),
    };
    // Unsupported/raced metadata cannot establish a normal or empty observation.
    if o.reasons
        .iter()
        .any(|r| matches!(r, LocalReadReason::Unsupported | LocalReadReason::Changed))
    {
        return Ok(o);
    }
    let Some(c) = files.iter().find(|f| f.path == ".git/config") else {
        reason(&mut o.reasons, LocalReadReason::Unavailable);
        return Ok(o);
    };
    let kind = match config(&c.bytes) {
        Ok(k) => k,
        Err(r) => {
            reason(&mut o.reasons, r);
            return Ok(o);
        }
    };
    let mut refs = BTreeMap::new();
    // An unread loose ref can override any packed tip; native losses make packed tips unproven.
    if reasons.is_empty()
        && let Some(p) = files.iter().find(|f| f.path == ".git/packed-refs")
    {
        let iter = match gix_ref::packed::Iter::new(&p.bytes, kind) {
            Ok(i) => i,
            Err(_) => {
                reason(&mut o.reasons, LocalReadReason::Malformed);
                return Ok(o);
            }
        };
        let mut previous: Option<Vec<u8>> = None;
        for (count, item) in iter.enumerate() {
            if count >= MAX_SOURCE_ENTRIES.saturating_sub(entries) {
                reason(&mut o.reasons, LocalReadReason::Limit);
                break;
            }
            let r = match item {
                Ok(r) => r,
                Err(_) => {
                    reason(&mut o.reasons, LocalReadReason::Malformed);
                    continue;
                }
            };
            let bytes: &[u8] = r.name.as_bstr().as_ref();
            if previous.as_ref().is_some_and(|p| p.as_slice() >= bytes) {
                reason(&mut o.reasons, LocalReadReason::Malformed);
                o.branches.clear();
                return Ok(o);
            }
            previous = Some(bytes.to_vec());
            let name = match std::str::from_utf8(bytes) {
                Ok(n) => n,
                Err(_) => {
                    reason(&mut o.reasons, LocalReadReason::InvalidName);
                    continue;
                }
            };
            if name.starts_with("refs/heads/") {
                if local_ref(name).is_err() {
                    reason(&mut o.reasons, LocalReadReason::InvalidName);
                    continue;
                }
                if refs.len() >= MAX_SOURCE_BRANCHES {
                    reason(&mut o.reasons, LocalReadReason::Limit);
                    continue;
                }
                refs.insert(name.to_owned(), Target::Object(r.target()));
            }
        }
    }
    for f in files
        .iter()
        .filter(|f| f.path.starts_with(".git/refs/heads/"))
    {
        let name = &f.path[5..];
        if !refs.contains_key(name) && refs.len() >= MAX_SOURCE_BRANCHES {
            reason(&mut o.reasons, LocalReadReason::Limit);
            continue;
        }
        match gix_ref::file::loose::Reference::try_from_path(local_ref(name)?, &f.bytes, kind) {
            Ok(r) => {
                refs.insert(name.into(), r.target);
            }
            Err(_) => {
                refs.remove(name);
                reason(&mut o.reasons, LocalReadReason::Malformed);
            }
        }
    }
    let head = files
        .iter()
        .find(|f| f.path == ".git/HEAD")
        .and_then(|f| {
            FullName::try_from("HEAD").ok().and_then(|n| {
                gix_ref::file::loose::Reference::try_from_path(n, &f.bytes, kind).ok()
            })
        })
        .map(|r| r.target);
    if head.is_none() {
        reason(&mut o.reasons, LocalReadReason::Unavailable);
    }
    let head_name = match &head {
        Some(Target::Symbolic(t)) => match target_name(t) {
            Some(n) if n.starts_with("refs/heads/") => {
                if local_ref(n).is_err() {
                    reason(&mut o.reasons, LocalReadReason::InvalidName);
                    None
                } else {
                    Some(n).filter(|n| !root.excluded(&format!(".git/{n}")))
                }
            }
            Some(_) => None,
            None => {
                reason(&mut o.reasons, LocalReadReason::InvalidName);
                None
            }
        },
        _ => None,
    };
    let head_known = head_name
        .and_then(|n| resolve(n, &refs).ok().flatten())
        .is_some();
    // Missing symbolic targets are unborn only in a complete traversal; otherwise the relation stays unknown.
    if let Some(n) = head_name
        && !refs.contains_key(n)
        && o.reasons.is_empty()
    {
        if refs.len() < MAX_SOURCE_BRANCHES {
            o.branches.push(LocalBranch {
                full_ref: n.into(),
                oid: None,
                head: BranchHead::Unborn,
            });
        } else {
            reason(&mut o.reasons, LocalReadReason::Limit);
        }
    }
    for name in refs.keys() {
        let oid = match resolve(name, &refs) {
            Ok(id) => id,
            Err(()) => {
                reason(&mut o.reasons, LocalReadReason::Symbolic);
                None
            }
        };
        if oid.is_none() {
            reason(&mut o.reasons, LocalReadReason::Symbolic);
        }
        let relation = match &head {
            Some(Target::Object(_)) => BranchHead::Detached,
            Some(Target::Symbolic(_)) if head_known => {
                if head_name == Some(name.as_str()) {
                    BranchHead::Current
                } else {
                    BranchHead::Other
                }
            }
            _ => BranchHead::Unknown,
        };
        o.branches.push(LocalBranch {
            full_ref: name.clone(),
            oid,
            head: relation,
        });
    }
    o.branches.sort_by(|a, b| a.full_ref.cmp(&b.full_ref));
    if o.reasons.is_empty() {
        o.completeness = Completeness::Complete;
    }
    Ok(o)
}
