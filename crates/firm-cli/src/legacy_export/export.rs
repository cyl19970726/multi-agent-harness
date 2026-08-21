use super::*;

/// Create one immutable archive. The source store is only ever opened for read.
pub fn export_archive(
    store_root: &Path,
    project_id: Option<&str>,
    project_root: Option<&Path>,
    output: &Path,
) -> Result<ExportSummary, String> {
    let project_root = project_root.ok_or_else(|| {
        "legacy export needs an explicit project root; refusing implicit source discovery"
            .to_string()
    })?;
    reject_symlink_or_non_directory(store_root, "primary source store")?;
    reject_symlink_or_non_directory(project_root, "project root")?;
    if output.exists() {
        return Err(format!(
            "archive destination already exists (refusing to overwrite): {}",
            output.display()
        ));
    }
    let mut sources = discover_sources(store_root, project_root)?;
    reject_output_inside_roots(
        &sources
            .iter()
            .map(|source| source.root.as_path())
            .collect::<Vec<_>>(),
        project_root,
        output,
    )?;
    for source in &mut sources {
        source.before = snapshot_directory(&source.root)?;
    }

    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("create archive parent: {e}"))?;
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let output_name = output
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "archive destination needs a valid final path component".to_string())?;
    let staging_path = parent.join(format!(
        ".{output_name}.partial-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir(&staging_path).map_err(|e| format!("create archive staging dir: {e}"))?;
    let mut staging = StagingDir {
        path: staging_path,
        keep: false,
    };

    let resolved_project_id = project_id
        .map(str::to_string)
        .or_else(|| project_id_from_metadata(store_root))
        .unwrap_or_else(|| {
            store_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unidentified-project")
                .to_string()
        });

    let mut ledgers = Vec::new();
    let mut file_meta = FileMetaMap::new();
    let mut linked_rows = 0_u64;
    let mut manifest_sources = Vec::new();
    for source in &sources {
        let result = archive_source(source, &staging.path, &mut file_meta)?;
        linked_rows += result.linked_rows;
        ledgers.extend(result.ledgers);
        manifest_sources.push(ManifestSource {
            id: source.id.clone(),
            kind: source.kind.clone(),
            path: canonical_string(&source.root),
            snapshot_sha256: snapshot_hash(&source.before)?,
            snapshot_files: source.before.clone(),
            linked_ledgers: result.linked_ledgers,
        });
    }

    let inventory = build_inventory(&ledgers)?;
    let (edges, known_anomalies) = build_edges(&resolved_project_id, &ledgers, &inventory)?;
    validate_authorized_anomaly_contract(&resolved_project_id, &known_anomalies)?;
    let edges_bytes = jsonl_bytes(&edges)?;
    write_archive_file(&staging.path, "edges.jsonl", &edges_bytes)?;
    file_meta.insert(
        "edges.jsonl".into(),
        ("foreign_key_edges".into(), None, None, None),
    );

    let interpretation_materials =
        copy_interpretation_files(project_root, &staging.path, &mut file_meta)?;

    // Re-snapshot every source only after all reads and interpretation copies.
    // A difference means the archive could mix rows from different moments, so
    // the staging directory is discarded and nothing is published.
    for source in &sources {
        ensure_source_unchanged(source)?;
    }

    let mut files = Vec::new();
    for (path, (category, source_path, source_present, source_lines)) in file_meta {
        let bytes = fs::read(staging.path.join(&path))
            .map_err(|e| format!("read staged archive file {path}: {e}"))?;
        files.push(ManifestFile {
            path,
            category,
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            line_count: physical_line_count(&bytes),
            source_path,
            source_present,
            source_lines,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let required_edge_count = edges.iter().filter(|edge| edge.closure_required).count() as u64;
    let unresolved_required_edges = edges
        .iter()
        .filter(|edge| {
            edge.closure_required
                && !target_exists(edge, &inventory)
                && !known_anomalies
                    .iter()
                    .any(|anomaly| anomaly_matches_edge(anomaly, edge))
        })
        .count() as u64;
    let manifest = Manifest {
        format: ARCHIVE_FORMAT.into(),
        version: ARCHIVE_VERSION,
        exporter_version: EXPORTER_VERSION.into(),
        exported_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        project: ManifestProject {
            id: resolved_project_id.clone(),
            project_root: Some(canonical_string(project_root)),
        },
        workflow_archive: Some(WorkflowArchiveContract {
            encoding: "opaque-bytes".into(),
            ledgers: WORKFLOW_LEDGERS
                .iter()
                .map(|value| (*value).into())
                .collect(),
            patch_root: WORKFLOW_PATCH_ROOT.into(),
            restore_mode: "read-only".into(),
        }),
        source_comparisons: compare_manifest_sources(&manifest_sources),
        interpretation_materials,
        sources: manifest_sources,
        files,
        known_anomalies: known_anomalies.clone(),
        closure: ClosureSummary {
            edge_count: edges.len() as u64,
            required_edge_count,
            unresolved_required_edges,
        },
    };
    let mut manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| format!("serialize manifest: {e}"))?;
    manifest_bytes.push(b'\n');
    write_archive_file(&staging.path, "manifest.json", &manifest_bytes)?;

    fs::rename(&staging.path, output).map_err(|e| {
        format!(
            "publish archive {} -> {}: {e}",
            staging.path.display(),
            output.display()
        )
    })?;
    staging.keep = true;

    Ok(ExportSummary {
        format: ARCHIVE_FORMAT.into(),
        archive: canonical_string(output),
        project_id: resolved_project_id,
        source_stores: manifest
            .sources
            .iter()
            .map(|source| source.path.clone())
            .collect(),
        files: manifest.files.len(),
        linked_rows,
        edges: edges.len() as u64,
        unresolved_required_edges,
        known_anomalies: known_anomalies.len() as u64,
    })
}
