//! Read boundaries for provider-owned native Session storage.
//!
//! The path remains server-side. It is resolved only from an exact canonical
//! AgentSession binding, then consumed by the disposable provider projection
//! service. Harness never copies the transcript or keeps a replay cursor.

use std::fs;
use std::io::{BufRead, BufReader, Seek};
use std::path::{Path, PathBuf};

use harness_core::NativeSessionRef;
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::Deserialize;

use crate::{CliError, CliResult};

mod deepseek;

const MAX_DISCOVERY_LINE_BYTES: usize = 1024 * 1024;

/// Find the canonical Codex rollout whose own `session_meta.payload.id`
/// exactly names `session_id`.
///
/// The caller supplies the Codex home explicitly. A matching filename is only
/// a candidate and is never accepted as evidence by itself. This proves that
/// the same-user provider store contains the session metadata; it does not
/// prove a live attachment, exclusive ownership, or authentication.
pub(crate) fn discover_codex_rollout(
    codex_home: &Path,
    session_id: &str,
) -> CliResult<Option<PathBuf>> {
    if session_id.trim().is_empty() {
        return Err(CliError::Usage(
            "Codex native session id must not be empty".into(),
        ));
    }
    let canonical_home = match fs::canonicalize(codex_home) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let sessions = canonical_home.join("sessions");
    let canonical_sessions = match fs::canonicalize(&sessions) {
        Ok(path) if path.starts_with(&canonical_home) => path,
        Ok(_) => {
            return Err(CliError::Usage(
                "Codex sessions root escapes the canonical Codex home".into(),
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let suffix = format!("{session_id}.jsonl");
    find_codex_rollout_with_metadata(&canonical_sessions, &suffix, session_id, 5)
}

fn find_codex_rollout_with_metadata(
    root: &Path,
    filename_suffix: &str,
    session_id: &str,
    depth: usize,
) -> CliResult<Option<PathBuf>> {
    if depth == 0 || !root.is_dir() {
        return Ok(None);
    }
    let mut found = None;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(filename_suffix))
        {
            validate_codex_rollout_metadata(&path, session_id)?;
            let canonical = fs::canonicalize(&path)?;
            if !canonical.starts_with(root) {
                return Err(CliError::Usage(format!(
                    "Codex rollout candidate escapes the canonical sessions root: {}",
                    path.display()
                )));
            }
            if found.replace(canonical).is_some() {
                return Err(CliError::Usage(format!(
                    "multiple Codex rollout candidates name exact session `{session_id}`"
                )));
            }
        }
        if metadata.is_dir() {
            if let Some(nested) =
                find_codex_rollout_with_metadata(&path, filename_suffix, session_id, depth - 1)?
            {
                if found.replace(nested).is_some() {
                    return Err(CliError::Usage(format!(
                        "multiple Codex rollout candidates name exact session `{session_id}`"
                    )));
                }
            }
        }
    }
    Ok(found)
}

fn validate_codex_rollout_metadata(path: &Path, session_id: &str) -> CliResult<()> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut session_meta_id = None;
    loop {
        if reader.fill_buf()?.is_empty() {
            break;
        }
        let line_start = reader.stream_position()?;
        let row = match CodexDiscoveryRow::deserialize(&mut serde_json::Deserializer::from_reader(
            &mut reader,
        )) {
            Ok(row) => row,
            Err(error) if error.is_eof() => break,
            Err(error) => return Err(error.into()),
        };
        let mut terminator = Vec::new();
        let terminator_bytes = reader.read_until(b'\n', &mut terminator)?;
        if terminator_bytes == 0 || !terminator.ends_with(b"\n") {
            // The provider may be appending the final row. It is not valid
            // metadata yet and the projection reader will expose the omitted
            // tail as truncated; complete malformed rows remain errors.
            break;
        }
        if !terminator[..terminator.len() - 1]
            .iter()
            .all(u8::is_ascii_whitespace)
        {
            return Err(CliError::Usage(format!(
                "Codex rollout row has trailing non-whitespace content: {}",
                path.display()
            )));
        }
        let line_bytes = reader.stream_position()?.saturating_sub(line_start) as usize;
        match row {
            CodexDiscoveryRow::SessionMeta(_) if line_bytes > MAX_DISCOVERY_LINE_BYTES => {
                return Err(CliError::Usage(
                    "provider-native Session metadata line exceeds 1 MiB".into(),
                ));
            }
            CodexDiscoveryRow::SessionMeta(payload_id) => {
                if session_meta_id.replace(payload_id).is_some() {
                    return Err(CliError::Usage(format!(
                        "Codex rollout candidate has multiple session_meta rows: {}",
                        path.display()
                    )));
                }
            }
            CodexDiscoveryRow::Other if line_bytes > MAX_DISCOVERY_LINE_BYTES => {
                if session_meta_id.is_none() {
                    return Err(CliError::Usage(
                        "provider-native Session discovery line exceeds 1 MiB before session metadata"
                            .into(),
                    ));
                }
            }
            CodexDiscoveryRow::Other => {}
        }
    }
    let metadata_id = session_meta_id.ok_or_else(|| {
        CliError::Usage(format!(
            "Codex rollout candidate has no session_meta row: {}",
            path.display()
        ))
    })?;
    if metadata_id != session_id {
        return Err(CliError::Usage(format!(
            "Codex rollout candidate metadata id `{metadata_id}` does not match requested session `{session_id}`"
        )));
    }
    Ok(())
}

enum CodexDiscoveryRow {
    SessionMeta(String),
    Other,
}

impl<'de> Deserialize<'de> for CodexDiscoveryRow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct RowVisitor;

        impl<'de> Visitor<'de> for RowVisitor {
            type Value = CodexDiscoveryRow;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a Codex provider-native JSON object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut row_type = None;
                let mut session_meta_id = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => {
                            if row_type.is_some() {
                                return Err(de::Error::duplicate_field("type"));
                            }
                            row_type = Some(map.next_value::<String>()?);
                        }
                        "payload" if row_type.as_deref() == Some("session_meta") => {
                            #[derive(Deserialize)]
                            struct SessionMetaPayload {
                                id: String,
                            }
                            if session_meta_id.is_some() {
                                return Err(de::Error::duplicate_field("payload"));
                            }
                            session_meta_id = Some(map.next_value::<SessionMetaPayload>()?.id);
                        }
                        "payload" if row_type.is_none() => {
                            return Err(de::Error::custom(
                                "Codex rollout payload precedes its row type",
                            ));
                        }
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                match row_type.as_deref() {
                    Some("session_meta") => session_meta_id
                        .map(CodexDiscoveryRow::SessionMeta)
                        .ok_or_else(|| de::Error::missing_field("payload.id")),
                    Some(_) => Ok(CodexDiscoveryRow::Other),
                    None => Err(de::Error::missing_field("type")),
                }
            }
        }

        deserializer.deserialize_map(RowVisitor)
    }
}

fn locate(session: &NativeSessionRef) -> CliResult<Option<PathBuf>> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        CliError::Usage("HOME is unavailable for native session discovery".into())
    })?;
    match session.provider.as_str() {
        "codex" => discover_codex_rollout(
            &std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex")),
            &session.native_session_id,
        ),
        "kimi" => find_kimi_wire(
            &kimi_code_home(&home).join("sessions"),
            &session.native_session_id,
            4,
        ),
        "claude" | "claude-code" | "claude_code" => find_exact_file(
            &home.join(".claude/projects"),
            &format!("{}.jsonl", session.native_session_id),
            4,
        ),
        "pi" => locate_pi_jsonl(&session.native_session_id),
        _ => Ok(None),
    }
}

/// Resolve a transcript together with the canonical containment root. The
/// provider decoder revalidates this boundary before reading.
pub(crate) fn locate_read_boundary(
    session: &NativeSessionRef,
    execution_space_id: &str,
) -> CliResult<Option<(PathBuf, PathBuf)>> {
    let Some(path) = locate(session)? else {
        return Ok(None);
    };
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        CliError::Usage("HOME is unavailable for native session discovery".into())
    })?;
    let root = match session.provider.as_str() {
        "codex" => std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"))
            .join("sessions"),
        "kimi" => kimi_code_home(&home).join("sessions"),
        "claude" | "claude-code" | "claude_code" => home.join(".claude/projects"),
        "pi" => pi_sessions_root(&path, Some(execution_space_id))?,
        _ => return Ok(None),
    };
    Ok(Some((root, path)))
}

pub(crate) fn read_deepseek_session_jsonl(session: &NativeSessionRef) -> CliResult<String> {
    if session.provider != "deepseek_harness" && session.provider != "deepseek" {
        return Err(CliError::Usage(
            "DeepSeek native reader requires a DeepSeek Harness Session".into(),
        ));
    }
    deepseek::read_official_jsonl(&session.native_session_id)
}

fn locate_pi_jsonl(locator: &str) -> CliResult<Option<PathBuf>> {
    let path = PathBuf::from(locator);
    if !path.is_absolute() {
        return Err(CliError::Usage(
            "Pi native Session locator must be absolute".into(),
        ));
    }
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CliError::Usage(
            "Pi native Session must be a regular non-symlink JSONL file".into(),
        ));
    }
    let canonical = fs::canonicalize(path)?;
    let root = pi_sessions_root(&canonical, None)?;
    if !canonical.starts_with(&root)
        || canonical.extension().and_then(|v| v.to_str()) != Some("jsonl")
    {
        return Err(CliError::Usage(
            "Pi native Session is outside an exact pi_sessions root".into(),
        ));
    }
    Ok(Some(canonical))
}

fn pi_sessions_root(path: &Path, execution_space_id: Option<&str>) -> CliResult<PathBuf> {
    let mut candidate = PathBuf::new();
    let mut previous = None::<String>;
    let mut observed_space = None::<String>;
    for component in path.components() {
        let value = component.as_os_str().to_string_lossy().into_owned();
        candidate.push(&value);
        if previous.as_deref() == Some("execution-spaces") {
            observed_space = Some(value.clone());
        }
        if component.as_os_str() == "pi_sessions" {
            if let Some(expected) = execution_space_id {
                if observed_space.as_deref() != Some(expected) {
                    return Err(CliError::Usage(
                        "Pi native Session belongs to another Execution Space".into(),
                    ));
                }
            }
            return fs::canonicalize(candidate).map_err(Into::into);
        }
        previous = Some(value);
    }
    Err(CliError::Usage(
        "Pi native Session locator does not contain the managed pi_sessions root".into(),
    ))
}

fn kimi_code_home(home: &Path) -> PathBuf {
    std::env::var_os("KIMI_CODE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kimi-code"))
}

fn find_exact_file(root: &Path, filename: &str, depth: usize) -> CliResult<Option<PathBuf>> {
    if depth == 0 {
        return Ok(None);
    }
    let canonical_root = match fs::canonicalize(root) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    find_exact_file_beneath(&canonical_root, &canonical_root, filename, depth)
}

fn find_exact_file_beneath(
    root: &Path,
    canonical_root: &Path,
    filename: &str,
    depth: usize,
) -> CliResult<Option<PathBuf>> {
    if depth == 0 || !root.is_dir() {
        return Ok(None);
    }
    let mut found = None;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() && path.file_name().and_then(|name| name.to_str()) == Some(filename) {
            let candidate = fs::canonicalize(&path)?;
            if !candidate.starts_with(canonical_root) {
                return Err(CliError::Usage(format!(
                    "provider-native Session candidate escapes its canonical root: {}",
                    path.display()
                )));
            }
            if found.replace(candidate).is_some() {
                return Err(CliError::Usage(format!(
                    "multiple provider-native Session candidates exactly name `{filename}`"
                )));
            }
        }
        if metadata.is_dir() {
            if let Some(nested) =
                find_exact_file_beneath(&path, canonical_root, filename, depth - 1)?
            {
                if found.replace(nested).is_some() {
                    return Err(CliError::Usage(format!(
                        "multiple provider-native Session candidates exactly name `{filename}`"
                    )));
                }
            }
        }
    }
    Ok(found)
}

fn find_kimi_wire(root: &Path, session_dir: &str, depth: usize) -> CliResult<Option<PathBuf>> {
    if depth == 0 {
        return Ok(None);
    }
    let canonical_root = match fs::canonicalize(root) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !fs::symlink_metadata(&canonical_root)?.is_dir() {
        return Ok(None);
    }
    // Current Kimi Code stores `<workDirKey>/<sessionId>/agents/main/wire.jsonl`.
    // Older Python releases used `session_<sessionId>` directories. Accept both
    // exact provider-owned layouts during migration, but reject duplicates.
    let mut expected = vec![session_dir.to_string()];
    if !session_dir.starts_with("session_") {
        expected.push(format!("session_{session_dir}"));
    }
    find_kimi_wire_beneath(
        &canonical_root,
        &canonical_root,
        &expected,
        session_dir,
        depth,
    )
}

fn find_kimi_wire_beneath(
    root: &Path,
    canonical_root: &Path,
    expected_session_dirs: &[String],
    requested_session: &str,
    depth: usize,
) -> CliResult<Option<PathBuf>> {
    if depth == 0 {
        return Ok(None);
    }
    let mut found = None;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    expected_session_dirs
                        .iter()
                        .any(|expected| expected == name)
                })
            {
                let wire = path.join("agents/main/wire.jsonl");
                match fs::symlink_metadata(&wire) {
                    Ok(wire_metadata) => {
                        if wire_metadata.file_type().is_symlink() {
                            return Err(CliError::Usage(format!(
                                "Kimi wire candidate is a symbolic link: {}",
                                wire.display()
                            )));
                        }
                        if !wire_metadata.is_file() {
                            return Err(CliError::Usage(format!(
                                "Kimi wire candidate is not a regular file: {}",
                                wire.display()
                            )));
                        }
                        let candidate = fs::canonicalize(&wire)?;
                        if !candidate.starts_with(canonical_root) {
                            return Err(CliError::Usage(format!(
                                "Kimi wire candidate escapes the canonical sessions root: {}",
                                wire.display()
                            )));
                        }
                        if found.replace(candidate).is_some() {
                            return Err(CliError::Usage(format!(
                                "multiple Kimi Session candidates name `{requested_session}`"
                            )));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if let Some(nested) = find_kimi_wire_beneath(
                &path,
                canonical_root,
                expected_session_dirs,
                requested_session,
                depth - 1,
            )? {
                if found.replace(nested).is_some() {
                    return Err(CliError::Usage(format!(
                        "multiple Kimi Session candidates name `{requested_session}`"
                    )));
                }
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

    fn codex_home(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "firm-codex-rollout-{label}-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("sessions/2026/08/09")).expect("sessions root");
        root
    }

    fn kimi_sessions(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "firm-kimi-sessions-{label}-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("Kimi sessions root");
        root
    }

    #[test]
    fn pi_session_locator_is_exact_jsonl_beneath_managed_root() {
        let root = std::env::temp_dir().join(format!(
            "firm-pi-native-session-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        let session = root.join("execution-spaces/space-a/pi_sessions/run-a/session.jsonl");
        fs::create_dir_all(session.parent().expect("parent")).expect("Pi session dir");
        fs::write(&session, "{\"type\":\"turn_end\"}\n").expect("Pi JSONL");
        assert_eq!(
            locate_pi_jsonl(session.to_str().expect("UTF-8 path")).expect("Pi boundary"),
            Some(fs::canonicalize(&session).expect("canonical Pi Session"))
        );
        assert!(pi_sessions_root(&session, Some("space-a")).is_ok());
        assert!(pi_sessions_root(&session, Some("space-b")).is_err());
        let outside = root.join("outside.jsonl");
        fs::write(&outside, "{}\n").expect("outside");
        assert!(locate_pi_jsonl(outside.to_str().expect("UTF-8 path")).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn discovers_codex_rollout_only_from_exact_session_metadata() {
        let home = codex_home("valid");
        let session_id = "019f-rollout-valid";
        let rollout = home
            .join("sessions/2026/08/09")
            .join(format!("rollout-2026-08-09T00-00-00-{session_id}.jsonl"));
        fs::write(
            &rollout,
            format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\"}}}}\n"),
        )
        .expect("rollout");
        assert_eq!(
            discover_codex_rollout(&home, session_id).expect("discovery"),
            Some(fs::canonicalize(rollout).expect("canonical rollout"))
        );
        fs::remove_dir_all(home).expect("cleanup");
    }

    #[test]
    fn matching_codex_rollout_filename_with_mismatched_metadata_is_rejected() {
        let home = codex_home("mismatch");
        let requested = "019f-requested";
        let rollout = home
            .join("sessions/2026/08/09")
            .join(format!("rollout-2026-08-09T00-00-00-{requested}.jsonl"));
        fs::write(
            rollout,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"019f-different\"}}\n",
        )
        .expect("rollout");
        let error = discover_codex_rollout(&home, requested).expect_err("mismatch rejects");
        assert!(error
            .to_string()
            .contains("does not match requested session"));
        fs::remove_dir_all(home).expect("cleanup");
    }

    #[test]
    fn malformed_line_before_exact_codex_metadata_rejects_candidate() {
        let home = codex_home("malformed-before-exact");
        let session_id = "019f-malformed-before-exact";
        let rollout = home
            .join("sessions/2026/08/09")
            .join(format!("rollout-2026-08-09T00-00-00-{session_id}.jsonl"));
        fs::write(
            rollout,
            format!(
                "not-json\n{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\"}}}}\n"
            ),
        )
        .expect("rollout");
        assert!(discover_codex_rollout(&home, session_id).is_err());
        fs::remove_dir_all(home).expect("cleanup");
    }

    #[test]
    fn incomplete_codex_tail_does_not_hide_exact_complete_metadata() {
        let home = codex_home("incomplete-tail");
        let session_id = "019f-incomplete-tail";
        let rollout = home
            .join("sessions/2026/08/09")
            .join(format!("rollout-2026-08-09T00-00-00-{session_id}.jsonl"));
        fs::write(
            &rollout,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\"}}}}\n{{\"type\":\"event_msg\""
            ),
        )
        .expect("active rollout");
        assert_eq!(
            discover_codex_rollout(&home, session_id).expect("active discovery"),
            Some(fs::canonicalize(rollout).expect("canonical active rollout"))
        );
        fs::remove_dir_all(home).expect("cleanup");
    }

    #[test]
    fn oversized_codex_event_after_exact_metadata_does_not_break_discovery() {
        let home = codex_home("oversized-event-after-metadata");
        let session_id = "019f-oversized-event-after-metadata";
        let rollout = home
            .join("sessions/2026/08/09")
            .join(format!("rollout-2026-08-09T00-00-00-{session_id}.jsonl"));
        let mut contents = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\"}}}}\n{{\"type\":\"event_msg\",\"payload\":\""
        )
        .into_bytes();
        contents.extend(std::iter::repeat_n(b'x', MAX_DISCOVERY_LINE_BYTES));
        contents.extend_from_slice(b"\"}\n");
        fs::write(&rollout, contents).expect("rollout");

        assert_eq!(
            discover_codex_rollout(&home, session_id).expect("bounded discovery"),
            Some(fs::canonicalize(&rollout).expect("canonical rollout"))
        );
        fs::remove_dir_all(home).expect("cleanup");
    }

    #[test]
    fn oversized_codex_event_before_metadata_is_rejected() {
        let home = codex_home("oversized-event-before-metadata");
        let session_id = "019f-oversized-event-before-metadata";
        let rollout = home
            .join("sessions/2026/08/09")
            .join(format!("rollout-2026-08-09T00-00-00-{session_id}.jsonl"));
        let mut contents = b"{\"type\":\"event_msg\",\"payload\":\"".to_vec();
        contents.extend(std::iter::repeat_n(b'x', MAX_DISCOVERY_LINE_BYTES));
        contents.extend_from_slice(
            format!(
                "\"}}\n{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\"}}}}\n"
            )
            .as_bytes(),
        );
        fs::write(rollout, contents).expect("rollout");

        let error = discover_codex_rollout(&home, session_id).expect_err("must fail closed");
        assert!(error.to_string().contains("before session metadata"));
        fs::remove_dir_all(home).expect("cleanup");
    }

    #[test]
    fn oversized_or_unclassifiable_codex_metadata_after_exact_metadata_is_rejected() {
        for (label, second_row) in [
            (
                "oversized-metadata",
                format!(
                    "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"019f-oversized-later\",\"padding\":\"{}\"}}}}\n",
                    "x".repeat(MAX_DISCOVERY_LINE_BYTES)
                ),
            ),
            (
                "unclassifiable-oversized",
                format!(
                    "{{\"payload\":\"{}\",\"type\":\"event_msg\"}}\n",
                    "x".repeat(MAX_DISCOVERY_LINE_BYTES)
                ),
            ),
        ] {
            let home = codex_home(label);
            let session_id = format!("019f-{label}");
            let rollout = home
                .join("sessions/2026/08/09")
                .join(format!("rollout-2026-08-09T00-00-00-{session_id}.jsonl"));
            fs::write(
                rollout,
                format!(
                    "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\"}}}}\n{second_row}"
                ),
            )
            .expect("rollout");
            assert!(discover_codex_rollout(&home, &session_id).is_err());
            fs::remove_dir_all(home).expect("cleanup");
        }
    }

    #[test]
    fn missing_or_multiple_codex_metadata_rejects_candidate() {
        for (label, contents) in [
            ("missing", "{\"type\":\"event_msg\",\"payload\":{}}\n"),
            (
                "multiple",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"019f-ambiguous\"}}\n{\"type\":\"session_meta\",\"payload\":{\"id\":\"019f-ambiguous\"}}\n",
            ),
        ] {
            let home = codex_home(label);
            let session_id = "019f-ambiguous";
            let rollout = home
                .join("sessions/2026/08/09")
                .join(format!("rollout-2026-08-09T00-00-00-{session_id}.jsonl"));
            fs::write(rollout, contents).expect("rollout");
            assert!(discover_codex_rollout(&home, session_id).is_err());
            fs::remove_dir_all(home).expect("cleanup");
        }
    }

    #[test]
    fn discovers_only_the_exact_kimi_session_wire() {
        let root = kimi_sessions("valid");
        let wire = root.join("wd_project/019f-kimi-valid/agents/main/wire.jsonl");
        fs::create_dir_all(wire.parent().expect("wire parent")).expect("wire parent");
        fs::write(&wire, "{\"type\":\"turn.prompt\"}\n").expect("wire");
        assert_eq!(
            find_kimi_wire(&root, "019f-kimi-valid", 4).expect("Kimi discovery"),
            Some(fs::canonicalize(&wire).expect("canonical wire"))
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn claude_session_discovery_requires_an_exact_filename() {
        let root = kimi_sessions("claude-exact-filename");
        let requested = "claude-native-session";
        fs::write(
            root.join(format!("prefix-{requested}.jsonl")),
            "{\"type\":\"assistant\"}\n",
        )
        .expect("prefix candidate");
        assert_eq!(
            find_exact_file(&root, &format!("{requested}.jsonl"), 4)
                .expect("exact Claude discovery"),
            None,
            "a suffix match is not an exact provider Session"
        );
        let exact = root.join(format!("{requested}.jsonl"));
        fs::write(&exact, "{\"type\":\"assistant\"}\n").expect("exact candidate");
        assert_eq!(
            find_exact_file(&root, &format!("{requested}.jsonl"), 4)
                .expect("exact Claude discovery"),
            Some(fs::canonicalize(exact).expect("canonical exact candidate"))
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn duplicate_kimi_session_directories_are_rejected() {
        let root = kimi_sessions("duplicate");
        for session_dir in ["019f-kimi-duplicate", "session_019f-kimi-duplicate"] {
            let wire = root
                .join("wd_project")
                .join(session_dir)
                .join("agents/main/wire.jsonl");
            fs::create_dir_all(wire.parent().expect("wire parent")).expect("wire parent");
            fs::write(wire, "{}\n").expect("wire");
        }
        let error = find_kimi_wire(&root, "019f-kimi-duplicate", 4)
            .expect_err("duplicate candidates reject");
        assert!(error
            .to_string()
            .contains("multiple Kimi Session candidates"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn kimi_wire_symbolic_link_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = kimi_sessions("wire-symlink");
        let outside_root = kimi_sessions("wire-symlink-outside");
        let outside = outside_root.join("wire.jsonl");
        fs::write(&outside, "{}\n").expect("outside wire");
        let wire = root.join("session_019f-kimi-link/agents/main/wire.jsonl");
        fs::create_dir_all(wire.parent().expect("wire parent")).expect("wire parent");
        symlink(&outside, &wire).expect("wire symlink");
        let error = find_kimi_wire(&root, "019f-kimi-link", 4).expect_err("symlink rejects");
        assert!(error.to_string().contains("symbolic link"));
        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(outside_root).expect("cleanup outside");
    }
}
