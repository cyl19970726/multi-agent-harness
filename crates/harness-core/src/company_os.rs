//! Canonical Company OS product records.
//!
//! These records are deliberately independent from executor-native Mission,
//! Wave, Agent Team, Workflow, and provider records. In particular, a
//! [`WorkItem`] is not an executor task and no legacy task is converted into
//! one by this module.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CompanyOsValidationError {
    #[error("{field} is required")]
    Required { field: &'static str },
    #[error("{field} is invalid: {reason}")]
    Invalid { field: &'static str, reason: String },
}

pub trait ValidateCompanyOs {
    fn validate(&self) -> Result<(), CompanyOsValidationError>;
}

fn required(value: &str, field: &'static str) -> Result<(), CompanyOsValidationError> {
    if value.trim().is_empty() {
        Err(CompanyOsValidationError::Required { field })
    } else {
        Ok(())
    }
}

fn required_strings(
    values: &[String],
    field: &'static str,
) -> Result<(), CompanyOsValidationError> {
    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(CompanyOsValidationError::Required { field });
    }
    let mut seen = BTreeSet::new();
    if values.iter().any(|value| !seen.insert(value)) {
        return Err(CompanyOsValidationError::Invalid {
            field,
            reason: "must not contain duplicates".into(),
        });
    }
    Ok(())
}

fn required_object(value: &Value, field: &'static str) -> Result<(), CompanyOsValidationError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(CompanyOsValidationError::Invalid {
            field,
            reason: "must be a JSON object".into(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    Human,
    Agent,
    External,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ActorRef {
    pub actor_type: ActorType,
    pub actor_id: String,
}

impl ValidateCompanyOs for ActorRef {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.actor_id, "ActorRef.actor_id")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Active,
    Invited,
    Paused,
    Ended,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredAvailability {
    Available,
    Busy,
    Paused,
    Offline,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanMember {
    pub id: String,
    pub display_name: String,
    pub title: Option<String>,
    pub status: MemberStatus,
    pub availability: Option<DeclaredAvailability>,
    pub membership_refs: Vec<String>,
    pub responsibility_summary: String,
    pub permission_policy_refs: Vec<String>,
    pub authority_policy_refs: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for HumanMember {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "HumanMember.id")?;
        required(&self.display_name, "HumanMember.display_name")?;
        required(
            &self.responsibility_summary,
            "HumanMember.responsibility_summary",
        )?;
        required_strings(&self.membership_refs, "HumanMember.membership_refs")?;
        required_strings(
            &self.permission_policy_refs,
            "HumanMember.permission_policy_refs",
        )?;
        required_strings(
            &self.authority_policy_refs,
            "HumanMember.authority_policy_refs",
        )?;
        required(&self.created_at, "HumanMember.created_at")?;
        required(&self.updated_at, "HumanMember.updated_at")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandingAgent {
    pub id: String,
    pub display_name: String,
    pub role: String,
    pub status: MemberStatus,
    /// A business declaration. It must not be inferred from runtime health.
    pub availability: DeclaredAvailability,
    pub assignment_capacity: Option<u32>,
    pub exclusive_assignment_ref: Option<String>,
    pub membership_refs: Vec<String>,
    pub responsibility_summary: String,
    pub capability_refs: Vec<String>,
    pub permission_policy_refs: Vec<String>,
    pub runtime_refs: Vec<String>,
    pub provider_session_refs: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for StandingAgent {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "StandingAgent.id")?;
        required(&self.display_name, "StandingAgent.display_name")?;
        required(&self.role, "StandingAgent.role")?;
        required(
            &self.responsibility_summary,
            "StandingAgent.responsibility_summary",
        )?;
        required_strings(&self.membership_refs, "StandingAgent.membership_refs")?;
        required_strings(&self.capability_refs, "StandingAgent.capability_refs")?;
        required_strings(
            &self.permission_policy_refs,
            "StandingAgent.permission_policy_refs",
        )?;
        required_strings(&self.runtime_refs, "StandingAgent.runtime_refs")?;
        required_strings(
            &self.provider_session_refs,
            "StandingAgent.provider_session_refs",
        )?;
        if matches!(self.assignment_capacity, Some(0)) {
            return Err(CompanyOsValidationError::Invalid {
                field: "StandingAgent.assignment_capacity",
                reason: "must be positive when declared".into(),
            });
        }
        required(&self.created_at, "StandingAgent.created_at")?;
        required(&self.updated_at, "StandingAgent.updated_at")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalParticipant {
    pub id: String,
    pub display_name_or_organization: String,
    pub engagement_scope: String,
    pub sponsor_actor_ref: ActorRef,
    pub access_expires_at: String,
    pub confidentiality_or_contract_refs: Vec<String>,
    pub membership_refs: Vec<String>,
    pub restricted_permission_refs: Vec<String>,
    pub status: MemberStatus,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for ExternalParticipant {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "ExternalParticipant.id")?;
        required(
            &self.display_name_or_organization,
            "ExternalParticipant.display_name_or_organization",
        )?;
        required(
            &self.engagement_scope,
            "ExternalParticipant.engagement_scope",
        )?;
        self.sponsor_actor_ref.validate()?;
        if !matches!(
            self.sponsor_actor_ref.actor_type,
            ActorType::Human | ActorType::Agent
        ) {
            return Err(CompanyOsValidationError::Invalid {
                field: "ExternalParticipant.sponsor_actor_ref",
                reason: "an external participant requires an internal sponsor".into(),
            });
        }
        required(
            &self.access_expires_at,
            "ExternalParticipant.access_expires_at",
        )?;
        required_strings(
            &self.confidentiality_or_contract_refs,
            "ExternalParticipant.confidentiality_or_contract_refs",
        )?;
        required_strings(&self.membership_refs, "ExternalParticipant.membership_refs")?;
        required_strings(
            &self.restricted_permission_refs,
            "ExternalParticipant.restricted_permission_refs",
        )?;
        required(&self.created_at, "ExternalParticipant.created_at")?;
        required(&self.updated_at, "ExternalParticipant.updated_at")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceActor {
    pub id: String,
    pub display_name: String,
    pub service_kind: String,
    pub owner_actor_ref: ActorRef,
    pub credential_boundary: String,
    pub permission_policy_refs: Vec<String>,
    pub audit_policy_ref: String,
    pub status: MemberStatus,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for ServiceActor {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "ServiceActor.id")?;
        required(&self.display_name, "ServiceActor.display_name")?;
        required(&self.service_kind, "ServiceActor.service_kind")?;
        self.owner_actor_ref.validate()?;
        if !matches!(
            self.owner_actor_ref.actor_type,
            ActorType::Human | ActorType::Agent
        ) {
            return Err(CompanyOsValidationError::Invalid {
                field: "ServiceActor.owner_actor_ref",
                reason: "a service requires a human or agent owner".into(),
            });
        }
        required(
            &self.credential_boundary,
            "ServiceActor.credential_boundary",
        )?;
        required_strings(
            &self.permission_policy_refs,
            "ServiceActor.permission_policy_refs",
        )?;
        required(&self.audit_policy_ref, "ServiceActor.audit_policy_ref")?;
        required(&self.created_at, "ServiceActor.created_at")?;
        required(&self.updated_at, "ServiceActor.updated_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrgUnitStatus {
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgUnit {
    pub id: String,
    pub organization_id: String,
    pub name: String,
    pub purpose: String,
    pub parent_unit_id: Option<String>,
    pub status: OrgUnitStatus,
    pub human_lead_actor_ref: Option<ActorRef>,
    pub agent_lead_actor_ref: Option<ActorRef>,
    pub policy_refs: Vec<String>,
    pub document_space_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for OrgUnit {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "OrgUnit.id")?;
        required(&self.organization_id, "OrgUnit.organization_id")?;
        required(&self.name, "OrgUnit.name")?;
        required(&self.purpose, "OrgUnit.purpose")?;
        if let Some(lead) = &self.human_lead_actor_ref {
            lead.validate()?;
            if lead.actor_type != ActorType::Human {
                return Err(CompanyOsValidationError::Invalid {
                    field: "OrgUnit.human_lead_actor_ref",
                    reason: "must reference a human actor".into(),
                });
            }
        }
        if let Some(lead) = &self.agent_lead_actor_ref {
            lead.validate()?;
            if lead.actor_type != ActorType::Agent {
                return Err(CompanyOsValidationError::Invalid {
                    field: "OrgUnit.agent_lead_actor_ref",
                    reason: "must reference an agent actor".into(),
                });
            }
        }
        required_strings(&self.policy_refs, "OrgUnit.policy_refs")?;
        required(&self.created_at, "OrgUnit.created_at")?;
        required(&self.updated_at, "OrgUnit.updated_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationMembershipRole {
    Lead,
    Member,
    Advisor,
    Observer,
    ExternalPartner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationMembershipStatus {
    Active,
    Invited,
    Paused,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationMembership {
    pub id: String,
    pub organization_id: String,
    pub org_unit_id: String,
    pub actor_ref: ActorRef,
    pub membership_role: OrganizationMembershipRole,
    pub title_or_function: Option<String>,
    pub status: OrganizationMembershipStatus,
    pub starts_at: String,
    pub ends_at: Option<String>,
    pub authority_policy_refs: Vec<String>,
    pub created_by_actor_ref: ActorRef,
    pub created_at: String,
}

impl ValidateCompanyOs for OrganizationMembership {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "OrganizationMembership.id")?;
        required(
            &self.organization_id,
            "OrganizationMembership.organization_id",
        )?;
        required(&self.org_unit_id, "OrganizationMembership.org_unit_id")?;
        self.actor_ref.validate()?;
        if self.membership_role == OrganizationMembershipRole::Lead
            && !matches!(
                self.actor_ref.actor_type,
                ActorType::Human | ActorType::Agent
            )
        {
            return Err(CompanyOsValidationError::Invalid {
                field: "OrganizationMembership.actor_ref",
                reason: "external and service actors cannot be organization leads".into(),
            });
        }
        required(&self.starts_at, "OrganizationMembership.starts_at")?;
        required_strings(
            &self.authority_policy_refs,
            "OrganizationMembership.authority_policy_refs",
        )?;
        self.created_by_actor_ref.validate()?;
        required(&self.created_at, "OrganizationMembership.created_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Actor,
    Document,
    TypedRecord,
    BusinessModule,
    Milestone,
    WorkItem,
    Approval,
    FinancialRecord,
    Evidence,
    Execution,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityRef {
    pub kind: EntityKind,
    pub id: String,
}

impl ValidateCompanyOs for EntityRef {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "EntityRef.id")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Page,
    Database,
    Dashboard,
    Template,
    Policy,
    Record,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleStatus {
    Draft,
    Active,
    Paused,
    Completed,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub space_id: String,
    pub parent_document_id: Option<String>,
    pub title: String,
    pub kind: DocumentKind,
    pub lifecycle_status: LifecycleStatus,
    pub block_ids: Vec<String>,
    pub template_ref: Option<String>,
    pub permission_policy_refs: Vec<String>,
    pub reference_refs: Vec<EntityRef>,
    pub created_by: ActorRef,
    pub updated_by: ActorRef,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for Document {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "Document.id")?;
        required(&self.space_id, "Document.space_id")?;
        required(&self.title, "Document.title")?;
        required_strings(&self.block_ids, "Document.block_ids")?;
        required_strings(
            &self.permission_policy_refs,
            "Document.permission_policy_refs",
        )?;
        self.created_by.validate()?;
        self.updated_by.validate()?;
        for reference in &self.reference_refs {
            reference.validate()?;
        }
        required(&self.created_at, "Document.created_at")?;
        required(&self.updated_at, "Document.updated_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    RichText,
    Heading,
    List,
    Checklist,
    Callout,
    Code,
    Media,
    Attachment,
    SimpleTable,
    Comment,
    Mention,
    EmbeddedView,
    Metric,
    Decision,
    WorkItem,
    RelationSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    pub document_id: String,
    pub kind: BlockKind,
    pub position: u32,
    pub content: Value,
    pub referenced_entities: Vec<EntityRef>,
    pub created_by: ActorRef,
    pub updated_by: ActorRef,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for Block {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "Block.id")?;
        required(&self.document_id, "Block.document_id")?;
        required_object(&self.content, "Block.content")?;
        for reference in &self.referenced_entities {
            reference.validate()?;
        }
        self.created_by.validate()?;
        self.updated_by.validate()?;
        required(&self.created_at, "Block.created_at")?;
        required(&self.updated_at, "Block.updated_at")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedRecord {
    pub id: String,
    pub module_id: String,
    pub record_type: String,
    pub title: String,
    pub fields: Value,
    pub lifecycle_status: String,
    pub source_document_ref: Option<String>,
    pub created_by: ActorRef,
    pub updated_by: ActorRef,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for TypedRecord {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "TypedRecord.id")?;
        required(&self.module_id, "TypedRecord.module_id")?;
        required(&self.record_type, "TypedRecord.record_type")?;
        required(&self.title, "TypedRecord.title")?;
        required_object(&self.fields, "TypedRecord.fields")?;
        required(&self.lifecycle_status, "TypedRecord.lifecycle_status")?;
        self.created_by.validate()?;
        self.updated_by.validate()?;
        required(&self.created_at, "TypedRecord.created_at")?;
        required(&self.updated_at, "TypedRecord.updated_at")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    pub id: String,
    pub from_ref: EntityRef,
    pub relation_type: String,
    pub to_ref: EntityRef,
    pub provenance_ref: Option<EntityRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_status: Option<String>,
    pub created_by: ActorRef,
    pub created_at: String,
}

impl ValidateCompanyOs for Relation {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "Relation.id")?;
        self.from_ref.validate()?;
        self.to_ref.validate()?;
        required(&self.relation_type, "Relation.relation_type")?;
        if self.from_ref == self.to_ref {
            return Err(CompanyOsValidationError::Invalid {
                field: "Relation.to_ref",
                reason: "a relation cannot link an entity to itself".into(),
            });
        }
        if let Some(provenance) = &self.provenance_ref {
            provenance.validate()?;
        }
        self.created_by.validate()?;
        required(&self.created_at, "Relation.created_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    Table,
    Board,
    Timeline,
    Calendar,
    Chart,
    Dashboard,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub id: String,
    pub module_id: Option<String>,
    pub title: String,
    pub mode: ViewMode,
    pub source_kinds: Vec<EntityKind>,
    pub query: Value,
    pub owner: ActorRef,
    pub policy_refs: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for View {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "View.id")?;
        required(&self.title, "View.title")?;
        if self.source_kinds.is_empty() {
            return Err(CompanyOsValidationError::Required {
                field: "View.source_kinds",
            });
        }
        required_object(&self.query, "View.query")?;
        self.owner.validate()?;
        required_strings(&self.policy_refs, "View.policy_refs")?;
        required(&self.created_at, "View.created_at")?;
        required(&self.updated_at, "View.updated_at")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationRule {
    pub relation_type: String,
    pub from_kind: EntityKind,
    pub to_kind: EntityKind,
    pub required: bool,
    pub cross_module: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessModule {
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub root_document_ref: String,
    pub record_types: Vec<String>,
    /// The explicit declaration may be empty, but must be present on the wire.
    pub relation_rules: Vec<RelationRule>,
    pub default_view_refs: Vec<String>,
    pub policy_refs: Vec<String>,
    pub lifecycle_rules: Vec<String>,
    pub metric_definition_refs: Vec<String>,
    pub custom_page_definition_refs: Vec<String>,
    pub status: LifecycleStatus,
    pub owner: ActorRef,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for BusinessModule {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "BusinessModule.id")?;
        required(&self.name, "BusinessModule.name")?;
        required(&self.purpose, "BusinessModule.purpose")?;
        required(&self.root_document_ref, "BusinessModule.root_document_ref")?;
        required_strings(&self.record_types, "BusinessModule.record_types")?;
        for rule in &self.relation_rules {
            required(
                &rule.relation_type,
                "BusinessModule.relation_rules.relation_type",
            )?;
        }
        required_strings(&self.default_view_refs, "BusinessModule.default_view_refs")?;
        required_strings(&self.policy_refs, "BusinessModule.policy_refs")?;
        required_strings(&self.lifecycle_rules, "BusinessModule.lifecycle_rules")?;
        required_strings(
            &self.metric_definition_refs,
            "BusinessModule.metric_definition_refs",
        )?;
        required_strings(
            &self.custom_page_definition_refs,
            "BusinessModule.custom_page_definition_refs",
        )?;
        self.owner.validate()?;
        required(&self.created_at, "BusinessModule.created_at")?;
        required(&self.updated_at, "BusinessModule.updated_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus {
    Draft,
    Submitted,
    Triaged,
    Accepted,
    InProgress,
    WaitingForApproval,
    Blocked,
    InReview,
    Completed,
    Cancelled,
    Archived,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkType {
    Development,
    Design,
    Research,
    Content,
    Legal,
    Procurement,
    Finance,
    Operations,
    Governance,
    HumanAction,
    #[default]
    General,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneStatus {
    Planned,
    Active,
    AtRisk,
    Achieved,
    Cancelled,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Milestone {
    pub id: String,
    pub title: String,
    pub outcome: String,
    pub status: MilestoneStatus,
    pub accountable_owner: ActorRef,
    #[serde(default)]
    pub source_document_ref: Option<String>,
    #[serde(default)]
    pub business_module_ref: Option<String>,
    #[serde(default)]
    pub target_at: Option<String>,
    pub acceptance_criteria: Vec<String>,
    pub work_item_refs: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub achieved_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkQuery {
    #[serde(default)]
    pub statuses: Vec<WorkItemStatus>,
    #[serde(default)]
    pub work_types: Vec<WorkType>,
    #[serde(default)]
    pub business_module_refs: Vec<String>,
    #[serde(default)]
    pub milestone_refs: Vec<String>,
    #[serde(default)]
    pub accountable_owner: Option<ActorRef>,
    #[serde(default)]
    pub assignee: Option<ActorRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSummary {
    pub total: u64,
    pub active: u64,
    pub completed: u64,
    pub blocked: u64,
    pub waiting_for_approval: u64,
    pub unassigned: u64,
    pub without_milestone: u64,
    pub without_business_line: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneProgress {
    pub milestone: Milestone,
    pub total_work_items: u64,
    pub completed_work_items: u64,
    pub blocked_work_items: u64,
    pub waiting_for_approval_work_items: u64,
    pub progress_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorWorkload {
    pub actor: ActorRef,
    pub accountable_count: u64,
    pub assigned_count: u64,
    pub active_count: u64,
    pub work_item_refs: Vec<String>,
}

/// One derived read model shared by Overview, Board, All Work, Milestones,
/// Timeline, and Workload. It owns no facts; every id points back to a native
/// WorkItem or Milestone row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkProjection {
    pub query: WorkQuery,
    pub summary: WorkSummary,
    pub work_items: Vec<WorkItem>,
    pub milestones: Vec<MilestoneProgress>,
    pub board: std::collections::BTreeMap<String, Vec<String>>,
    pub business_lines: std::collections::BTreeMap<String, Vec<String>>,
    pub work_types: std::collections::BTreeMap<String, Vec<String>>,
    pub workload: Vec<ActorWorkload>,
}

impl ValidateCompanyOs for Milestone {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "Milestone.id")?;
        required(&self.title, "Milestone.title")?;
        required(&self.outcome, "Milestone.outcome")?;
        self.accountable_owner.validate()?;
        if let Some(reference) = &self.source_document_ref {
            required(reference, "Milestone.source_document_ref")?;
        }
        if let Some(reference) = &self.business_module_ref {
            required(reference, "Milestone.business_module_ref")?;
        }
        if let Some(target_at) = &self.target_at {
            required(target_at, "Milestone.target_at")?;
        }
        required_strings(&self.acceptance_criteria, "Milestone.acceptance_criteria")?;
        required_strings(&self.work_item_refs, "Milestone.work_item_refs")?;
        if self.status == MilestoneStatus::Achieved
            && self
                .achieved_at
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            return Err(CompanyOsValidationError::Required {
                field: "Milestone.achieved_at",
            });
        }
        required(&self.created_at, "Milestone.created_at")?;
        required(&self.updated_at, "Milestone.updated_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Direct,
    MissionWave,
    AgentTeam,
    DynamicWorkflow,
    Host,
    External,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionKind {
    DirectHumanWork,
    StandingAgentWork,
    ExternalEngagement,
    Mission,
    Wave,
    AgentTeamRun,
    MemberRun,
    WorkflowRun,
    WorkflowStep,
    HostExecution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionRef {
    pub kind: ExecutionKind,
    pub reference: String,
    pub role_in_execution: Option<String>,
    pub status: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: String,
    pub title: String,
    pub objective: String,
    pub status: WorkItemStatus,
    pub source_document_ref: String,
    pub source_record_refs: Vec<String>,
    /// Optional company checkpoint. This is never a Mission Wave.
    #[serde(default)]
    pub milestone_ref: Option<String>,
    /// Stable company-work classification used by every Work projection.
    #[serde(default)]
    pub work_type: WorkType,
    /// Explicit business-line relation. Absence means unclassified, not inferred.
    #[serde(default)]
    pub business_module_ref: Option<String>,
    pub result_document_ref: Option<String>,
    pub result_record_refs: Vec<String>,
    pub submitted_by: ActorRef,
    pub requested_by: Option<ActorRef>,
    pub accountable_owner: ActorRef,
    pub assignees: Vec<ActorRef>,
    pub contributors: Vec<ActorRef>,
    pub reviewer: Option<ActorRef>,
    pub approver: Option<ActorRef>,
    pub execution_mode: ExecutionMode,
    pub execution_refs: Vec<ExecutionRef>,
    pub approval_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub artifact_refs: Vec<String>,
    pub outcome_summary: Option<String>,
    pub due_at: Option<String>,
    pub priority: Option<String>,
    pub risk_level: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

impl ValidateCompanyOs for WorkItem {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "WorkItem.id")?;
        required(&self.title, "WorkItem.title")?;
        required(&self.objective, "WorkItem.objective")?;
        required(&self.source_document_ref, "WorkItem.source_document_ref")?;
        if let Some(reference) = &self.milestone_ref {
            required(reference, "WorkItem.milestone_ref")?;
        }
        if let Some(reference) = &self.business_module_ref {
            required(reference, "WorkItem.business_module_ref")?;
        }
        self.submitted_by.validate()?;
        if let Some(requester) = &self.requested_by {
            requester.validate()?;
        }
        self.accountable_owner.validate()?;
        for actor in self.assignees.iter().chain(&self.contributors) {
            actor.validate()?;
        }
        if let Some(reviewer) = &self.reviewer {
            reviewer.validate()?;
        }
        if let Some(approver) = &self.approver {
            approver.validate()?;
        }
        for execution in &self.execution_refs {
            required(&execution.reference, "WorkItem.execution_refs.reference")?;
        }
        required_strings(&self.source_record_refs, "WorkItem.source_record_refs")?;
        required_strings(&self.result_record_refs, "WorkItem.result_record_refs")?;
        required_strings(&self.approval_refs, "WorkItem.approval_refs")?;
        required_strings(&self.evidence_refs, "WorkItem.evidence_refs")?;
        required_strings(&self.artifact_refs, "WorkItem.artifact_refs")?;
        if self.status == WorkItemStatus::Completed {
            if self
                .completed_at
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                return Err(CompanyOsValidationError::Required {
                    field: "WorkItem.completed_at",
                });
            }
            if self
                .outcome_summary
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                return Err(CompanyOsValidationError::Required {
                    field: "WorkItem.outcome_summary",
                });
            }
            if self.result_document_ref.is_none() && self.result_record_refs.is_empty() {
                return Err(CompanyOsValidationError::Invalid {
                    field: "WorkItem.result_document_ref",
                    reason: "completed work requires a durable result destination".into(),
                });
            }
        }
        required(&self.created_at, "WorkItem.created_at")?;
        required(&self.updated_at, "WorkItem.updated_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentDeliveryState {
    Pending,
    Delivered,
    Acknowledged,
    Declined,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub id: String,
    pub work_item_id: String,
    pub recipient: ActorRef,
    pub sender: ActorRef,
    pub assigned_role: String,
    pub scope: Option<String>,
    pub delivery_state: AssignmentDeliveryState,
    pub delivery_policy_ref: String,
    pub correlation_id: String,
    pub delivery_evidence_ref: Option<String>,
    pub assigned_at: String,
    pub delivered_at: Option<String>,
    pub acknowledged_at: Option<String>,
}

impl ValidateCompanyOs for Assignment {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "Assignment.id")?;
        required(&self.work_item_id, "Assignment.work_item_id")?;
        self.recipient.validate()?;
        self.sender.validate()?;
        required(&self.assigned_role, "Assignment.assigned_role")?;
        required(&self.delivery_policy_ref, "Assignment.delivery_policy_ref")?;
        required(&self.correlation_id, "Assignment.correlation_id")?;
        required(&self.assigned_at, "Assignment.assigned_at")?;
        if matches!(
            self.delivery_state,
            AssignmentDeliveryState::Delivered | AssignmentDeliveryState::Acknowledged
        ) && self
            .delivered_at
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            return Err(CompanyOsValidationError::Required {
                field: "Assignment.delivered_at",
            });
        }
        if self.delivery_state == AssignmentDeliveryState::Acknowledged
            && self
                .acknowledged_at
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            return Err(CompanyOsValidationError::Required {
                field: "Assignment.acknowledged_at",
            });
        }
        if self.recipient.actor_type == ActorType::Agent
            && matches!(
                self.delivery_state,
                AssignmentDeliveryState::Delivered | AssignmentDeliveryState::Acknowledged
            )
            && self
                .delivery_evidence_ref
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            return Err(CompanyOsValidationError::Required {
                field: "Assignment.delivery_evidence_ref",
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Requested,
    Approved,
    Rejected,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub subject_ref: EntityRef,
    pub action_summary: String,
    pub requested_by: ActorRef,
    pub required_approver_refs: Vec<ActorRef>,
    pub required_actor_type: Option<ActorType>,
    pub policy_ref: String,
    pub status: ApprovalStatus,
    pub decided_by: Vec<ActorRef>,
    pub decision_note: Option<String>,
    pub evidence_refs: Vec<String>,
    pub requested_at: String,
    pub decided_at: Option<String>,
    pub expires_at: Option<String>,
}

impl ValidateCompanyOs for Approval {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "Approval.id")?;
        self.subject_ref.validate()?;
        required(&self.action_summary, "Approval.action_summary")?;
        self.requested_by.validate()?;
        if self.required_approver_refs.is_empty() {
            return Err(CompanyOsValidationError::Required {
                field: "Approval.required_approver_refs",
            });
        }
        for actor in self.required_approver_refs.iter().chain(&self.decided_by) {
            actor.validate()?;
        }
        required(&self.policy_ref, "Approval.policy_ref")?;
        required_strings(&self.evidence_refs, "Approval.evidence_refs")?;
        required(&self.requested_at, "Approval.requested_at")?;
        if matches!(
            self.status,
            ApprovalStatus::Approved | ApprovalStatus::Rejected
        ) {
            if self.decided_by.is_empty() {
                return Err(CompanyOsValidationError::Required {
                    field: "Approval.decided_by",
                });
            }
            if self
                .decided_at
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                return Err(CompanyOsValidationError::Required {
                    field: "Approval.decided_at",
                });
            }
            if self
                .decision_note
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                return Err(CompanyOsValidationError::Required {
                    field: "Approval.decision_note",
                });
            }
        }
        if let Some(required_type) = self.required_actor_type {
            if matches!(self.status, ApprovalStatus::Approved)
                && !self
                    .decided_by
                    .iter()
                    .any(|actor| actor.actor_type == required_type)
            {
                return Err(CompanyOsValidationError::Invalid {
                    field: "Approval.decided_by",
                    reason: "does not satisfy required actor type".into(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    /// Decimal amount as a string so wire values remain exact and currency-safe.
    pub amount: String,
    pub currency: String,
}

impl ValidateCompanyOs for Money {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.amount, "Money.amount")?;
        let mut parts = self.amount.split('.');
        let whole = parts.next().unwrap_or_default();
        let fraction = parts.next();
        let invalid_format = whole.is_empty()
            || !whole.chars().all(|character| character.is_ascii_digit())
            || (whole.len() > 1 && whole.starts_with('0'))
            || fraction.is_some_and(|value| {
                value.is_empty() || !value.chars().all(|character| character.is_ascii_digit())
            })
            || parts.next().is_some();
        if invalid_format {
            return Err(CompanyOsValidationError::Invalid {
                field: "Money.amount",
                reason: "must be an unsigned decimal string".into(),
            });
        }
        if !self
            .amount
            .chars()
            .any(|character| matches!(character, '1'..='9'))
        {
            return Err(CompanyOsValidationError::Invalid {
                field: "Money.amount",
                reason: "must be greater than zero".into(),
            });
        }
        required(&self.currency, "Money.currency")?;
        if self.currency.len() != 3 || !self.currency.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(CompanyOsValidationError::Invalid {
                field: "Money.currency",
                reason: "must be a three-letter uppercase currency code".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitmentStatus {
    Proposed,
    PendingApproval,
    Approved,
    Cancelled,
    Fulfilled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commitment {
    pub id: String,
    pub amount: Money,
    pub status: CommitmentStatus,
    pub source_document_id: String,
    pub submitted_by: ActorRef,
    pub accountable_owner: ActorRef,
    pub relation_ids: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub approval_refs: Vec<String>,
    pub audit_event_ids: Vec<String>,
    pub due_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for Commitment {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "Commitment.id")?;
        self.amount.validate()?;
        required(&self.source_document_id, "Commitment.source_document_id")?;
        self.submitted_by.validate()?;
        self.accountable_owner.validate()?;
        required_strings(&self.relation_ids, "Commitment.relation_ids")?;
        required_strings(&self.evidence_refs, "Commitment.evidence_refs")?;
        required_strings(&self.approval_refs, "Commitment.approval_refs")?;
        required_strings(&self.audit_event_ids, "Commitment.audit_event_ids")?;
        if matches!(
            self.status,
            CommitmentStatus::Approved | CommitmentStatus::Fulfilled
        ) && self.approval_refs.is_empty()
        {
            return Err(CompanyOsValidationError::Required {
                field: "Commitment.approval_refs",
            });
        }
        required(&self.created_at, "Commitment.created_at")?;
        required(&self.updated_at, "Commitment.updated_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    Prepared,
    PendingApproval,
    Processing,
    Settled,
    Failed,
    Reversed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub id: String,
    pub amount: Money,
    pub status: PaymentStatus,
    pub source_document_id: String,
    pub submitted_by: ActorRef,
    pub accountable_owner: ActorRef,
    pub related_commitment_refs: Vec<String>,
    pub relation_ids: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub approval_refs: Vec<String>,
    pub audit_event_ids: Vec<String>,
    pub occurred_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for Payment {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "Payment.id")?;
        self.amount.validate()?;
        required(&self.source_document_id, "Payment.source_document_id")?;
        self.submitted_by.validate()?;
        self.accountable_owner.validate()?;
        required_strings(
            &self.related_commitment_refs,
            "Payment.related_commitment_refs",
        )?;
        required_strings(&self.relation_ids, "Payment.relation_ids")?;
        required_strings(&self.evidence_refs, "Payment.evidence_refs")?;
        required_strings(&self.approval_refs, "Payment.approval_refs")?;
        required_strings(&self.audit_event_ids, "Payment.audit_event_ids")?;
        if matches!(
            self.status,
            PaymentStatus::Processing | PaymentStatus::Settled | PaymentStatus::Reversed
        ) && self.approval_refs.is_empty()
        {
            return Err(CompanyOsValidationError::Required {
                field: "Payment.approval_refs",
            });
        }
        if matches!(
            self.status,
            PaymentStatus::Settled | PaymentStatus::Reversed
        ) {
            if self
                .occurred_at
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                return Err(CompanyOsValidationError::Required {
                    field: "Payment.occurred_at",
                });
            }
            if self.evidence_refs.is_empty() {
                return Err(CompanyOsValidationError::Required {
                    field: "Payment.evidence_refs",
                });
            }
        }
        required(&self.created_at, "Payment.created_at")?;
        required(&self.updated_at, "Payment.updated_at")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataQueryDeclaration {
    pub id: String,
    pub source_kind: EntityKind,
    pub source_scope: String,
    pub permission_policy_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomPageDefinition {
    pub id: String,
    pub module_id: String,
    pub purpose: String,
    pub allowed_data_queries: Vec<DataQueryDeclaration>,
    pub approved_ui_components: Vec<String>,
    pub action_command_refs: Vec<String>,
    pub standard_view_fallback_ref: String,
    pub owner: ActorRef,
    pub package_ref: String,
    pub package_version: String,
    pub fixture_ref: String,
    pub visual_contract_ref: String,
    pub policy_refs: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for CustomPageDefinition {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "CustomPageDefinition.id")?;
        required(&self.module_id, "CustomPageDefinition.module_id")?;
        required(&self.purpose, "CustomPageDefinition.purpose")?;
        if self.allowed_data_queries.is_empty() {
            return Err(CompanyOsValidationError::Required {
                field: "CustomPageDefinition.allowed_data_queries",
            });
        }
        for query in &self.allowed_data_queries {
            required(&query.id, "DataQueryDeclaration.id")?;
            required(&query.source_scope, "DataQueryDeclaration.source_scope")?;
            required(
                &query.permission_policy_ref,
                "DataQueryDeclaration.permission_policy_ref",
            )?;
        }
        if self.approved_ui_components.is_empty() {
            return Err(CompanyOsValidationError::Required {
                field: "CustomPageDefinition.approved_ui_components",
            });
        }
        required_strings(
            &self.approved_ui_components,
            "CustomPageDefinition.approved_ui_components",
        )?;
        required_strings(
            &self.action_command_refs,
            "CustomPageDefinition.action_command_refs",
        )?;
        required(
            &self.standard_view_fallback_ref,
            "CustomPageDefinition.standard_view_fallback_ref",
        )?;
        self.owner.validate()?;
        required(&self.package_ref, "CustomPageDefinition.package_ref")?;
        required(
            &self.package_version,
            "CustomPageDefinition.package_version",
        )?;
        required(&self.fixture_ref, "CustomPageDefinition.fixture_ref")?;
        required(
            &self.visual_contract_ref,
            "CustomPageDefinition.visual_contract_ref",
        )?;
        required_strings(&self.policy_refs, "CustomPageDefinition.policy_refs")?;
        required(&self.created_at, "CustomPageDefinition.created_at")?;
        required(&self.updated_at, "CustomPageDefinition.updated_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomPagePackageKind {
    Html,
    React,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomPagePackage {
    pub id: String,
    pub definition_id: String,
    pub version: String,
    pub kind: CustomPagePackageKind,
    pub artifact_ref: String,
    pub entrypoint: String,
    pub integrity_digest: String,
    pub built_at: String,
}

impl ValidateCompanyOs for CustomPagePackage {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "CustomPagePackage.id")?;
        required(&self.definition_id, "CustomPagePackage.definition_id")?;
        required(&self.version, "CustomPagePackage.version")?;
        required(&self.artifact_ref, "CustomPagePackage.artifact_ref")?;
        required(&self.entrypoint, "CustomPagePackage.entrypoint")?;
        required(&self.integrity_digest, "CustomPagePackage.integrity_digest")?;
        required(&self.built_at, "CustomPagePackage.built_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    R0,
    R1,
    R2,
    R3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionCommandStatus {
    Requested,
    Authorized,
    Rejected,
    Executed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionEffect {
    CreateRecord,
    UpdateRecord,
    TransitionState,
    CreateRelation,
    DispatchExternal,
    CreateCommitment,
    SettlePayment,
    SubmitLegalFiling,
    ChangePermission,
    ChangeOrganization,
}

/// Server-owned policy for one named Action Command.
///
/// API implementations resolve this record by `ActionCommand.policy_ref` and
/// compare the request against it. They must not authorize from the risk,
/// permission, or approval flags copied into the request itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPolicyDefinition {
    pub id: String,
    pub module_ref: String,
    pub definition_ref: String,
    pub command_name: String,
    pub required_permission: String,
    pub risk_tier: RiskTier,
    pub requires_human_approval: bool,
    pub allowed_actor_kinds: Vec<ActorType>,
    pub allowed_effects: Vec<ActionEffect>,
}

impl ValidateCompanyOs for ActionPolicyDefinition {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "ActionPolicyDefinition.id")?;
        required(&self.module_ref, "ActionPolicyDefinition.module_ref")?;
        required(
            &self.definition_ref,
            "ActionPolicyDefinition.definition_ref",
        )?;
        required(&self.command_name, "ActionPolicyDefinition.command_name")?;
        required(
            &self.required_permission,
            "ActionPolicyDefinition.required_permission",
        )?;
        if self.allowed_actor_kinds.is_empty() {
            return Err(CompanyOsValidationError::Required {
                field: "ActionPolicyDefinition.allowed_actor_kinds",
            });
        }
        let unique_actor_kinds = self
            .allowed_actor_kinds
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if unique_actor_kinds.len() != self.allowed_actor_kinds.len() {
            return Err(CompanyOsValidationError::Invalid {
                field: "ActionPolicyDefinition.allowed_actor_kinds",
                reason: "must not contain duplicates".into(),
            });
        }
        if self.allowed_effects.is_empty() {
            return Err(CompanyOsValidationError::Required {
                field: "ActionPolicyDefinition.allowed_effects",
            });
        }
        let unique_effects = self
            .allowed_effects
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if unique_effects.len() != self.allowed_effects.len() {
            return Err(CompanyOsValidationError::Invalid {
                field: "ActionPolicyDefinition.allowed_effects",
                reason: "must not contain duplicates".into(),
            });
        }
        if self.risk_tier == RiskTier::R3 && !self.requires_human_approval {
            return Err(CompanyOsValidationError::Invalid {
                field: "ActionPolicyDefinition.requires_human_approval",
                reason: "R3 policies require an explicit human gate".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    Requested,
    PolicyAuthorized,
    ApprovalAttached,
    Executed,
    Failed,
    Cancelled,
}

/// Append-only evidence of an observed Action Command state change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub action_command_id: String,
    pub event_kind: AuditEventKind,
    pub actor_ref: ActorRef,
    pub subject_ref: EntityRef,
    pub detail: Value,
    pub evidence_refs: Vec<String>,
    pub occurred_at: String,
}

impl ValidateCompanyOs for AuditEvent {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "AuditEvent.id")?;
        required(&self.action_command_id, "AuditEvent.action_command_id")?;
        self.actor_ref.validate()?;
        self.subject_ref.validate()?;
        required_object(&self.detail, "AuditEvent.detail")?;
        required_strings(&self.evidence_refs, "AuditEvent.evidence_refs")?;
        required(&self.occurred_at, "AuditEvent.occurred_at")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionCommand {
    /// Canonical idempotency key. A store must return the existing command for
    /// a repeated id rather than apply its effect twice.
    pub id: String,
    pub command_name: String,
    pub subject_ref: EntityRef,
    pub requested_by: ActorRef,
    pub payload: Value,
    pub required_permission: String,
    pub policy_ref: String,
    pub risk_tier: RiskTier,
    pub requires_human_approval: bool,
    pub approval_refs: Vec<String>,
    pub status: ActionCommandStatus,
    pub audit_event_refs: Vec<String>,
    pub requested_at: String,
    pub completed_at: Option<String>,
}

impl ValidateCompanyOs for ActionCommand {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "ActionCommand.id")?;
        required(&self.command_name, "ActionCommand.command_name")?;
        self.subject_ref.validate()?;
        self.requested_by.validate()?;
        required_object(&self.payload, "ActionCommand.payload")?;
        required(
            &self.required_permission,
            "ActionCommand.required_permission",
        )?;
        required(&self.policy_ref, "ActionCommand.policy_ref")?;
        required_strings(&self.approval_refs, "ActionCommand.approval_refs")?;
        required_strings(&self.audit_event_refs, "ActionCommand.audit_event_refs")?;
        required(&self.requested_at, "ActionCommand.requested_at")?;
        if self.risk_tier == RiskTier::R3 && !self.requires_human_approval {
            return Err(CompanyOsValidationError::Invalid {
                field: "ActionCommand.requires_human_approval",
                reason: "R3 actions require an explicit human gate".into(),
            });
        }
        if self.requires_human_approval
            && matches!(
                self.status,
                ActionCommandStatus::Authorized | ActionCommandStatus::Executed
            )
            && self.approval_refs.is_empty()
        {
            return Err(CompanyOsValidationError::Required {
                field: "ActionCommand.approval_refs",
            });
        }
        if self.status == ActionCommandStatus::Executed {
            if self.audit_event_refs.is_empty() {
                return Err(CompanyOsValidationError::Required {
                    field: "ActionCommand.audit_event_refs",
                });
            }
            if self
                .completed_at
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                return Err(CompanyOsValidationError::Required {
                    field: "ActionCommand.completed_at",
                });
            }
        }
        Ok(())
    }
}

impl ActionCommand {
    pub fn idempotency_key(&self) -> &str {
        &self.id
    }

    /// Validate client-supplied snapshots against the server-owned policy and
    /// the effect selected by server dispatch code.
    pub fn validate_against_policy(
        &self,
        policy: &ActionPolicyDefinition,
        effect: ActionEffect,
    ) -> Result<(), CompanyOsValidationError> {
        self.validate()?;
        policy.validate()?;
        let mismatch = if self.policy_ref != policy.id {
            Some((
                "ActionCommand.policy_ref",
                "does not resolve to the supplied policy",
            ))
        } else if self.command_name != policy.command_name {
            Some(("ActionCommand.command_name", "does not match server policy"))
        } else if self.required_permission != policy.required_permission {
            Some((
                "ActionCommand.required_permission",
                "does not match server policy",
            ))
        } else if self.risk_tier != policy.risk_tier {
            Some(("ActionCommand.risk_tier", "does not match server policy"))
        } else if self.requires_human_approval != policy.requires_human_approval {
            Some((
                "ActionCommand.requires_human_approval",
                "does not match server policy",
            ))
        } else if !policy
            .allowed_actor_kinds
            .contains(&self.requested_by.actor_type)
        {
            Some((
                "ActionCommand.requested_by",
                "actor kind is not allowed by server policy",
            ))
        } else if !policy.allowed_effects.contains(&effect) {
            Some((
                "ActionCommand.command_name",
                "server-selected effect is not allowed by policy",
            ))
        } else {
            None
        };
        if let Some((field, reason)) = mismatch {
            return Err(CompanyOsValidationError::Invalid {
                field,
                reason: reason.into(),
            });
        }
        Ok(())
    }
}
