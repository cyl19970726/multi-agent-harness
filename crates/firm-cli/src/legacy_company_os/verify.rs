use super::*;

/// copy of the archive.
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

    let mut entries = std::collections::BTreeMap::new();
    for entry in &manifest.files {
        validate_relative_archive_path(&entry.path)?;
        crate::legacy_export::reject_relative_symlink_components(
            archive,
            Path::new(&entry.path),
            "archive file",
        )?;
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
        if entries.insert(entry.path.clone(), entry).is_some() {
            return Err(format!("duplicate manifest path: {}", entry.path));
        }
    }

    // The echoed exclusion contract must match this verifier's own contract
    // exactly: an archive produced under a weaker contract is not acceptable.
    let expected_contract: Vec<ExclusionContractEcho> = EXCLUSION_CONTRACT
        .iter()
        .map(|rule| ExclusionContractEcho {
            name: rule.name.into(),
            is_dir: rule.is_dir,
            reason: rule.reason.label().into(),
        })
        .collect();
    if manifest.exclusion_contract != expected_contract {
        return Err("archive exclusion contract does not match verifier contract".into());
    }

    // No archived file may come from an excluded location. Recomputed from
    // the contract over every recorded source path, independent of the
    // exporter's own per-store exclusion list. Directory rules match exact
    // component names at any depth; file rules (including the *.token/*.key/
    // *.pem/.env.* patterns) apply to the final component only.
    for entry in &manifest.files {
        let Some(source_path) = &entry.source_path else {
            continue;
        };
        let source = Path::new(source_path);
        if !source.is_absolute() {
            return Err(format!(
                "archived file source location is not absolute: {}",
                entry.path
            ));
        }
        let mut components = source.components().peekable();
        while let Some(component) = components.next() {
            let name = component.as_os_str().to_str().ok_or_else(|| {
                format!("non-UTF-8 archived source path component: {source_path}")
            })?;
            let is_last = components.peek().is_none();
            let matched = if is_last {
                exclusion_for_name(name, false).or_else(|| exclusion_for_name(name, true))
            } else {
                exclusion_for_name(name, true)
            };
            if let Some(reason) = matched {
                return Err(format!(
                    "archived file {} comes from excluded location {} ({}: {})",
                    entry.path,
                    source_path,
                    reason.label(),
                    name
                ));
            }
        }
    }

    let empty_sha256 = sha256_hex(b"");
    for store in &manifest.stores {
        validate_store_archive_id(&store.id)?;
        if store.path.is_empty() || !Path::new(&store.path).is_absolute() {
            return Err(format!(
                "store source location is not absolute: {}",
                store.id
            ));
        }
        if store.ledgers.len() != LEDGER_CONTRACT.len() {
            // Verification is not relaxed: the archive still fails. The message
            // only becomes actionable, naming the exporter that wrote it and
            // the direction of the drift.
            let remedy = if store.ledgers.len() < LEDGER_CONTRACT.len() {
                "written before this binary's ledger contract grew; \
                 re-export with the current binary"
            } else {
                "written against a newer ledger contract this binary does not know; \
                 verify with the binary that produced it"
            };
            return Err(format!(
                "store {} has {} ledger entries, contract requires {}: {}, {}",
                store.id,
                store.ledgers.len(),
                LEDGER_CONTRACT.len(),
                archive_provenance(&manifest),
                remedy
            ));
        }
        for (ledger, contract) in store.ledgers.iter().zip(LEDGER_CONTRACT.iter()) {
            if ledger.ledger != contract.ledger
                || ledger.section != contract.section.label()
                || ledger.object_type != contract.object_type
                || ledger.schema_version != contract.section.schema_version()
            {
                return Err(format!(
                    "store {} ledger entry does not match the contract for {}",
                    store.id, contract.ledger
                ));
            }
            let expected_archive_path = format!("stores/{}/ledgers/{}", store.id, contract.ledger);
            if ledger.archive_path != expected_archive_path {
                return Err(format!(
                    "store {} ledger {} has wrong archive path: {}",
                    store.id, contract.ledger, ledger.archive_path
                ));
            }
            // Source location structure: the recorded ledger must sit exactly
            // at <store.path>/<ledger name>, so nothing nested inside an
            // excluded directory (or anywhere else) can pose as a ledger.
            let expected_source = Path::new(&store.path).join(contract.ledger);
            if Path::new(&ledger.source_path) != expected_source {
                return Err(format!(
                    "store {} ledger {} source path escapes its store root: {}",
                    store.id, contract.ledger, ledger.source_path
                ));
            }
            let entry = entries.get(&ledger.archive_path).ok_or_else(|| {
                format!(
                    "manifest files miss ledger archive path: {}",
                    ledger.archive_path
                )
            })?;
            if entry.category != "legacy_ledger"
                || entry.sha256 != ledger.sha256
                || entry.bytes != ledger.bytes
                || entry.rows != Some(ledger.rows)
                || entry.source_path.as_deref() != Some(ledger.source_path.as_str())
            {
                return Err(format!(
                    "ledger/file manifest mismatch: {}",
                    ledger.archive_path
                ));
            }
            if ledger.present {
                if ledger.bytes == 0 && ledger.rows != 0 {
                    return Err(format!(
                        "present ledger with rows but no bytes: {}",
                        ledger.archive_path
                    ));
                }
            } else if ledger.bytes != 0 || ledger.rows != 0 || ledger.sha256 != empty_sha256 {
                return Err(format!(
                    "absent ledger must be an empty archived entry: {}",
                    ledger.archive_path
                ));
            }
        }
        for excluded in &store.excluded_locations {
            let path = Path::new(&excluded.path);
            if !path.is_absolute() {
                return Err(format!(
                    "excluded location is not absolute: {}",
                    excluded.path
                ));
            }
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format!("excluded location has no file name: {}", excluded.path))?;
            let matches_reason = exclusion_for_name(name, true)
                .or_else(|| exclusion_for_name(name, false))
                .is_some_and(|reason| reason.label() == excluded.reason);
            if !matches_reason {
                return Err(format!(
                    "excluded location does not match the contract rule for its reason: {}",
                    excluded.path
                ));
            }
        }
        // The uncontracted audit list must be internally consistent: names
        // only, top-level `*.jsonl`, and disjoint from both the ledger
        // contract and the exclusion contract — otherwise a contracted or
        // excludable file could be laundered into the "current surface"
        // bucket where nothing else checks it.
        for name in &store.uncontracted_ledgers {
            if name.contains('/') || name.contains('\\') || !name.ends_with(".jsonl") {
                return Err(format!(
                    "store {} uncontracted entry is not a top-level jsonl name: {name}",
                    store.id
                ));
            }
            if LEDGER_CONTRACT.iter().any(|c| c.ledger == name.as_str()) {
                return Err(format!(
                    "store {} lists a contracted ledger as uncontracted: {name}",
                    store.id
                ));
            }
            if exclusion_for_name(name, true)
                .or_else(|| exclusion_for_name(name, false))
                .is_some()
            {
                return Err(format!(
                    "store {} lists an excludable location as uncontracted: {name}",
                    store.id
                ));
            }
        }
    }

    // Totals must recompute exactly from the store sections.
    let recomputed = ManifestTotals {
        stores: manifest.stores.len() as u64,
        ledgers_present: manifest
            .stores
            .iter()
            .flat_map(|s| s.ledgers.iter())
            .filter(|l| l.present)
            .count() as u64,
        rows: manifest
            .stores
            .iter()
            .flat_map(|s| s.ledgers.iter())
            .map(|l| l.rows)
            .sum(),
        bytes: manifest
            .stores
            .iter()
            .flat_map(|s| s.ledgers.iter())
            .map(|l| l.bytes)
            .sum(),
        excluded_locations_present: manifest
            .stores
            .iter()
            .flat_map(|s| s.excluded_locations.iter())
            .filter(|e| e.present)
            .count() as u64,
    };
    if manifest.totals != recomputed {
        return Err("manifest totals do not recompute from store sections".into());
    }

    // Restore-read proof: copy the manifest-listed files into an isolated
    // temp dir, then re-read every ledger from that detached copy — parsing
    // each row and rechecking counts and hashes — proving the archive alone
    // reconstructs readable records without any live store.
    let restored_rows = restore_read_proof(archive, &manifest)?;

    Ok(VerifySummary {
        format: ARCHIVE_FORMAT.into(),
        archive: canonical_string(archive),
        stores: manifest.stores.len(),
        ledgers_present: manifest.totals.ledgers_present,
        rows: restored_rows,
        files: manifest.files.len(),
        uncontracted_ledgers: manifest
            .stores
            .iter()
            .map(|s| s.uncontracted_ledgers.len() as u64)
            .sum(),
        restore_read: "verified".into(),
    })
}
