use super::*;

/// Verify hashes, line counts, latest projections, edge regeneration, and
/// referential closure without consulting the live store.
pub fn verify_archive(archive: &Path) -> Result<VerifySummary, String> {
    reject_symlink_ancestors(archive, "archive directory")?;
    reject_symlink_or_non_directory(archive, "archive directory")?;
    let manifest_path = archive.join("manifest.json");
    let manifest_bytes =
        fs::read(&manifest_path).map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("parse {}: {e}", manifest_path.display()))?;
    if manifest.format != ARCHIVE_FORMAT || manifest.version != ARCHIVE_VERSION {
        return Err(format!(
            "unsupported archive format/version: {}/{}",
            manifest.format, manifest.version
        ));
    }
    if manifest.sources.is_empty() {
        return Err("archive manifest must contain at least one source".into());
    }

    let mut entries = BTreeMap::new();
    for entry in &manifest.files {
        validate_relative_archive_path(&entry.path)?;
        reject_relative_symlink_components(archive, Path::new(&entry.path), "archive file")?;
        if entries.insert(entry.path.clone(), entry).is_some() {
            return Err(format!("duplicate manifest path: {}", entry.path));
        }
        let path = archive.join(&entry.path);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("inspect archived file {}: {e}", path.display()))?;
        if !metadata.file_type().is_file() {
            return Err(format!(
                "manifest path is not a regular file: {}",
                entry.path
            ));
        }
        let bytes =
            fs::read(&path).map_err(|e| format!("read archived file {}: {e}", path.display()))?;
        let actual_hash = sha256_hex(&bytes);
        if actual_hash != entry.sha256 {
            return Err(format!(
                "SHA-256 mismatch for {}: manifest {}, actual {}",
                entry.path, entry.sha256, actual_hash
            ));
        }
        if bytes.len() as u64 != entry.bytes {
            return Err(format!(
                "byte-count mismatch for {}: manifest {}, actual {}",
                entry.path,
                entry.bytes,
                bytes.len()
            ));
        }
        let lines = physical_line_count(&bytes);
        if lines != entry.line_count {
            return Err(format!(
                "line-count mismatch for {}: manifest {}, actual {}",
                entry.path, entry.line_count, lines
            ));
        }
        if let Some(source_lines) = &entry.source_lines {
            if source_lines.len() as u64 != lines {
                return Err(format!(
                    "source-line map length mismatch for {}: {} mappings for {} lines",
                    entry.path,
                    source_lines.len(),
                    lines
                ));
            }
        }
    }

    let mut ledgers = Vec::new();
    let mut source_ids = BTreeSet::new();
    for source in &manifest.sources {
        validate_source_id(&source.id)?;
        if !source_ids.insert(source.id.clone()) {
            return Err(format!("duplicate archive source id: {}", source.id));
        }
        if source.snapshot_sha256 != snapshot_hash(&source.snapshot_files)? {
            return Err(format!(
                "source snapshot hash mismatch in manifest: {}",
                source.id
            ));
        }
        let mut snapshot_paths = BTreeMap::new();
        for snapshot_file in &source.snapshot_files {
            validate_relative_archive_path(&snapshot_file.path)?;
            if snapshot_paths
                .insert(snapshot_file.path.as_str(), snapshot_file)
                .is_some()
            {
                return Err(format!(
                    "duplicate source snapshot path for {}: {}",
                    source.id, snapshot_file.path
                ));
            }
        }
        for ledger in LEGACY_LEDGERS {
            let archive_path = format!("sources/{}/raw/{ledger}", source.id);
            let entry = entries.get(&archive_path).ok_or_else(|| {
                format!("manifest/archive missing required ledger: {archive_path}")
            })?;
            let bytes = fs::read(archive.join(&archive_path))
                .map_err(|e| format!("read {archive_path}: {e}"))?;
            validate_jsonl(&bytes, ledger)?;
            if entry.category != "raw_legacy_ledger" {
                return Err(format!(
                    "wrong category for source {}/{ledger}: {}",
                    source.id, entry.category
                ));
            }
            let snapshot_file = snapshot_paths.get(*ledger).copied();
            match (entry.source_present, snapshot_file) {
                (Some(true), Some(snapshot_file))
                    if snapshot_file.bytes == bytes.len() as u64
                        && snapshot_file.sha256 == sha256_hex(&bytes) => {}
                (Some(false), None) if bytes.is_empty() => {}
                _ => {
                    return Err(format!(
                        "raw legacy ledger does not match source snapshot presence/hash: {}/{}",
                        source.id, ledger
                    ));
                }
            }
            ledgers.push(ArchivedLedger {
                source_id: source.id.clone(),
                ledger: (*ledger).to_string(),
                archive_path,
                source_lines: (1..=physical_line_count(&bytes)).collect(),
                bytes,
            });
        }
        for ledger in &source.linked_ledgers {
            let archive_path = format!("sources/{}/records/{ledger}", source.id);
            let entry = entries
                .get(&archive_path)
                .ok_or_else(|| format!("manifest/archive missing linked rows: {archive_path}"))?;
            let bytes = fs::read(archive.join(&archive_path))
                .map_err(|e| format!("read {archive_path}: {e}"))?;
            if !snapshot_paths.contains_key(ledger.as_str()) {
                return Err(format!(
                    "linked ledger is absent from source snapshot: {}/{}",
                    source.id, ledger
                ));
            }
            validate_linked_records(&bytes, ledger, &archive_path)?;
            let source_lines = entry
                .source_lines
                .clone()
                .ok_or_else(|| format!("linked rows lack source-line map: {archive_path}"))?;
            ledgers.push(ArchivedLedger {
                source_id: source.id.clone(),
                ledger: ledger.clone(),
                archive_path,
                bytes,
                source_lines,
            });
        }
    }
    if manifest.source_comparisons != compare_manifest_sources(&manifest.sources) {
        return Err("source comparison summary does not match source snapshots".into());
    }
    validate_interpretation_materials(&manifest, &entries)?;

    for ledger in &ledgers {
        let latest_path = format!("sources/{}/latest/{}", ledger.source_id, ledger.ledger);
        if !entries.contains_key(&latest_path) {
            return Err(format!("latest projection missing: {latest_path}"));
        }
        let expected = latest_projection(&ledger.bytes, &ledger.ledger)?;
        let actual =
            fs::read(archive.join(&latest_path)).map_err(|e| format!("read {latest_path}: {e}"))?;
        if actual != expected {
            return Err(format!("latest projection mismatch: {latest_path}"));
        }
    }

    let inventory = build_inventory(&ledgers)?;
    let (expected_edges, expected_anomalies) =
        build_edges(&manifest.project.id, &ledgers, &inventory)?;
    validate_authorized_anomaly_contract(&manifest.project.id, &expected_anomalies)?;
    let expected_edge_bytes = jsonl_bytes(&expected_edges)?;
    let actual_edge_bytes =
        fs::read(archive.join("edges.jsonl")).map_err(|e| format!("read edges.jsonl: {e}"))?;
    if actual_edge_bytes != expected_edge_bytes {
        return Err("edges.jsonl does not match edges regenerated from archived rows".into());
    }

    let required_edge_count = expected_edges
        .iter()
        .filter(|edge| edge.closure_required)
        .count() as u64;
    if manifest.known_anomalies != expected_anomalies {
        return Err(
            "known anomaly whitelist does not match semantic predicates over raw rows".into(),
        );
    }
    let unresolved = expected_edges
        .iter()
        .filter(|edge| {
            edge.closure_required
                && !target_exists(edge, &inventory)
                && !expected_anomalies
                    .iter()
                    .any(|anomaly| anomaly_matches_edge(anomaly, edge))
        })
        .collect::<Vec<_>>();
    if manifest.closure.edge_count != expected_edges.len() as u64
        || manifest.closure.required_edge_count != required_edge_count
        || manifest.closure.unresolved_required_edges != unresolved.len() as u64
    {
        return Err("manifest closure counts do not match regenerated edges".into());
    }
    if !unresolved.is_empty() {
        let sample = unresolved
            .iter()
            .take(5)
            .map(|edge| {
                format!(
                    "{}/{}:{} {} -> {}:{}",
                    edge.source_id,
                    edge.source_ledger,
                    edge.source_store_line,
                    edge.field,
                    edge.target_kind,
                    edge.target_id
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "legacy foreign-key closure failed ({} unresolved): {sample}",
            unresolved.len()
        ));
    }

    Ok(VerifySummary {
        format: ARCHIVE_FORMAT.into(),
        archive: canonical_string(archive),
        files: manifest.files.len(),
        edges: expected_edges.len() as u64,
        closure: "verified".into(),
        known_anomalies: expected_anomalies.len() as u64,
    })
}
