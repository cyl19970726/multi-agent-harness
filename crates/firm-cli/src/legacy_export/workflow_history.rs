use super::*;

/// Retired Dynamic Workflow journals. These are copied and verified as opaque
/// bytes: historical or partially-written rows must not be normalized through
/// the current Rust types merely to preserve them.
pub(super) const WORKFLOW_LEDGERS: &[&str] = &[
    "workflow_runs.jsonl",
    "workflow_steps.jsonl",
    "workflow_patches.jsonl",
    "workflow_artifact_manifests.jsonl",
];
pub(super) const WORKFLOW_PATCH_ROOT: &str = "workflow-patches";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WorkflowArchiveContract {
    pub(super) encoding: String,
    pub(super) ledgers: Vec<String>,
    pub(super) patch_root: String,
    pub(super) restore_mode: String,
}

pub(super) fn validate_workflow_archive_contract(
    contract: Option<&WorkflowArchiveContract>,
) -> Result<(), String> {
    let contract = contract.ok_or_else(|| "v2 archive is missing workflow_archive".to_string())?;
    let expected_ledgers = WORKFLOW_LEDGERS
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    if contract.encoding != "opaque-bytes"
        || contract.ledgers != expected_ledgers
        || contract.patch_root != WORKFLOW_PATCH_ROOT
        || contract.restore_mode != "read-only"
    {
        return Err("workflow archive contract does not match the v2 exporter contract".into());
    }
    Ok(())
}

pub(super) fn archive_workflow_history(
    source: &SourceSpec,
    archive_root: &Path,
    file_meta: &mut FileMetaMap,
    prefix: &str,
) -> Result<(), String> {
    for ledger in WORKFLOW_LEDGERS {
        let source_path = source.root.join(ledger);
        let (bytes, present) = if source_path.is_file() {
            (
                fs::read(&source_path)
                    .map_err(|error| format!("read {}: {error}", source_path.display()))?,
                true,
            )
        } else {
            (Vec::new(), false)
        };
        let archive_path = format!("{prefix}/workflow/raw/{ledger}");
        write_archive_file(archive_root, &archive_path, &bytes)?;
        file_meta.insert(
            archive_path,
            (
                "raw_workflow_ledger".into(),
                Some(source_path.display().to_string()),
                Some(present),
                None,
            ),
        );
    }

    let patch_prefix = format!("{WORKFLOW_PATCH_ROOT}/");
    for snapshot in source
        .before
        .iter()
        .filter(|entry| entry.path.starts_with(&patch_prefix))
    {
        let relative = Path::new(&snapshot.path)
            .strip_prefix(WORKFLOW_PATCH_ROOT)
            .map_err(|error| format!("invalid workflow patch snapshot path: {error}"))?;
        let relative_text = relative
            .to_str()
            .ok_or_else(|| format!("non-UTF-8 workflow patch path: {}", relative.display()))?;
        validate_relative_archive_path(relative_text)?;
        reject_relative_symlink_components(
            &source.root,
            Path::new(&snapshot.path),
            "workflow patch",
        )?;
        let source_path = source.root.join(&snapshot.path);
        let bytes = fs::read(&source_path)
            .map_err(|error| format!("read {}: {error}", source_path.display()))?;
        let archive_path = format!("{prefix}/workflow/patches/{relative_text}");
        write_archive_file(archive_root, &archive_path, &bytes)?;
        file_meta.insert(
            archive_path,
            (
                "raw_workflow_patch".into(),
                Some(source_path.display().to_string()),
                Some(true),
                None,
            ),
        );
    }
    Ok(())
}

pub(super) fn verify_workflow_history(
    archive: &Path,
    source: &ManifestSource,
    snapshot_paths: &BTreeMap<&str, &SnapshotFile>,
    entries: &BTreeMap<String, &ManifestFile>,
) -> Result<(), String> {
    for ledger in WORKFLOW_LEDGERS {
        let archive_path = format!("sources/{}/workflow/raw/{ledger}", source.id);
        let entry = entries
            .get(&archive_path)
            .ok_or_else(|| format!("manifest/archive missing workflow ledger: {archive_path}"))?;
        if entry.category != "raw_workflow_ledger" {
            return Err(format!(
                "wrong category for workflow ledger: {archive_path}"
            ));
        }
        let bytes = fs::read(archive.join(&archive_path))
            .map_err(|error| format!("read {archive_path}: {error}"))?;
        match (entry.source_present, snapshot_paths.get(*ledger).copied()) {
            (Some(true), Some(snapshot))
                if snapshot.bytes == bytes.len() as u64
                    && snapshot.sha256 == sha256_hex(&bytes) => {}
            (Some(false), None) if bytes.is_empty() => {}
            _ => {
                return Err(format!(
                    "raw workflow ledger does not match source snapshot: {}/{}",
                    source.id, ledger
                ));
            }
        }
    }

    let patch_prefix = format!("{WORKFLOW_PATCH_ROOT}/");
    let snapshot_patches = source
        .snapshot_files
        .iter()
        .filter(|entry| entry.path.starts_with(&patch_prefix))
        .map(|entry| {
            let relative = entry.path.trim_start_matches(&patch_prefix);
            (relative.to_string(), entry)
        })
        .collect::<BTreeMap<_, _>>();
    let archive_prefix = format!("sources/{}/workflow/patches/", source.id);
    let archived_patches = entries
        .iter()
        .filter(|(path, entry)| {
            path.starts_with(&archive_prefix) && entry.category == "raw_workflow_patch"
        })
        .map(|(path, entry)| (path.trim_start_matches(&archive_prefix).to_string(), *entry))
        .collect::<BTreeMap<_, _>>();
    if snapshot_patches.keys().collect::<Vec<_>>() != archived_patches.keys().collect::<Vec<_>>() {
        return Err(format!(
            "workflow patch inventory does not match source snapshot: {}",
            source.id
        ));
    }
    for (relative, snapshot) in snapshot_patches {
        let archive_path = format!("{archive_prefix}{relative}");
        let bytes = fs::read(archive.join(&archive_path))
            .map_err(|error| format!("read {archive_path}: {error}"))?;
        if snapshot.bytes != bytes.len() as u64 || snapshot.sha256 != sha256_hex(&bytes) {
            return Err(format!(
                "workflow patch does not match source snapshot: {archive_path}"
            ));
        }
    }
    Ok(())
}

/// Verify an archive and return its retired Workflow files as opaque bytes.
/// This is deliberately a read-only restore seam: callers can inspect or copy
/// the exact historical bytes without reopening any current writer.
pub fn restore_read_workflow_history(archive: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    verify_archive(archive)?;
    let manifest_path = archive.join("manifest.json");
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|error| format!("read {}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", manifest_path.display()))?;
    validate_workflow_archive_contract(manifest.workflow_archive.as_ref())?;
    let mut restored = BTreeMap::new();
    for entry in manifest.files.iter().filter(|entry| {
        matches!(
            entry.category.as_str(),
            "raw_workflow_ledger" | "raw_workflow_patch"
        )
    }) {
        restored.insert(
            entry.path.clone(),
            fs::read(archive.join(&entry.path))
                .map_err(|error| format!("read {}: {error}", entry.path))?,
        );
    }
    Ok(restored)
}

/// Return only syntactically parseable rows while retaining their exact source
/// line and byte slice. The authoritative archive is the separate opaque copy.
pub(super) fn parseable_jsonl_records(bytes: &[u8]) -> Vec<JsonlRecord<'_>> {
    let mut records = Vec::new();
    let mut start = 0_usize;
    for (index, end) in bytes
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'\n').then_some(index + 1))
        .chain((!bytes.is_empty() && bytes.last() != Some(&b'\n')).then_some(bytes.len()))
        .enumerate()
    {
        let line = index as u64 + 1;
        let raw = &bytes[start..end];
        let content = raw.strip_suffix(b"\n").unwrap_or(raw);
        if !content.iter().all(u8::is_ascii_whitespace) {
            if let Ok(value) = serde_json::from_slice(content) {
                records.push(JsonlRecord { line, raw, value });
            }
        }
        start = end;
    }
    records
}
