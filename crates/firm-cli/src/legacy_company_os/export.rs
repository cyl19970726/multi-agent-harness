use super::*;

/// Every source store is only ever opened for read; nothing is deleted.
pub fn export_archive(firm_home: &Path, output: &Path) -> Result<ExportSummary, String> {
    reject_symlink_or_non_directory(firm_home, "Firm home")?;
    if output.exists() {
        return Err(format!(
            "archive destination already exists (refusing to overwrite): {}",
            output.display()
        ));
    }
    let stores = enumerate_stores(firm_home)?;
    reject_output_inside_sources(firm_home, &stores, output)?;

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

    // Snapshot exactly what the export reads (contracted ledgers and
    // control-plane files, plus each store's top-level entry names) so a
    // source that moves mid-export discards the staging directory instead of
    // publishing a mixed-moment archive. Excluded locations are listed by
    // name only; their content is never read, hashed, or archived.
    let before = snapshot_inputs(&stores, firm_home)?;

    let mut files: Vec<ManifestFile> = Vec::new();
    archive_control_plane_files(firm_home, &staging.path, &mut files)?;

    let mut manifest_stores: Vec<ManifestStore> = Vec::new();
    let mut ledgers_present = 0_u64;
    let mut total_rows = 0_u64;
    let mut total_bytes = 0_u64;
    let mut excluded_locations = 0_u64;
    let mut uncontracted_ledgers = 0_u64;
    for store in &stores {
        let archived = archive_store(store, &staging.path, &mut files)?;
        uncontracted_ledgers += archived.uncontracted_ledgers.len() as u64;
        ledgers_present += archived.ledgers.iter().filter(|l| l.present).count() as u64;
        total_rows += archived.ledgers.iter().map(|l| l.rows).sum::<u64>();
        total_bytes += archived.ledgers.iter().map(|l| l.bytes).sum::<u64>();
        excluded_locations += archived
            .excluded_locations
            .iter()
            .filter(|e| e.present)
            .count() as u64;
        manifest_stores.push(archived);
    }

    // Re-read every input only after all reads; a difference means the
    // archive could mix rows from different moments.
    ensure_inputs_unchanged(&before, &stores, firm_home)?;

    files.sort_by(|a, b| a.path.cmp(&b.path));
    let totals = ManifestTotals {
        stores: manifest_stores.len() as u64,
        ledgers_present,
        rows: total_rows,
        bytes: total_bytes,
        excluded_locations_present: excluded_locations,
    };
    let manifest = Manifest {
        format: ARCHIVE_FORMAT.into(),
        version: ARCHIVE_VERSION,
        exporter_version: EXPORTER_VERSION.into(),
        source_revision: source_revision().into(),
        exported_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        firm_home: canonical_string(firm_home),
        stores: manifest_stores,
        exclusion_contract: EXCLUSION_CONTRACT
            .iter()
            .map(|rule| ExclusionContractEcho {
                name: rule.name.into(),
                is_dir: rule.is_dir,
                reason: rule.reason.label().into(),
            })
            .collect(),
        files,
        totals,
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
        firm_home: canonical_string(firm_home),
        exporter_version: EXPORTER_VERSION.into(),
        source_revision: source_revision().into(),
        stores: manifest.stores.len(),
        ledgers_present,
        rows: total_rows,
        bytes: total_bytes,
        files: manifest.files.len(),
        excluded_locations,
        uncontracted_ledgers,
    })
}
