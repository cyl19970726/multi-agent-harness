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

mod export;
mod verify;

pub use export::export_archive;
pub use verify::verify_archive;

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
mod tests;
