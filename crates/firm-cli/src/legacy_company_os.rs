//! Read-only export + verification for the retired Company OS record surface
//! (DOC-108 Stage A: the machinery that makes later deletion safe).
//!
//! The archive preserves source JSONL bytes byte-for-byte. Per source record
//! store enumerated on this machine (Company Stores, Execution Space stores,
//! project and repo-local compatibility stores, machine node stores) the
//! manifest records the absolute source location, ledger/object type, schema
//! version, row count, byte count, and SHA-256 of every contracted legacy
//! ledger, plus the exporter version and the exact source revision of the
//! exporting binary. Secret and provider-native locations are listed as
//! excluded and are never exported.
//!
//! Hard boundaries: no secret/token export, no provider-native transcript
//! copying, no name-based mapping into current Work, and no deletion — this
//! stage mutates nothing outside the archive destination.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::legacy_export::{
    canonical_string, physical_line_count, reject_symlink_ancestors,
    reject_symlink_or_non_directory, resolve_with_existing_ancestor, sha256_hex,
    validate_relative_archive_path, write_archive_file, StagingDir,
};

const ARCHIVE_FORMAT: &str = "legacy-company-os-v1";
const ARCHIVE_VERSION: u32 = 1;
const EXPORTER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Same compile-time provenance as `--build-info`: build.rs embeds
/// `FIRM_BUILD_GIT_REV`, and a build outside a git checkout falls back to
/// "unknown" instead of failing.
fn source_revision() -> &'static str {
    option_env!("FIRM_BUILD_GIT_REV").unwrap_or("unknown")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSummary {
    pub format: String,
    pub archive: String,
    pub firm_home: String,
    pub exporter_version: String,
    pub source_revision: String,
    pub stores: usize,
    pub ledgers_present: u64,
    pub rows: u64,
    pub bytes: u64,
    pub files: usize,
    pub excluded_locations: u64,
    #[serde(default)]
    pub uncontracted_ledgers: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifySummary {
    pub format: String,
    pub archive: String,
    pub stores: usize,
    pub ledgers_present: u64,
    pub rows: u64,
    pub files: usize,
    #[serde(default)]
    pub uncontracted_ledgers: u64,
    pub restore_read: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    exporter_version: String,
    source_revision: String,
    exported_at_unix_ms: u128,
    firm_home: String,
    stores: Vec<ManifestStore>,
    /// The exclusion contract the exporter applied, echoed so verification
    /// and later deletion stages can audit it offline.
    exclusion_contract: Vec<ExclusionContractEcho>,
    files: Vec<ManifestFile>,
    totals: ManifestTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExclusionContractEcho {
    name: String,
    is_dir: bool,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestStore {
    /// Stable archive id: `<kind>:<source-id>`, path-safe.
    id: String,
    kind: String,
    /// Absolute source location at export time.
    path: String,
    /// Whether the store directory existed when enumerated.
    present: bool,
    /// Identity fields copied from the store's own metadata.json, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity: Option<serde_json::Value>,
    ledgers: Vec<ManifestLedger>,
    excluded_locations: Vec<ExcludedLocation>,
    /// Names of top-level `*.jsonl` files that are neither contracted nor
    /// excluded (current, non-retired surfaces). Recorded so the manifest's
    /// completeness claim is auditable: nothing on disk is invisible.
    #[serde(default)]
    uncontracted_ledgers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestLedger {
    ledger: String,
    section: String,
    object_type: String,
    /// Exporter read-contract tag for this ledger family. Source rows do not
    /// carry per-row schema versions; this names the archive contract under
    /// which the preserved bytes remain readable.
    schema_version: String,
    /// Absolute source location at export time.
    source_path: String,
    present: bool,
    rows: u64,
    bytes: u64,
    sha256: String,
    archive_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExcludedLocation {
    /// Absolute source location explicitly never exported.
    path: String,
    reason: String,
    present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestFile {
    path: String,
    category: String,
    sha256: String,
    bytes: u64,
    line_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rows: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestTotals {
    stores: u64,
    ledgers_present: u64,
    rows: u64,
    bytes: u64,
    excluded_locations_present: u64,
}

/// Which retired surface a contracted ledger belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LedgerSection {
    /// The retired Company OS record ledgers (Docs, Organization, Work,
    /// Finance, programmable pages, action/audit machinery).
    CompanyOs,
    /// Mission / Mission Log / pre-ADR-0051 Wave coordination ledgers.
    MissionCoordination,
    /// Legacy AgentIdentity compatibility rows (retired members/agents
    /// identity ledgers and their events/runtimes).
    AgentIdentityCompatibility,
    /// Preserved-but-retired read-only history (TeamMessage projections,
    /// provider dispatch history).
    RetiredHistory,
}

impl LedgerSection {
    fn label(self) -> &'static str {
        match self {
            Self::CompanyOs => "company_os",
            Self::MissionCoordination => "mission_coordination",
            Self::AgentIdentityCompatibility => "agent_identity_compatibility",
            Self::RetiredHistory => "retired_history",
        }
    }

    /// Exporter read-contract tag for the section. Source rows carry no
    /// uniform per-row schema version, so this names the archive contract
    /// under which the preserved bytes remain readable; it is honestly the
    /// exporter's own versioned interpretation, not a source-derived field.
    fn schema_version(self) -> &'static str {
        match self {
            Self::CompanyOs => "company-os-ledger-v1",
            Self::MissionCoordination => "mission-coordination-ledger-v1",
            Self::AgentIdentityCompatibility => "agent-identity-compat-ledger-v1",
            Self::RetiredHistory => "retired-history-ledger-v1",
        }
    }
}

struct LedgerContract {
    ledger: &'static str,
    object_type: &'static str,
    section: LedgerSection,
}

/// The explicit legacy ledger contract (DOC-108). Every contracted ledger is
/// enumerated per store — present or explicitly absent — so a later deletion
/// stage can prove nothing contracted was left behind. Non-contracted files
/// are either excluded locations (below) or current surfaces outside the
/// retirement scope.
const LEDGER_CONTRACT: &[LedgerContract] = &[
    // --- Company OS: Docs ---
    LedgerContract {
        ledger: "company_os_documents.jsonl",
        object_type: "company_os_document",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_blocks.jsonl",
        object_type: "company_os_block",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_blocks_v2.jsonl",
        object_type: "company_os_block_v2",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_document_revisions.jsonl",
        object_type: "company_os_document_revision",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_document_change_ops.jsonl",
        object_type: "company_os_document_change_op",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_typed_records.jsonl",
        object_type: "company_os_typed_record",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_relations.jsonl",
        object_type: "company_os_relation",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_views.jsonl",
        object_type: "company_os_view",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_business_modules.jsonl",
        object_type: "company_os_business_module",
        section: LedgerSection::CompanyOs,
    },
    // --- Company OS: actors / Organization ---
    LedgerContract {
        ledger: "company_os_human_members.jsonl",
        object_type: "company_os_human_member_actor",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_human_provider_launch_profiles.jsonl",
        object_type: "company_os_human_provider_launch_profile",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_agent_memberships.jsonl",
        object_type: "company_os_agent_membership_actor",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_external_participants.jsonl",
        object_type: "company_os_external_participant_actor",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_service_actors.jsonl",
        object_type: "company_os_service_actor",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_org_units.jsonl",
        object_type: "company_os_org_unit",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_organization_memberships.jsonl",
        object_type: "company_os_organization_membership",
        section: LedgerSection::CompanyOs,
    },
    // --- Company OS: Work / Finance / Approvals ---
    LedgerContract {
        ledger: "company_os_milestones.jsonl",
        object_type: "company_os_milestone",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_approvals.jsonl",
        object_type: "company_os_approval",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_commitments.jsonl",
        object_type: "company_os_commitment",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_payments.jsonl",
        object_type: "company_os_payment",
        section: LedgerSection::CompanyOs,
    },
    // --- Company OS: programmable pages + action/audit machinery ---
    LedgerContract {
        ledger: "company_os_custom_page_definitions.jsonl",
        object_type: "company_os_custom_page_definition",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_custom_page_packages.jsonl",
        object_type: "company_os_custom_page_package",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_action_commands.jsonl",
        object_type: "company_os_action_command",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_action_policy_definitions.jsonl",
        object_type: "company_os_action_policy_definition",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_audit_events.jsonl",
        object_type: "company_os_audit_event",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_action_audit_reservations.jsonl",
        object_type: "company_os_action_audit_reservation",
        section: LedgerSection::CompanyOs,
    },
    // --- Company OS: Work cutover leftovers ---
    LedgerContract {
        ledger: "company_os_work_items.jsonl",
        object_type: "company_os_work_item",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_assignments.jsonl",
        object_type: "company_os_assignment",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_work_cutover_fences.jsonl",
        object_type: "company_os_work_cutover_fence",
        section: LedgerSection::CompanyOs,
    },
    LedgerContract {
        ledger: "company_os_standing_agents.jsonl",
        object_type: "legacy_standing_agent",
        section: LedgerSection::CompanyOs,
    },
    // --- Mission / Wave coordination ---
    LedgerContract {
        ledger: "missions.jsonl",
        object_type: "mission",
        section: LedgerSection::MissionCoordination,
    },
    LedgerContract {
        ledger: "mission_log.jsonl",
        object_type: "mission_log_entry",
        section: LedgerSection::MissionCoordination,
    },
    LedgerContract {
        ledger: "waves.jsonl",
        object_type: "legacy_wave",
        section: LedgerSection::MissionCoordination,
    },
    // --- AgentIdentity compatibility rows ---
    LedgerContract {
        ledger: "members.jsonl",
        object_type: "legacy_member_identity",
        section: LedgerSection::AgentIdentityCompatibility,
    },
    LedgerContract {
        ledger: "agents.jsonl",
        object_type: "legacy_agent_identity",
        section: LedgerSection::AgentIdentityCompatibility,
    },
    LedgerContract {
        ledger: "agent_identities.jsonl",
        object_type: "agent_identity_compatibility_row",
        section: LedgerSection::AgentIdentityCompatibility,
    },
    LedgerContract {
        ledger: "agent_events.jsonl",
        object_type: "legacy_agent_event",
        section: LedgerSection::AgentIdentityCompatibility,
    },
    LedgerContract {
        ledger: "agent_runtimes.jsonl",
        object_type: "legacy_agent_runtime",
        section: LedgerSection::AgentIdentityCompatibility,
    },
    LedgerContract {
        ledger: "durable_agent_members.jsonl",
        object_type: "legacy_durable_agent_member",
        section: LedgerSection::AgentIdentityCompatibility,
    },
    // --- Retired read-only history sections ---
    LedgerContract {
        ledger: "team_messages.jsonl",
        object_type: "team_message_projection",
        section: LedgerSection::RetiredHistory,
    },
    LedgerContract {
        ledger: "provider_dispatch_events.jsonl",
        object_type: "provider_dispatch_event",
        section: LedgerSection::RetiredHistory,
    },
    LedgerContract {
        ledger: "company_store_migrations.jsonl",
        object_type: "company_store_migration",
        section: LedgerSection::RetiredHistory,
    },
];

/// Why a location is excluded from the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExclusionReason {
    /// Secret/token-bearing file (credentials, keys, env files). Never read
    /// into the archive.
    SecretFile,
    /// Provider-native session transcripts; the provider's own store is the
    /// sole transcript authority and is never copied (ADR 0032).
    ProviderNativeTranscript,
    /// Provider runtime working state (may embed environment or transcripts).
    ProviderNativeRuntimeState,
    /// Harness-authored prompt text; unstructured and may embed secrets.
    UnstructuredPromptContent,
    /// IPC endpoints and lock files; meaningless outside the live machine.
    EphemeralIpcOrLock,
    /// Current (not retired) Remote Fabric / collaboration state: outside the
    /// legacy Company OS retirement scope but listed so deletion stages can
    /// see the full surface.
    OutOfScopeCurrentState,
}

impl ExclusionReason {
    fn label(self) -> &'static str {
        match self {
            Self::SecretFile => "secret_file",
            Self::ProviderNativeTranscript => "provider_native_transcript",
            Self::ProviderNativeRuntimeState => "provider_native_runtime_state",
            Self::UnstructuredPromptContent => "unstructured_prompt_content",
            Self::EphemeralIpcOrLock => "ephemeral_ipc_or_lock",
            Self::OutOfScopeCurrentState => "out_of_scope_current_state",
        }
    }
}

/// One exclusion-contract rule, matched against a store's top-level entries.
/// The legacy ledgers are all top-level files, so top-level matching covers
/// the whole export surface; everything under an excluded directory inherits
/// its exclusion.
struct ExclusionRule {
    name: &'static str,
    is_dir: bool,
    reason: ExclusionReason,
}

const EXCLUSION_CONTRACT: &[ExclusionRule] = &[
    ExclusionRule {
        name: "provider-sessions",
        is_dir: true,
        reason: ExclusionReason::ProviderNativeTranscript,
    },
    ExclusionRule {
        name: "runtimes",
        is_dir: true,
        reason: ExclusionReason::ProviderNativeRuntimeState,
    },
    ExclusionRule {
        name: "prompts",
        is_dir: true,
        reason: ExclusionReason::UnstructuredPromptContent,
    },
    ExclusionRule {
        name: "remote-fabric",
        is_dir: true,
        reason: ExclusionReason::OutOfScopeCurrentState,
    },
    ExclusionRule {
        name: "collaboration-v1",
        is_dir: true,
        reason: ExclusionReason::OutOfScopeCurrentState,
    },
    ExclusionRule {
        name: ".env",
        is_dir: false,
        reason: ExclusionReason::SecretFile,
    },
    ExclusionRule {
        name: "secrets.json",
        is_dir: false,
        reason: ExclusionReason::SecretFile,
    },
    ExclusionRule {
        name: "tokens.json",
        is_dir: false,
        reason: ExclusionReason::SecretFile,
    },
    ExclusionRule {
        name: "auth.json",
        is_dir: false,
        reason: ExclusionReason::SecretFile,
    },
    ExclusionRule {
        name: "credentials.json",
        is_dir: false,
        reason: ExclusionReason::SecretFile,
    },
    ExclusionRule {
        name: "daemon.sock",
        is_dir: false,
        reason: ExclusionReason::EphemeralIpcOrLock,
    },
    ExclusionRule {
        name: ".registry.lock",
        is_dir: false,
        reason: ExclusionReason::EphemeralIpcOrLock,
    },
];

/// Name-pattern exclusions beyond exact top-level names: `.env.*`,
/// `*.token`, `*.key`, `*.pem`, `*.sock`.
fn exclusion_for_name(name: &str, is_dir: bool) -> Option<ExclusionReason> {
    for rule in EXCLUSION_CONTRACT {
        if rule.name == name && rule.is_dir == is_dir {
            return Some(rule.reason);
        }
    }
    if is_dir {
        return None;
    }
    if name.starts_with(".env.")
        || name.ends_with(".token")
        || name.ends_with(".key")
        || name.ends_with(".pem")
    {
        return Some(ExclusionReason::SecretFile);
    }
    if name.ends_with(".sock") || name.ends_with(".lock") {
        return Some(ExclusionReason::EphemeralIpcOrLock);
    }
    None
}

/// Control-plane enumeration inputs archived alongside the ledgers so the
/// store list in the manifest is auditable offline. These are registry and
/// marker files only; none of them carries secrets.
const CONTROL_PLANE_REGISTRIES: &[(&str, &str)] = &[
    (
        "projects/registry.json",
        "registries/projects.registry.json",
    ),
    (
        "companies/registry.json",
        "registries/companies.registry.json",
    ),
    (
        "execution-spaces/registry.json",
        "registries/execution-spaces.registry.json",
    ),
];

const CONTROL_PLANE_MARKERS: &[&str] = &[
    "ACTIVE_PROJECT",
    "ACTIVE_COMPANY",
    "ACTIVE_SPACE",
    "NODE_ID",
];

/// Create one immutable archive of the retired Company OS record surface.
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

/// Exporter provenance recorded in the manifest, rendered for diagnostics.
///
/// `LEDGER_CONTRACT` grows as retired surfaces are added without bumping
/// `ARCHIVE_VERSION` (the on-disk shape is unchanged), so an archive written by
/// an earlier binary fails the contract-length cross-check. The manifest
/// already carries the exporter identity, so that failure can name the binary
/// that produced the archive instead of reporting a bare length difference.
fn archive_provenance(manifest: &Manifest) -> String {
    format!(
        "archive produced by exporter {} (source revision {})",
        manifest.exporter_version, manifest.source_revision
    )
}

/// Verify an archive without consulting any live store: manifest hashes and
/// byte/line counts, the ledger/exclusion contract cross-checks, and a
/// restore-read proof that re-reads every ledger from an isolated temp-dir
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

fn validate_store_archive_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!("unsafe archive store id: {value}"));
    }
    Ok(())
}

/// Copy every manifest-listed file into an isolated temp dir and re-read all
/// ledgers from that detached copy. Returns the total restored row count.
fn restore_read_proof(archive: &Path, manifest: &Manifest) -> Result<u64, String> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let restore_path = std::env::temp_dir().join(format!(
        "legacy-company-os-restore-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir(&restore_path).map_err(|e| format!("create restore dir: {e}"))?;
    let restore = StagingDir {
        path: restore_path,
        keep: false,
    };

    for entry in &manifest.files {
        let bytes = fs::read(archive.join(&entry.path))
            .map_err(|e| format!("restore read {}: {e}", entry.path))?;
        write_archive_file(&restore.path, &entry.path, &bytes)?;
    }

    let mut restored_rows = 0_u64;
    for store in &manifest.stores {
        for ledger in &store.ledgers {
            let restored = restore.path.join(&ledger.archive_path);
            let bytes = fs::read(&restored)
                .map_err(|e| format!("read restored {}: {e}", restored.display()))?;
            if sha256_hex(&bytes) != ledger.sha256 {
                return Err(format!(
                    "restored hash mismatch for {}",
                    ledger.archive_path
                ));
            }
            let rows =
                crate::legacy_export::jsonl_records(&bytes, &ledger.archive_path)?.len() as u64;
            if rows != ledger.rows {
                return Err(format!(
                    "restored row-count mismatch for {}: manifest {}, restored {}",
                    ledger.archive_path, ledger.rows, rows
                ));
            }
            restored_rows += rows;
        }
    }
    // Control-plane registries must parse as JSON from the restored copy.
    for entry in &manifest.files {
        if entry.category != "control_plane_registry" {
            continue;
        }
        let bytes = fs::read(restore.path.join(&entry.path))
            .map_err(|e| format!("read restored {}: {e}", entry.path))?;
        if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
            return Err(format!(
                "restored control-plane registry is not valid JSON: {}",
                entry.path
            ));
        }
    }
    Ok(restored_rows)
}

/// One enumerated source record store on this machine.
#[derive(Debug, Clone)]
struct SourceStore {
    /// Archive id `<kind>-<source-id>`, validated path-safe.
    id: String,
    kind: &'static str,
    root: PathBuf,
    present: bool,
    identity: Option<serde_json::Value>,
}

/// Read-only view over the retired Company Store registry (DOC-108). This is
/// the export path's only dependence on the registry format — the writers and
/// the CLI/serve selection layer are gone; the reader stays so historical
/// Company Stores remain enumerable for export/verify.
mod retired_company_registry {
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CompanyContext {
        pub id: String,
        pub name: String,
        pub store_root: PathBuf,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct CompanyRegistryEntry {
        id: String,
        name: String,
        store_root: PathBuf,
    }

    #[derive(Debug, Clone, Default, Deserialize)]
    struct CompanyRegistry {
        #[serde(default)]
        companies: Vec<CompanyRegistryEntry>,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct CompanyMetadata {
        company_id: String,
        name: String,
    }

    fn companies_dir(firm_home: &Path) -> PathBuf {
        firm_home.join("companies")
    }

    fn read_metadata(store_root: &Path) -> Result<Option<CompanyMetadata>, String> {
        let path = store_root.join("metadata.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| format!("parse {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("read {}: {e}", path.display())),
        }
    }

    /// Merge registry entries and on-disk stores with metadata.json, deduped
    /// by id and sorted for a stable export manifest.
    pub fn list_companies(firm_home: &Path) -> Result<Vec<CompanyContext>, String> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let registry_file = companies_dir(firm_home).join("registry.json");
        match std::fs::read_to_string(&registry_file) {
            Ok(text) if !text.trim().is_empty() => {
                let registry: CompanyRegistry = serde_json::from_str(&text)
                    .map_err(|e| format!("parse {}: {e}", registry_file.display()))?;
                for entry in registry.companies {
                    if seen.insert(entry.id.clone()) {
                        out.push(CompanyContext {
                            id: entry.id,
                            name: entry.name,
                            store_root: entry.store_root,
                        });
                    }
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("read {}: {e}", registry_file.display())),
        }

        if let Ok(read_dir) = std::fs::read_dir(companies_dir(firm_home)) {
            for dir_entry in read_dir.flatten() {
                if !dir_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let id = match dir_entry.file_name().into_string() {
                    Ok(name) => name,
                    Err(_) => continue,
                };
                if seen.contains(&id) {
                    continue;
                }
                let store_root = dir_entry.path();
                if let Ok(Some(meta)) = read_metadata(&store_root) {
                    seen.insert(id);
                    out.push(CompanyContext {
                        id: meta.company_id,
                        name: meta.name,
                        store_root,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }
}

/// Enumerate every source record store under the resolved Firm home: Company
/// Stores, Execution Space stores, project-derived compatibility stores,
/// repo-local compatibility stores (`<project_root>/.harness`), and machine
/// node stores. Registries and on-disk layouts are both consulted and deduped
/// by canonical path; the store id list is never hardcoded.
///
/// Scope note: the product resolves exactly one Firm home (`FIRM_HOME`, else
/// `~/.firm`, else the legacy `~/.harness` fallback). Stores under that home
/// plus repo-local compatibility stores of its known projects are the machine
/// surface the product can see; a second home the product itself would never
/// resolve is out of scope.
fn enumerate_stores(firm_home: &Path) -> Result<Vec<SourceStore>, String> {
    let mut stores = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    if let Ok(home) = fs::canonicalize(firm_home) {
        seen.insert(home);
    }

    // 1. Company Stores (ADR 0040): the company layer merges registry entries
    //    and on-disk stores with metadata.json.
    for ctx in retired_company_registry::list_companies(firm_home)
        .map_err(|e| format!("enumerate Company Stores: {e}"))?
    {
        let identity = store_identity(&ctx.store_root);
        push_store(
            &mut stores,
            &mut seen,
            "company",
            &ctx.id,
            ctx.store_root,
            identity,
        )?;
    }

    // 2. Execution Space stores (ADR 0042): registry entries, plus on-disk
    //    space stores the registry does not know (mirrors the company/project
    //    layers' registry+scan merge, which `list_spaces` does not do).
    for space in crate::execution_space::list_spaces(firm_home)
        .map_err(|e| format!("enumerate Execution Space stores: {e}"))?
    {
        let identity = store_identity(&space.store_root);
        push_store(
            &mut stores,
            &mut seen,
            "space",
            &space.id,
            space.store_root,
            identity,
        )?;
    }
    for dir in child_directories(&crate::execution_space::spaces_dir(firm_home))? {
        let identity = store_identity(&dir);
        let id = identity
            .as_ref()
            .and_then(|value| value.get("space_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| dir.file_name().and_then(|s| s.to_str()).map(str::to_string))
            .ok_or_else(|| format!("non-UTF-8 Execution Space dir name: {}", dir.display()))?;
        push_store(&mut stores, &mut seen, "space", &id, dir, identity)?;
    }

    // 3. Project-derived compatibility stores: the project layer merges
    //    registry entries, on-disk stores with metadata.json, and the reserved
    //    _global project.
    let projects = crate::project::list_projects(firm_home)
        .map_err(|e| format!("enumerate Project compatibility stores: {e}"))?;
    for ctx in &projects {
        let identity = store_identity(&ctx.store_root);
        push_store(
            &mut stores,
            &mut seen,
            "project",
            &ctx.id,
            ctx.store_root.clone(),
            identity,
        )?;
    }

    // 4. Repo-local compatibility stores (`<project_root>/.harness`), the
    //    pre-centralization layout. The reserved _global project is skipped:
    //    its root is HOME, and `<home>/.harness` is the legacy Firm home
    //    fallback — when that fallback is active it IS the resolved Firm home
    //    this enumeration already covers, and the canonical-path dedup above
    //    would drop a duplicate probe anyway.
    for ctx in &projects {
        if ctx.id == harness_core::GLOBAL_PROJECT_ID {
            continue;
        }
        let local = ctx.project_root.join(".harness");
        if !local.exists() {
            continue;
        }
        reject_symlink_or_non_directory(&local, "repo-local source store")?;
        let mut identity = store_identity(&local).unwrap_or_else(|| serde_json::json!({}));
        if let Some(target) = crate::project::read_migrated_marker(&local)
            .map_err(|e| format!("read migrated marker in {}: {e}", local.display()))?
        {
            identity["migrated_to_central"] =
                serde_json::Value::String(target.display().to_string());
        }
        push_store(
            &mut stores,
            &mut seen,
            "repo-local",
            &ctx.id,
            local,
            Some(identity),
        )?;
    }

    // 5. Machine node stores (`<firm_home>/nodes/<node_id>/`), the
    //    machine-scoped NodeDaemon surface.
    for dir in child_directories(&firm_home.join("nodes"))? {
        let id = dir
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("non-UTF-8 node store dir name: {}", dir.display()))?
            .to_string();
        let identity = store_identity(&dir);
        push_store(&mut stores, &mut seen, "node", &id, dir, identity)?;
    }

    stores.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(stores)
}

fn push_store(
    stores: &mut Vec<SourceStore>,
    seen: &mut BTreeSet<PathBuf>,
    kind: &'static str,
    source_id: &str,
    root: PathBuf,
    identity: Option<serde_json::Value>,
) -> Result<(), String> {
    validate_store_source_id(source_id)?;
    let present = root.is_dir();
    // Dedup by canonical path so one physical store reached through two
    // routes (registry + on-disk scan, or repo-local alias) is exported once.
    let dedup_key = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    if !seen.insert(dedup_key) {
        return Ok(());
    }
    stores.push(SourceStore {
        id: format!("{kind}-{source_id}"),
        kind,
        root,
        present,
        identity,
    });
    Ok(())
}

fn validate_store_source_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!("unsafe archive store source id: {value}"));
    }
    Ok(())
}

/// The store's own identity record (`metadata.json`), kept as opaque JSON:
/// company_id/name, space_id/name, or project_id/canonical_path/kind.
fn store_identity(root: &Path) -> Option<serde_json::Value> {
    let bytes = fs::read(root.join("metadata.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.is_object().then_some(value)
}

fn child_directories(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let read_dir = match fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(error) => return Err(format!("read directory {}: {error}", dir.display())),
    };
    for entry in read_dir {
        let entry = entry.map_err(|e| format!("read entry in {}: {e}", dir.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("inspect enumerated store {}: {e}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "enumerated store must not be a symlink: {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Archive one enumerated store's contracted legacy ledgers into the staging
/// directory and return its manifest section.
fn archive_store(
    store: &SourceStore,
    archive_root: &Path,
    files: &mut Vec<ManifestFile>,
) -> Result<ManifestStore, String> {
    if store.present {
        reject_symlink_or_non_directory(&store.root, "enumerated source store")?;
    }
    // Ledger source paths are recorded under the canonical store root so the
    // manifest's store.path is always their exact parent — verification can
    // then prove structure (top-level contract ledgers only) instead of
    // pattern-matching arbitrary paths.
    let canonical_root = fs::canonicalize(&store.root).unwrap_or_else(|_| store.root.clone());
    let prefix = format!("stores/{}", store.id);
    let mut ledgers = Vec::new();
    for contract in LEDGER_CONTRACT {
        let source_path = canonical_root.join(contract.ledger);
        let (bytes, present) = if store.present && source_path.exists() {
            let metadata = fs::symlink_metadata(&source_path)
                .map_err(|e| format!("inspect ledger {}: {e}", source_path.display()))?;
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "legacy ledger must not be a symlink: {}",
                    source_path.display()
                ));
            }
            if !metadata.is_file() {
                return Err(format!(
                    "legacy ledger is not a regular file: {}",
                    source_path.display()
                ));
            }
            (
                fs::read(&source_path)
                    .map_err(|e| format!("read {}: {e}", source_path.display()))?,
                true,
            )
        } else {
            (Vec::new(), false)
        };
        // Every preserved row must parse as JSON; an archive of unparseable
        // bytes would fail the restore-read proof later, so fail the export
        // loudly instead of archiving garbage.
        let rows = crate::legacy_export::jsonl_records(
            &bytes,
            &format!("{}/{}", store.id, contract.ledger),
        )?
        .len() as u64;
        let archive_path = format!("{prefix}/ledgers/{}", contract.ledger);
        write_archive_file(archive_root, &archive_path, &bytes)?;
        let sha256 = sha256_hex(&bytes);
        files.push(ManifestFile {
            path: archive_path.clone(),
            category: "legacy_ledger".into(),
            sha256: sha256.clone(),
            bytes: bytes.len() as u64,
            line_count: physical_line_count(&bytes),
            rows: Some(rows),
            source_path: Some(source_path.display().to_string()),
        });
        ledgers.push(ManifestLedger {
            ledger: contract.ledger.into(),
            section: contract.section.label().into(),
            object_type: contract.object_type.into(),
            schema_version: contract.section.schema_version().into(),
            source_path: source_path.display().to_string(),
            present,
            rows,
            bytes: bytes.len() as u64,
            sha256,
            archive_path,
        });
    }
    let excluded_locations = if store.present {
        excluded_locations_for_store(&canonical_root)?
    } else {
        Vec::new()
    };
    let uncontracted_ledgers = if store.present {
        uncontracted_ledgers_for_store(&canonical_root, &excluded_locations)?
    } else {
        Vec::new()
    };
    Ok(ManifestStore {
        id: store.id.clone(),
        kind: store.kind.into(),
        path: canonical_root.display().to_string(),
        present: store.present,
        identity: store.identity.clone(),
        ledgers,
        excluded_locations,
        uncontracted_ledgers,
    })
}

/// Names of top-level `*.jsonl` files matching neither the ledger contract nor
/// the exclusion contract. Names only: content is never opened. This makes the
/// docstring's "nothing contracted was left behind" claim auditable — a stray
/// legacy-looking file can no longer be silently invisible in the manifest.
fn uncontracted_ledgers_for_store(
    root: &Path,
    excluded: &[ExcludedLocation],
) -> Result<Vec<String>, String> {
    let contracted: std::collections::BTreeSet<&str> =
        LEDGER_CONTRACT.iter().map(|c| c.ledger).collect();
    let excluded_names: std::collections::BTreeSet<String> = excluded
        .iter()
        .filter_map(|e| {
            Path::new(&e.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .collect();
    let mut names = Vec::new();
    let entries =
        fs::read_dir(root).map_err(|e| format!("enumerate store {}: {e}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("enumerate store {}: {e}", root.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".jsonl") {
            continue;
        }
        if contracted.contains(name.as_str()) || excluded_names.contains(&name) {
            continue;
        }
        if entry.path().is_file() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// List the store's top-level entries that match the exclusion contract.
/// Names only: excluded content is never opened.
fn excluded_locations_for_store(root: &Path) -> Result<Vec<ExcludedLocation>, String> {
    let mut excluded = Vec::new();
    let read_dir =
        fs::read_dir(root).map_err(|e| format!("read source store {}: {e}", root.display()))?;
    for entry in read_dir {
        let entry = entry.map_err(|e| format!("read entry in {}: {e}", root.display()))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| format!("non-UTF-8 entry name in {}", root.display()))?;
        let metadata = fs::symlink_metadata(root.join(&name))
            .map_err(|e| format!("inspect {}/{}: {e}", root.display(), name))?;
        let is_dir = metadata.is_dir();
        if let Some(reason) = exclusion_for_name(&name, is_dir) {
            excluded.push(ExcludedLocation {
                path: root.join(&name).display().to_string(),
                reason: reason.label().into(),
                present: true,
            });
        }
    }
    excluded.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(excluded)
}

/// Archive the Firm-home control-plane registries and markers that drove the
/// store enumeration, so the manifest's store list is auditable offline.
fn archive_control_plane_files(
    firm_home: &Path,
    archive_root: &Path,
    files: &mut Vec<ManifestFile>,
) -> Result<(), String> {
    for (source_relative, archive_relative) in CONTROL_PLANE_REGISTRIES {
        let source = firm_home.join(source_relative);
        if !source.is_file() {
            continue;
        }
        let bytes = fs::read(&source).map_err(|e| format!("read {}: {e}", source.display()))?;
        // Registries are JSON documents, not JSONL ledgers; record rows as
        // absent and keep byte/line accounting exact.
        write_archive_file(archive_root, archive_relative, &bytes)?;
        files.push(ManifestFile {
            path: (*archive_relative).into(),
            category: "control_plane_registry".into(),
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            line_count: physical_line_count(&bytes),
            rows: None,
            source_path: Some(source.display().to_string()),
        });
    }
    for marker in CONTROL_PLANE_MARKERS {
        let source = firm_home.join(marker);
        if !source.is_file() {
            continue;
        }
        let bytes = fs::read(&source).map_err(|e| format!("read {}: {e}", source.display()))?;
        let archive_relative = format!("markers/{marker}");
        write_archive_file(archive_root, &archive_relative, &bytes)?;
        files.push(ManifestFile {
            path: archive_relative,
            category: "control_plane_marker".into(),
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            line_count: physical_line_count(&bytes),
            rows: None,
            source_path: Some(source.display().to_string()),
        });
    }
    Ok(())
}

/// Mutation-detection snapshot over exactly what the export reads: the
/// contracted ledger bytes and control-plane files, plus each present store's
/// sorted top-level entry names (so a store gaining or losing entries
/// mid-export is caught even when the changed entry is not a contracted
/// ledger). Excluded locations are never opened.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InputSnapshot {
    ledger_files: Vec<(PathBuf, u64, String)>,
    control_plane_files: Vec<(PathBuf, u64, String)>,
    store_entry_names: Vec<(PathBuf, Vec<String>)>,
}

fn snapshot_inputs(stores: &[SourceStore], firm_home: &Path) -> Result<InputSnapshot, String> {
    let mut ledger_files = Vec::new();
    let mut store_entry_names = Vec::new();
    for store in stores {
        if !store.present {
            continue;
        }
        let mut names = Vec::new();
        let read_dir = fs::read_dir(&store.root)
            .map_err(|e| format!("snapshot read store {}: {e}", store.root.display()))?;
        for entry in read_dir {
            let entry = entry
                .map_err(|e| format!("snapshot read entry in {}: {e}", store.root.display()))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| format!("non-UTF-8 entry name in {}", store.root.display()))?;
            names.push(name);
        }
        names.sort();
        store_entry_names.push((store.root.clone(), names));
        for contract in LEDGER_CONTRACT {
            let path = store.root.join(contract.ledger);
            if !path.is_file() {
                continue;
            }
            let bytes =
                fs::read(&path).map_err(|e| format!("snapshot read {}: {e}", path.display()))?;
            ledger_files.push((path, bytes.len() as u64, sha256_hex(&bytes)));
        }
    }
    ledger_files.sort();
    let mut control_plane_files = Vec::new();
    for (relative, _) in CONTROL_PLANE_REGISTRIES {
        let path = firm_home.join(relative);
        if path.is_file() {
            let bytes =
                fs::read(&path).map_err(|e| format!("snapshot read {}: {e}", path.display()))?;
            control_plane_files.push((path, bytes.len() as u64, sha256_hex(&bytes)));
        }
    }
    for marker in CONTROL_PLANE_MARKERS {
        let path = firm_home.join(marker);
        if path.is_file() {
            let bytes =
                fs::read(&path).map_err(|e| format!("snapshot read {}: {e}", path.display()))?;
            control_plane_files.push((path, bytes.len() as u64, sha256_hex(&bytes)));
        }
    }
    control_plane_files.sort();
    Ok(InputSnapshot {
        ledger_files,
        control_plane_files,
        store_entry_names,
    })
}

fn ensure_inputs_unchanged(
    before: &InputSnapshot,
    stores: &[SourceStore],
    firm_home: &Path,
) -> Result<(), String> {
    let after = snapshot_inputs(stores, firm_home)?;
    if &after != before {
        return Err("source stores changed during export; refusing mixed-moment archive".into());
    }
    Ok(())
}

fn reject_output_inside_sources(
    firm_home: &Path,
    stores: &[SourceStore],
    output: &Path,
) -> Result<(), String> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = resolve_with_existing_ancestor(parent)?;
    let home = fs::canonicalize(firm_home)
        .map_err(|e| format!("canonicalize Firm home {}: {e}", firm_home.display()))?;
    if parent.starts_with(&home) {
        return Err(format!(
            "archive destination must be outside the Firm home: {}",
            output.display()
        ));
    }
    // Repo-local compatibility stores live outside the Firm home, so they
    // need their own containment check.
    for store in stores {
        if !store.present {
            continue;
        }
        let root = fs::canonicalize(&store.root)
            .map_err(|e| format!("canonicalize source store {}: {e}", store.root.display()))?;
        if parent.starts_with(&root) {
            return Err(format!(
                "archive destination must be outside every enumerated source store: {}",
                output.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "legacy-company-os-test-{tag}-{}-{nanos}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp root");
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_json(path: &Path, value: serde_json::Value) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create metadata parent");
        }
        fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).expect("write metadata");
    }

    /// Seed every store kind under one temp Firm home.
    fn seed_home(root: &Path) -> (PathBuf, PathBuf) {
        let home = root.join("firm-home");
        write_json(
            &home.join("companies/acme/metadata.json"),
            serde_json::json!({"company_id": "acme", "name": "Acme"}),
        );
        write_json(
            &home.join("execution-spaces/s1/metadata.json"),
            serde_json::json!({"space_id": "s1", "name": "S1"}),
        );
        let repo = root.join("repo-p1");
        fs::create_dir_all(repo.join(".harness")).expect("repo-local store");
        fs::write(repo.join(".harness/goals.jsonl"), b"{\"id\":\"g1\"}\n").expect("seed ledger");
        write_json(
            &home.join("projects/p1/metadata.json"),
            serde_json::json!({
                "project_id": "p1",
                "canonical_path": repo,
                "kind": "repo",
                "is_git_repo": false,
            }),
        );
        fs::create_dir_all(home.join("nodes/node-1")).expect("node store");
        (home, repo)
    }

    #[test]
    fn enumerates_all_five_store_kinds_sorted() {
        let root = TempRoot::new("enum-all");
        let (home, _repo) = seed_home(&root.0);
        let stores = enumerate_stores(&home).expect("enumerate");
        let ids: Vec<&str> = stores.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "company-acme",
                "node-node-1",
                "project-_global",
                "project-p1",
                "repo-local-p1",
                "space-s1"
            ]
        );
        let global = stores.iter().find(|s| s.id == "project-_global").unwrap();
        assert!(!global.present, "_global store was never materialized");
        let company = stores.iter().find(|s| s.id == "company-acme").unwrap();
        assert!(company.present);
        assert_eq!(company.identity.as_ref().unwrap()["company_id"], "acme");
    }

    #[test]
    fn registry_and_scan_dedup_by_canonical_path() {
        let root = TempRoot::new("enum-dedup");
        let (home, repo) = seed_home(&root.0);
        // Registry entries pointing at the same on-disk stores the scans find.
        write_json(
            &home.join("companies/registry.json"),
            serde_json::json!({
                "format_version": 1,
                "current_company_id": "acme",
                "companies": [{
                    "id": "acme",
                    "name": "Acme",
                    "store_root": home.join("companies/acme"),
                }],
            }),
        );
        write_json(
            &home.join("execution-spaces/registry.json"),
            serde_json::json!({
                "format_version": 1,
                "current_space_id": "s1",
                "spaces": [{
                    "id": "s1",
                    "name": "S1",
                    "store_root": home.join("execution-spaces/s1"),
                }],
            }),
        );
        write_json(
            &home.join("projects/registry.json"),
            serde_json::json!({
                "format_version": 1,
                "current_project_id": "p1",
                "projects": [{
                    "id": "p1",
                    "path": repo,
                    "store_root": home.join("projects/p1"),
                    "kind": "repo",
                }],
            }),
        );
        let stores = enumerate_stores(&home).expect("enumerate");
        let ids: Vec<&str> = stores.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "company-acme",
                "node-node-1",
                "project-_global",
                "project-p1",
                "repo-local-p1",
                "space-s1"
            ],
            "registry + scan must not double-enumerate one physical store"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_enumerated_store_fails_closed() {
        let root = TempRoot::new("enum-symlink");
        let (home, _repo) = seed_home(&root.0);
        std::os::unix::fs::symlink(home.join("nodes/node-1"), home.join("nodes/node-alias"))
            .expect("symlink");
        let error = enumerate_stores(&home).expect_err("symlink store must fail");
        assert!(error.contains("must not be a symlink"), "{error}");
    }

    #[test]
    fn unsafe_source_id_is_rejected() {
        let root = TempRoot::new("enum-unsafe-id");
        let home = root.0.join("firm-home");
        write_json(
            &home.join("companies/bad%2Fid/metadata.json"),
            serde_json::json!({"company_id": "bad/id", "name": "Bad"}),
        );
        let error = enumerate_stores(&home).expect_err("unsafe id must fail");
        assert!(error.contains("unsafe archive store source id"), "{error}");
    }

    #[test]
    fn exclusion_contract_matches_names_not_content() {
        // Directory rules.
        assert_eq!(
            exclusion_for_name("provider-sessions", true),
            Some(ExclusionReason::ProviderNativeTranscript)
        );
        assert_eq!(
            exclusion_for_name("runtimes", true),
            Some(ExclusionReason::ProviderNativeRuntimeState)
        );
        // A file named like an excluded directory is not excluded by it.
        assert_eq!(exclusion_for_name("provider-sessions", false), None);
        // Secret files: exact and pattern-based.
        assert_eq!(
            exclusion_for_name(".env", false),
            Some(ExclusionReason::SecretFile)
        );
        assert_eq!(
            exclusion_for_name(".env.local", false),
            Some(ExclusionReason::SecretFile)
        );
        assert_eq!(
            exclusion_for_name("node.key", false),
            Some(ExclusionReason::SecretFile)
        );
        assert_eq!(
            exclusion_for_name("api.token", false),
            Some(ExclusionReason::SecretFile)
        );
        assert_eq!(
            exclusion_for_name("cert.pem", false),
            Some(ExclusionReason::SecretFile)
        );
        // IPC / locks.
        assert_eq!(
            exclusion_for_name("daemon.sock", false),
            Some(ExclusionReason::EphemeralIpcOrLock)
        );
        assert_eq!(
            exclusion_for_name("node-fabric.lock", false),
            Some(ExclusionReason::EphemeralIpcOrLock)
        );
        // Contract ledgers and ordinary current ledgers are never excluded.
        assert_eq!(
            exclusion_for_name("company_os_documents.jsonl", false),
            None
        );
        assert_eq!(exclusion_for_name("missions.jsonl", false), None);
        assert_eq!(exclusion_for_name("work_operations.jsonl", false), None);
        assert_eq!(exclusion_for_name("metadata.json", false), None);
    }

    /// Seed a store with contract ledgers, excluded locations, and a
    /// non-contract non-excluded ledger.
    fn seed_company_os_surface(home: &Path) {
        let store = home.join("companies/acme");
        fs::write(
            store.join("company_os_documents.jsonl"),
            b"{\"id\":\"doc-1\",\"title\":\"One\"}\n{\"id\":\"doc-2\",\"title\":\"Two\"}\n",
        )
        .expect("documents");
        fs::write(
            store.join("company_os_blocks.jsonl"),
            b"{\"id\":\"blk-1\",\"document_id\":\"doc-1\"}\n",
        )
        .expect("blocks");
        fs::write(
            store.join("missions.jsonl"),
            b"{\"id\":\"mission-1\",\"title\":\"M\"}\n",
        )
        .expect("missions");
        fs::write(
            store.join("team_messages.jsonl"),
            b"{\"id\":\"tm-1\",\"body\":\"hi\"}\n",
        )
        .expect("team messages");
        // Present-but-excluded locations (content must never be archived).
        fs::create_dir_all(store.join("provider-sessions/session-1")).expect("provider-sessions");
        fs::write(
            store.join("provider-sessions/session-1/codex.stream-json.ndjson"),
            b"{\"type\":\"transcript\"}\n",
        )
        .expect("transcript");
        fs::create_dir_all(store.join("runtimes/runtime-1")).expect("runtimes");
        fs::write(store.join(".env"), b"TOKEN=never-exported\n").expect(".env");
        fs::write(store.join("tokens.json"), b"{\"api\":\"never-exported\"}\n").expect("tokens");
        // Non-contract, non-excluded current ledger: neither archived nor
        // listed as excluded.
        fs::write(store.join("teams.jsonl"), b"{\"id\":\"team-1\"}\n").expect("teams");
    }

    #[test]
    fn export_archives_contract_preserves_bytes_and_excludes_secrets() {
        let root = TempRoot::new("export-surface");
        let (home, _repo) = seed_home(&root.0);
        seed_company_os_surface(&home);
        fs::write(home.join("NODE_ID"), b"node-1\n").expect("node id");
        let output = root.0.join("archive-out");

        let summary = export_archive(&home, &output).expect("export");
        assert_eq!(summary.format, "legacy-company-os-v1");
        assert_eq!(summary.source_revision.len(), 40);
        assert!(summary.stores >= 5, "seeded kinds: {}", summary.stores);

        // Byte-exact preservation of a seeded ledger.
        assert_eq!(
            fs::read(output.join("stores/company-acme/ledgers/company_os_documents.jsonl"))
                .expect("archived documents"),
            b"{\"id\":\"doc-1\",\"title\":\"One\"}\n{\"id\":\"doc-2\",\"title\":\"Two\"}\n"
        );
        // Absent contract ledgers are still enumerated as empty files.
        assert_eq!(
            fs::read(output.join("stores/company-acme/ledgers/company_os_views.jsonl"))
                .expect("archived empty views"),
            b""
        );

        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("manifest.json")).expect("manifest"))
                .expect("manifest json");
        let stores = manifest["stores"].as_array().unwrap();
        let company = stores
            .iter()
            .find(|s| s["id"] == "company-acme")
            .expect("company store");
        let ledgers = company["ledgers"].as_array().unwrap();
        assert_eq!(ledgers.len(), LEDGER_CONTRACT.len());
        let documents = ledgers
            .iter()
            .find(|l| l["ledger"] == "company_os_documents.jsonl")
            .unwrap();
        assert_eq!(documents["present"], true);
        assert_eq!(documents["rows"], 2);
        assert_eq!(documents["section"], "company_os");
        assert_eq!(documents["object_type"], "company_os_document");
        assert_eq!(documents["schema_version"], "company-os-ledger-v1");
        assert!(documents["source_path"].as_str().unwrap().starts_with('/'));
        let waves = ledgers
            .iter()
            .find(|l| l["ledger"] == "waves.jsonl")
            .unwrap();
        assert_eq!(waves["present"], false);
        assert_eq!(waves["rows"], 0);
        let team_messages = ledgers
            .iter()
            .find(|l| l["ledger"] == "team_messages.jsonl")
            .unwrap();
        assert_eq!(team_messages["section"], "retired_history");
        assert_eq!(team_messages["rows"], 1);

        // Excluded locations are listed with reasons; their content is absent.
        let excluded = company["excluded_locations"].as_array().unwrap();
        let reasons: std::collections::BTreeMap<&str, &str> = excluded
            .iter()
            .map(|e| {
                (
                    e["path"].as_str().unwrap().rsplit('/').next().unwrap(),
                    e["reason"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(reasons["provider-sessions"], "provider_native_transcript");
        assert_eq!(reasons["runtimes"], "provider_native_runtime_state");
        assert_eq!(reasons[".env"], "secret_file");
        assert_eq!(reasons["tokens.json"], "secret_file");
        let archived_text = fs::read_to_string(output.join("manifest.json")).unwrap();
        assert!(!archived_text.contains("never-exported"));
        assert!(!output
            .join("stores/company-acme/ledgers/teams.jsonl")
            .exists());
        assert!(!output
            .join("stores/company-acme/provider-sessions")
            .exists());

        // Control-plane markers are archived for offline audit.
        assert_eq!(
            fs::read(output.join("markers/NODE_ID")).expect("marker"),
            b"node-1\n"
        );
        // Sources are untouched.
        assert_eq!(
            fs::read(home.join("companies/acme/company_os_documents.jsonl")).unwrap(),
            b"{\"id\":\"doc-1\",\"title\":\"One\"}\n{\"id\":\"doc-2\",\"title\":\"Two\"}\n"
        );
        // Skeleton verify accepts the archive (full contract checks land
        // next). Canonicalize first: the verifier refuses symlinked
        // ancestors, and the macOS temp root lives under /var -> /private/var.
        let canonical_output = fs::canonicalize(&output).expect("canonical archive path");
        verify_archive(&canonical_output).expect("verify skeleton");
    }

    #[test]
    fn export_fails_when_source_moves_mid_export() {
        let root = TempRoot::new("export-drift");
        let (home, _repo) = seed_home(&root.0);
        seed_company_os_surface(&home);
        let stores = enumerate_stores(&home).expect("enumerate");
        let before = snapshot_inputs(&stores, &home).expect("snapshot");
        fs::write(
            home.join("companies/acme/company_os_documents.jsonl"),
            b"{\"id\":\"doc-3\"}\n",
        )
        .expect("drift");
        let error = ensure_inputs_unchanged(&before, &stores, &home)
            .expect_err("drift must fail the export");
        assert!(error.contains("refusing mixed-moment archive"), "{error}");
    }

    /// Export the seeded home to a fresh archive dir and return its
    /// canonical path (verify refuses symlinked temp ancestors).
    fn export_seeded(root: &Path, home: &Path, name: &str) -> PathBuf {
        let output = root.join(name);
        export_archive(home, &output).expect("export");
        fs::canonicalize(&output).expect("canonical archive")
    }

    fn read_manifest(archive: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(archive.join("manifest.json")).expect("manifest"))
            .expect("manifest json")
    }

    fn write_manifest(archive: &Path, manifest: &serde_json::Value) {
        let mut bytes = serde_json::to_vec_pretty(manifest).unwrap();
        bytes.push(b'\n');
        fs::write(archive.join("manifest.json"), bytes).expect("write manifest");
    }

    #[test]
    fn verify_rejects_tampered_bytes_manifest_and_smuggled_exclusions() {
        let root = TempRoot::new("verify-tamper");
        let (home, _repo) = seed_home(&root.0);
        seed_company_os_surface(&home);

        // 1. Byte tampering in an archived ledger.
        let archive = export_seeded(&root.0, &home, "a1");
        fs::write(
            archive.join("stores/company-acme/ledgers/company_os_documents.jsonl"),
            b"{\"id\":\"doc-x\"}\n",
        )
        .unwrap();
        let error = verify_archive(&archive).expect_err("tampered bytes");
        assert!(error.contains("SHA-256 mismatch"), "{error}");

        // 2. A weakened echoed exclusion contract.
        let archive = export_seeded(&root.0, &home, "a2");
        let mut manifest = read_manifest(&archive);
        manifest["exclusion_contract"]
            .as_array_mut()
            .unwrap()
            .retain(|rule| rule["name"] != "tokens.json");
        write_manifest(&archive, &manifest);
        let error = verify_archive(&archive).expect_err("weakened contract");
        assert!(error.contains("exclusion contract"), "{error}");

        // 3. Doctored row count in a ledger entry (totals no longer
        //    recompute before the restore-read proof even runs).
        let archive = export_seeded(&root.0, &home, "a3");
        let mut manifest = read_manifest(&archive);
        let company = manifest["stores"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|s| s["id"] == "company-acme")
            .unwrap();
        company["ledgers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|l| l["ledger"] == "company_os_documents.jsonl")
            .unwrap()["rows"] = serde_json::json!(99);
        write_manifest(&archive, &manifest);
        let error = verify_archive(&archive).expect_err("doctored rows");
        assert!(
            error.contains("totals do not recompute") || error.contains("manifest mismatch"),
            "{error}"
        );

        // 4. A hand-added file smuggled from an excluded location.
        let archive = export_seeded(&root.0, &home, "a4");
        write_archive_file(
            &archive,
            "stores/company-acme/ledgers/sneaky.jsonl",
            b"{}\n",
        )
        .expect("plant file");
        let mut manifest = read_manifest(&archive);
        let store_path = manifest["stores"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "company-acme")
            .unwrap()["path"]
            .as_str()
            .unwrap()
            .to_string();
        manifest["files"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "path": "stores/company-acme/ledgers/sneaky.jsonl",
                "category": "legacy_ledger",
                "sha256": sha256_hex(b"{}\n"),
                "bytes": 3,
                "line_count": 1,
                "rows": 1,
                "source_path": format!("{store_path}/provider-sessions/sneaky.jsonl"),
            }));
        write_manifest(&archive, &manifest);
        let error = verify_archive(&archive).expect_err("smuggled exclusion");
        assert!(error.contains("excluded location"), "{error}");
    }

    #[test]
    fn restore_read_recovers_rows_from_detached_copy() {
        let root = TempRoot::new("verify-restore");
        let (home, _repo) = seed_home(&root.0);
        seed_company_os_surface(&home);
        let archive = export_seeded(&root.0, &home, "a1");
        let summary = verify_archive(&archive).expect("verify");
        assert_eq!(summary.restore_read, "verified");
        // Seeded rows: acme documents 2 + blocks 1 + missions 1 +
        // team_messages 1, plus repo-local goals.jsonl is NOT in this
        // contract.
        assert_eq!(summary.rows, 5);
        // Deleting the live sources must not affect verification.
        fs::remove_dir_all(home.join("companies/acme")).expect("remove live store");
        let summary = verify_archive(&archive).expect("verify offline");
        assert_eq!(summary.rows, 5);
    }
}
