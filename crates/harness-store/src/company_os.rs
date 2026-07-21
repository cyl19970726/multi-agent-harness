//! Append-only persistence for the Company OS product model.
//!
//! The ledgers in this module are deliberately namespaced and independent from
//! the legacy Goal/Task ledgers and from executor-native Mission/Wave records.
//! Reads use the repository's existing latest-row-wins projection.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use harness_core::{
    ActionCommand, ActionCommandStatus, ActionPolicyDefinition, ActorRef, ActorType, ActorWorkload,
    Approval, ApprovalStatus, Assignment, AuditEvent, AuditEventKind, Block, BusinessModule,
    Commitment, CommitmentStatus, CustomPageDefinition, CustomPagePackage, Document, EntityKind,
    EntityRef, ExternalParticipant, HumanMember, MemberStatus, Milestone, MilestoneProgress,
    OrgUnit, OrganizationMembership, Payment, PaymentStatus, Relation, ServiceActor, StandingAgent,
    TypedRecord, ValidateCompanyOs, View, WorkItem, WorkItemStatus, WorkProjection, WorkQuery,
    WorkSummary, WorkType,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::{latest_by_id, HarnessStore, StoreError, StoreResult};

const DOCUMENTS: &str = "company_os_documents.jsonl";
const BLOCKS: &str = "company_os_blocks.jsonl";
const TYPED_RECORDS: &str = "company_os_typed_records.jsonl";
const RELATIONS: &str = "company_os_relations.jsonl";
const VIEWS: &str = "company_os_views.jsonl";
const BUSINESS_MODULES: &str = "company_os_business_modules.jsonl";
const HUMAN_MEMBERS: &str = "company_os_human_members.jsonl";
const STANDING_AGENTS: &str = "company_os_standing_agents.jsonl";
const EXTERNAL_PARTICIPANTS: &str = "company_os_external_participants.jsonl";
const SERVICE_ACTORS: &str = "company_os_service_actors.jsonl";
const ORG_UNITS: &str = "company_os_org_units.jsonl";
const ORGANIZATION_MEMBERSHIPS: &str = "company_os_organization_memberships.jsonl";
const MILESTONES: &str = "company_os_milestones.jsonl";
const WORK_ITEMS: &str = "company_os_work_items.jsonl";
const ASSIGNMENTS: &str = "company_os_assignments.jsonl";
const APPROVALS: &str = "company_os_approvals.jsonl";
const COMMITMENTS: &str = "company_os_commitments.jsonl";
const PAYMENTS: &str = "company_os_payments.jsonl";
const CUSTOM_PAGE_DEFINITIONS: &str = "company_os_custom_page_definitions.jsonl";
const CUSTOM_PAGE_PACKAGES: &str = "company_os_custom_page_packages.jsonl";
const ACTION_COMMANDS: &str = "company_os_action_commands.jsonl";
const ACTION_POLICY_DEFINITIONS: &str = "company_os_action_policy_definitions.jsonl";
const AUDIT_EVENTS: &str = "company_os_audit_events.jsonl";
const ACTION_AUDIT_RESERVATIONS: &str = "company_os_action_audit_reservations.jsonl";

/// A read projection over the four first-class Company OS actor ledgers.
///
/// This intentionally does not include the legacy executor `AgentMember`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "actor_type", content = "actor", rename_all = "snake_case")]
pub enum CompanyActor {
    Human(HumanMember),
    Agent(StandingAgent),
    External(ExternalParticipant),
    Service(ServiceActor),
}

impl CompanyActor {
    pub fn actor_ref(&self) -> ActorRef {
        match self {
            Self::Human(actor) => ActorRef {
                actor_type: ActorType::Human,
                actor_id: actor.id.clone(),
            },
            Self::Agent(actor) => ActorRef {
                actor_type: ActorType::Agent,
                actor_id: actor.id.clone(),
            },
            Self::External(actor) => ActorRef {
                actor_type: ActorType::External,
                actor_id: actor.id.clone(),
            },
            Self::Service(actor) => ActorRef {
                actor_type: ActorType::Service,
                actor_id: actor.id.clone(),
            },
        }
    }
}

/// Financial records remain explicitly typed. Appending a Commitment never
/// creates a Payment, and a Payment must be appended through its own API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "record_type", content = "record", rename_all = "snake_case")]
pub enum FinancialRecord {
    Commitment(Commitment),
    Payment(Payment),
}

/// Atomic result of claiming a command id. A replay returns the current row
/// for the same immutable request identity; a conflicting reuse never writes.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionCommandClaimResult {
    Claimed(ActionCommand),
    Replay(ActionCommand),
    Conflict(ActionCommand),
}

/// Durable ownership reservation for an AuditEvent id. This closes the race
/// between validating a future terminal id and applying the governed effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionAuditReservation {
    pub id: String,
    pub action_command_id: String,
}

impl FinancialRecord {
    pub fn id(&self) -> &str {
        match self {
            Self::Commitment(record) => &record.id,
            Self::Payment(record) => &record.id,
        }
    }
}

macro_rules! company_read_api {
    ($raw:ident, $latest:ident, $type:ty, $file:expr) => {
        /// Raw append-only ledger rows in append order.
        pub fn $raw(&self) -> StoreResult<Vec<$type>> {
            self.read_jsonl($file)
        }

        /// Latest-row-wins projection, ordered by id.
        pub fn $latest(&self) -> StoreResult<Vec<$type>> {
            Ok(latest_by_id(self.$raw()?, |row| row.id.clone())
                .into_values()
                .collect())
        }
    };
}

impl HarnessStore {
    company_read_api!(documents, latest_documents, Document, DOCUMENTS);
    company_read_api!(blocks, latest_blocks, Block, BLOCKS);
    company_read_api!(
        typed_records,
        latest_typed_records,
        TypedRecord,
        TYPED_RECORDS
    );
    company_read_api!(relations, latest_relations, Relation, RELATIONS);
    company_read_api!(views, latest_views, View, VIEWS);
    company_read_api!(
        business_modules,
        latest_business_modules,
        BusinessModule,
        BUSINESS_MODULES
    );
    company_read_api!(
        human_members,
        latest_human_members,
        HumanMember,
        HUMAN_MEMBERS
    );
    company_read_api!(
        standing_agents,
        latest_standing_agents,
        StandingAgent,
        STANDING_AGENTS
    );
    company_read_api!(
        external_participants,
        latest_external_participants,
        ExternalParticipant,
        EXTERNAL_PARTICIPANTS
    );
    company_read_api!(
        service_actors,
        latest_service_actors,
        ServiceActor,
        SERVICE_ACTORS
    );
    company_read_api!(org_units, latest_org_units, OrgUnit, ORG_UNITS);
    company_read_api!(
        organization_memberships,
        latest_organization_memberships,
        OrganizationMembership,
        ORGANIZATION_MEMBERSHIPS
    );
    company_read_api!(work_items, latest_work_items, WorkItem, WORK_ITEMS);
    company_read_api!(milestones, latest_milestones, Milestone, MILESTONES);
    company_read_api!(assignments, latest_assignments, Assignment, ASSIGNMENTS);
    company_read_api!(approvals, latest_approvals, Approval, APPROVALS);
    company_read_api!(commitments, latest_commitments, Commitment, COMMITMENTS);
    company_read_api!(payments, latest_payments, Payment, PAYMENTS);
    company_read_api!(
        action_commands,
        latest_action_commands,
        ActionCommand,
        ACTION_COMMANDS
    );
    company_read_api!(
        action_policy_definitions,
        latest_action_policy_definitions,
        ActionPolicyDefinition,
        ACTION_POLICY_DEFINITIONS
    );
    company_read_api!(audit_events, latest_audit_events, AuditEvent, AUDIT_EVENTS);
    company_read_api!(
        action_audit_reservations,
        latest_action_audit_reservations,
        ActionAuditReservation,
        ACTION_AUDIT_RESERVATIONS
    );

    pub fn latest_action_command(&self, id: &str) -> StoreResult<Option<ActionCommand>> {
        self.find_by_id(ACTION_COMMANDS, id)
    }

    pub fn latest_action_policy_definition(
        &self,
        id: &str,
    ) -> StoreResult<Option<ActionPolicyDefinition>> {
        self.find_by_id(ACTION_POLICY_DEFINITIONS, id)
    }

    pub fn latest_audit_event(&self, id: &str) -> StoreResult<Option<AuditEvent>> {
        self.find_by_id(AUDIT_EVENTS, id)
    }
    company_read_api!(
        custom_page_definitions,
        latest_custom_page_definitions,
        CustomPageDefinition,
        CUSTOM_PAGE_DEFINITIONS
    );
    company_read_api!(
        custom_page_packages,
        latest_custom_page_packages,
        CustomPagePackage,
        CUSTOM_PAGE_PACKAGES
    );

    /// Combined actor rows. Each underlying actor-kind ledger remains
    /// append-only; this typed list is a convenience for generic read paths.
    pub fn actors(&self) -> StoreResult<Vec<CompanyActor>> {
        let mut actors = Vec::new();
        actors.extend(self.human_members()?.into_iter().map(CompanyActor::Human));
        actors.extend(self.standing_agents()?.into_iter().map(CompanyActor::Agent));
        actors.extend(
            self.external_participants()?
                .into_iter()
                .map(CompanyActor::External),
        );
        actors.extend(
            self.service_actors()?
                .into_iter()
                .map(CompanyActor::Service),
        );
        Ok(actors)
    }

    pub fn append_actor(&self, value: &CompanyActor) -> StoreResult<()> {
        match value {
            CompanyActor::Human(actor) => self.append_human_member(actor),
            CompanyActor::Agent(actor) => self.append_standing_agent(actor),
            CompanyActor::External(actor) => self.append_external_participant(actor),
            CompanyActor::Service(actor) => self.append_service_actor(actor),
        }
    }

    /// Combined raw financial rows. The explicit type is preserved and no
    /// projection turns a Commitment into a Payment.
    pub fn financial_records(&self) -> StoreResult<Vec<FinancialRecord>> {
        let mut records = Vec::new();
        records.extend(
            self.commitments()?
                .into_iter()
                .map(FinancialRecord::Commitment),
        );
        records.extend(self.payments()?.into_iter().map(FinancialRecord::Payment));
        Ok(records)
    }

    pub fn append_financial_record(&self, value: &FinancialRecord) -> StoreResult<()> {
        match value {
            FinancialRecord::Commitment(record) => self.append_commitment(record),
            FinancialRecord::Payment(record) => self.append_payment(record),
        }
    }

    pub fn append_human_member(&self, value: &HumanMember) -> StoreResult<()> {
        self.append_company_row(HUMAN_MEMBERS, value, |_| Ok(()))
    }

    pub fn append_standing_agent(&self, value: &StandingAgent) -> StoreResult<()> {
        self.append_company_row(STANDING_AGENTS, value, |_| Ok(()))
    }

    pub fn append_external_participant(&self, value: &ExternalParticipant) -> StoreResult<()> {
        self.append_company_row(EXTERNAL_PARTICIPANTS, value, |store| {
            store.require_actor(&value.sponsor_actor_ref)
        })
    }

    pub fn append_service_actor(&self, value: &ServiceActor) -> StoreResult<()> {
        self.append_company_row(SERVICE_ACTORS, value, |store| {
            store.require_actor(&value.owner_actor_ref)
        })
    }

    pub fn append_org_unit(&self, value: &OrgUnit) -> StoreResult<()> {
        self.append_company_row(ORG_UNITS, value, |store| {
            if let Some(parent_id) = &value.parent_unit_id {
                store.require_id::<OrgUnit>(ORG_UNITS, parent_id, "OrgUnit")?;
            }
            if let Some(actor) = &value.human_lead_actor_ref {
                store.require_actor(actor)?;
            }
            if let Some(actor) = &value.agent_lead_actor_ref {
                store.require_actor(actor)?;
            }
            Ok(())
        })
    }

    pub fn append_organization_membership(
        &self,
        value: &OrganizationMembership,
    ) -> StoreResult<()> {
        self.append_company_row(ORGANIZATION_MEMBERSHIPS, value, |store| {
            store.require_id::<OrgUnit>(ORG_UNITS, &value.org_unit_id, "OrgUnit")?;
            store.require_actor(&value.actor_ref)?;
            store.require_actor(&value.created_by_actor_ref)
        })
    }

    pub fn append_document(&self, value: &Document) -> StoreResult<()> {
        self.append_company_row(DOCUMENTS, value, |store| {
            if let Some(parent_id) = &value.parent_document_id {
                store.require_id::<Document>(DOCUMENTS, parent_id, "Document")?;
            }
            for block_id in &value.block_ids {
                store.require_id::<Block>(BLOCKS, block_id, "Block")?;
            }
            store.require_actor(&value.created_by)?;
            store.require_actor(&value.updated_by)?;
            for reference in &value.reference_refs {
                store.require_entity(reference)?;
            }
            Ok(())
        })
    }

    pub fn append_block(&self, value: &Block) -> StoreResult<()> {
        self.append_company_row(BLOCKS, value, |store| {
            store.require_id::<Document>(DOCUMENTS, &value.document_id, "Document")?;
            store.require_actor(&value.created_by)?;
            store.require_actor(&value.updated_by)?;
            for reference in &value.referenced_entities {
                store.require_entity(reference)?;
            }
            Ok(())
        })
    }

    pub fn append_typed_record(&self, value: &TypedRecord) -> StoreResult<()> {
        self.append_company_row(TYPED_RECORDS, value, |store| {
            store.require_id::<BusinessModule>(
                BUSINESS_MODULES,
                &value.module_id,
                "BusinessModule",
            )?;
            if let Some(document_id) = &value.source_document_ref {
                store.require_id::<Document>(DOCUMENTS, document_id, "Document")?;
            }
            store.require_actor(&value.created_by)?;
            store.require_actor(&value.updated_by)
        })
    }

    pub fn append_relation(&self, value: &Relation) -> StoreResult<()> {
        self.append_company_row(RELATIONS, value, |store| {
            store.require_entity(&value.from_ref)?;
            store.require_entity(&value.to_ref)?;
            if let Some(reference) = &value.provenance_ref {
                store.require_entity(reference)?;
            }
            store.require_actor(&value.created_by)
        })
    }

    pub fn append_view(&self, value: &View) -> StoreResult<()> {
        self.append_company_row(VIEWS, value, |store| {
            if let Some(module_id) = &value.module_id {
                store.require_id::<BusinessModule>(
                    BUSINESS_MODULES,
                    module_id,
                    "BusinessModule",
                )?;
            }
            store.require_actor(&value.owner)
        })
    }

    pub fn append_business_module(&self, value: &BusinessModule) -> StoreResult<()> {
        self.append_company_row(BUSINESS_MODULES, value, |store| {
            store.require_id::<Document>(DOCUMENTS, &value.root_document_ref, "Document")?;
            store.require_actor(&value.owner)?;
            for view_id in &value.default_view_refs {
                store.require_id::<View>(VIEWS, view_id, "View")?;
            }
            for definition_id in &value.custom_page_definition_refs {
                store.require_id::<CustomPageDefinition>(
                    CUSTOM_PAGE_DEFINITIONS,
                    definition_id,
                    "CustomPageDefinition",
                )?;
            }
            Ok(())
        })
    }

    pub fn append_work_item(&self, value: &WorkItem) -> StoreResult<()> {
        self.append_company_row(WORK_ITEMS, value, |store| {
            store.require_id::<Document>(DOCUMENTS, &value.source_document_ref, "Document")?;
            for record_id in &value.source_record_refs {
                store.require_id::<TypedRecord>(TYPED_RECORDS, record_id, "TypedRecord")?;
            }
            if let Some(milestone_id) = &value.milestone_ref {
                store.require_id::<Milestone>(MILESTONES, milestone_id, "Milestone")?;
            }
            if let Some(module_id) = &value.business_module_ref {
                store.require_id::<BusinessModule>(
                    BUSINESS_MODULES,
                    module_id,
                    "BusinessModule",
                )?;
            }
            if let Some(document_id) = &value.result_document_ref {
                store.require_id::<Document>(DOCUMENTS, document_id, "Document")?;
            }
            for record_id in &value.result_record_refs {
                store.require_id::<TypedRecord>(TYPED_RECORDS, record_id, "TypedRecord")?;
            }
            store.require_actor(&value.submitted_by)?;
            if let Some(actor) = &value.requested_by {
                store.require_actor(actor)?;
            }
            store.require_actor(&value.accountable_owner)?;
            for actor in value.assignees.iter().chain(&value.contributors) {
                store.require_actor(actor)?;
            }
            if let Some(actor) = &value.reviewer {
                store.require_actor(actor)?;
            }
            if let Some(actor) = &value.approver {
                store.require_actor(actor)?;
            }
            for approval_id in &value.approval_refs {
                store.require_id::<Approval>(APPROVALS, approval_id, "Approval")?;
            }
            Ok(())
        })
    }

    pub fn append_milestone(&self, value: &Milestone) -> StoreResult<()> {
        self.append_company_row(MILESTONES, value, |store| {
            store.require_actor(&value.accountable_owner)?;
            if let Some(document_id) = &value.source_document_ref {
                store.require_id::<Document>(DOCUMENTS, document_id, "Document")?;
            }
            if let Some(module_id) = &value.business_module_ref {
                store.require_id::<BusinessModule>(
                    BUSINESS_MODULES,
                    module_id,
                    "BusinessModule",
                )?;
            }
            for work_item_id in &value.work_item_refs {
                store.require_id::<WorkItem>(WORK_ITEMS, work_item_id, "WorkItem")?;
            }
            Ok(())
        })
    }

    pub fn append_assignment(&self, value: &Assignment) -> StoreResult<()> {
        self.append_company_row(ASSIGNMENTS, value, |store| {
            store.require_id::<WorkItem>(WORK_ITEMS, &value.work_item_id, "WorkItem")?;
            store.require_actor(&value.recipient)?;
            store.require_actor(&value.sender)
        })
    }

    pub fn append_approval(&self, value: &Approval) -> StoreResult<()> {
        self.append_company_row(APPROVALS, value, |store| {
            store.validate_approval_transition(value)?;
            store.require_entity(&value.subject_ref)?;
            store.require_authorized_actor_at(&value.requested_by, &value.requested_at)?;
            for actor in &value.required_approver_refs {
                store.require_authorized_actor_at(actor, &value.requested_at)?;
            }
            if let Some(decided_at) = &value.decided_at {
                for actor in &value.decided_by {
                    store.require_authorized_actor_at(actor, decided_at)?;
                }
            }
            Ok(())
        })
    }

    pub fn append_commitment(&self, value: &Commitment) -> StoreResult<()> {
        self.append_company_row(COMMITMENTS, value, |store| {
            store.validate_commitment_transition(value)?;
            store.require_id::<Document>(DOCUMENTS, &value.source_document_id, "Document")?;
            store.require_authorized_actor_at(&value.submitted_by, &value.updated_at)?;
            store.require_authorized_actor_at(&value.accountable_owner, &value.updated_at)?;
            for relation_id in &value.relation_ids {
                store.require_id::<Relation>(RELATIONS, relation_id, "Relation")?;
            }
            for approval_id in &value.approval_refs {
                store.require_id::<Approval>(APPROVALS, approval_id, "Approval")?;
            }
            Ok(())
        })
    }

    pub fn append_payment(&self, value: &Payment) -> StoreResult<()> {
        self.append_company_row(PAYMENTS, value, |store| {
            store.validate_payment_transition(value)?;
            store.require_id::<Document>(DOCUMENTS, &value.source_document_id, "Document")?;
            store.require_authorized_actor_at(&value.submitted_by, &value.updated_at)?;
            store.require_authorized_actor_at(&value.accountable_owner, &value.updated_at)?;
            for commitment_id in &value.related_commitment_refs {
                store.require_id::<Commitment>(COMMITMENTS, commitment_id, "Commitment")?;
            }
            for relation_id in &value.relation_ids {
                store.require_id::<Relation>(RELATIONS, relation_id, "Relation")?;
            }
            for approval_id in &value.approval_refs {
                store.require_id::<Approval>(APPROVALS, approval_id, "Approval")?;
            }
            Ok(())
        })
    }

    /// Policies are server-owned governance configuration. Updating an
    /// existing id is append-only, while its module and definition identity
    /// remain immutable.
    pub fn append_action_policy_definition(
        &self,
        value: &ActionPolicyDefinition,
    ) -> StoreResult<()> {
        self.append_company_row(ACTION_POLICY_DEFINITIONS, value, |store| {
            store.require_id::<BusinessModule>(
                BUSINESS_MODULES,
                &value.module_ref,
                "BusinessModule",
            )?;
            store.require_id::<CustomPageDefinition>(
                CUSTOM_PAGE_DEFINITIONS,
                &value.definition_ref,
                "CustomPageDefinition",
            )?;
            if let Some(previous) =
                store.find_by_id::<ActionPolicyDefinition>(ACTION_POLICY_DEFINITIONS, &value.id)?
            {
                if previous.module_ref != value.module_ref
                    || previous.definition_ref != value.definition_ref
                    || previous.command_name != value.command_name
                {
                    return Err(StoreError::Conflict(format!(
                        "immutable action policy identity changed: {}",
                        value.id
                    )));
                }
            }
            Ok(())
        })
    }

    /// Atomically claim a canonical command id. A same-identity retry returns
    /// Replay without appending; a different request reusing the id returns
    /// Conflict without applying an effect.
    pub fn claim_action_command(
        &self,
        value: &ActionCommand,
    ) -> StoreResult<ActionCommandClaimResult> {
        value
            .validate()
            .map_err(|error| StoreError::CompanyOsValidation(error.to_string()))?;
        if value.status != ActionCommandStatus::Requested {
            return Err(StoreError::Conflict(
                "a claimed ActionCommand must start in requested status".into(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.validate_action_request(value)?;
        if let Some(existing) = self.find_by_id::<ActionCommand>(ACTION_COMMANDS, &value.id)? {
            let original_claim = self
                .read_jsonl::<ActionCommand>(ACTION_COMMANDS)?
                .into_iter()
                .find(|row| row.id == value.id)
                .ok_or_else(|| missing("ActionCommand", &value.id))?;
            if same_action_claim(&original_claim, value) {
                return Ok(ActionCommandClaimResult::Replay(existing));
            }
            return Ok(ActionCommandClaimResult::Conflict(existing));
        }
        self.append_jsonl_unlocked(ACTION_COMMANDS, value)?;
        Ok(ActionCommandClaimResult::Claimed(value.clone()))
    }

    /// Atomically claim a command id and reserve all future AuditEvent ids.
    /// Any command-identity or reservation conflict is discovered before
    /// either ledger changes.
    pub fn claim_action_command_with_audit_reservations(
        &self,
        value: &ActionCommand,
        audit_ids: &[String],
    ) -> StoreResult<ActionCommandClaimResult> {
        value
            .validate()
            .map_err(|error| StoreError::CompanyOsValidation(error.to_string()))?;
        if value.status != ActionCommandStatus::Requested {
            return Err(StoreError::Conflict(
                "a claimed ActionCommand must start in requested status".into(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.validate_action_request(value)?;

        let existing = self.find_by_id::<ActionCommand>(ACTION_COMMANDS, &value.id)?;
        let result = if let Some(existing) = existing {
            let original_claim = self
                .read_jsonl::<ActionCommand>(ACTION_COMMANDS)?
                .into_iter()
                .find(|row| row.id == value.id)
                .ok_or_else(|| missing("ActionCommand", &value.id))?;
            if !same_action_claim(&original_claim, value) {
                return Ok(ActionCommandClaimResult::Conflict(existing));
            }
            ActionCommandClaimResult::Replay(existing)
        } else {
            ActionCommandClaimResult::Claimed(value.clone())
        };

        let reservations = self.prevalidate_audit_reservations_locked(&value.id, audit_ids)?;
        if matches!(result, ActionCommandClaimResult::Claimed(_)) {
            self.append_jsonl_unlocked(ACTION_COMMANDS, value)?;
        }
        for reservation in reservations {
            self.append_jsonl_unlocked(ACTION_AUDIT_RESERVATIONS, &reservation)?;
        }
        Ok(result)
    }

    /// Reserve future AuditEvent ids for one command before an external effect
    /// is applied. Exact same-owner replay is a no-op; another owner conflicts
    /// and no reservation from the call is appended.
    pub fn reserve_action_audit_ids(&self, command_id: &str, ids: &[String]) -> StoreResult<()> {
        if command_id.trim().is_empty() || ids.is_empty() {
            return Err(StoreError::Conflict(
                "command id and at least one audit id are required".into(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_id::<ActionCommand>(ACTION_COMMANDS, command_id, "ActionCommand")?;
        let append = self.prevalidate_audit_reservations_locked(command_id, ids)?;
        for reservation in append {
            self.append_jsonl_unlocked(ACTION_AUDIT_RESERVATIONS, &reservation)?;
        }
        Ok(())
    }

    /// Append one legal command state transition. Initial callers should use
    /// `claim_action_command` when they need the explicit CAS result.
    pub fn append_action_command(&self, value: &ActionCommand) -> StoreResult<()> {
        if value.status == ActionCommandStatus::Requested {
            return match self.claim_action_command(value)? {
                ActionCommandClaimResult::Claimed(_) | ActionCommandClaimResult::Replay(_) => {
                    Ok(())
                }
                ActionCommandClaimResult::Conflict(existing) => Err(StoreError::Conflict(format!(
                    "action command id {} already belongs to {}",
                    existing.id, existing.command_name
                ))),
            };
        }
        value
            .validate()
            .map_err(|error| StoreError::CompanyOsValidation(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if !self.validate_action_transition_locked(value, &[])? {
            return Ok(());
        }
        self.append_jsonl_unlocked(ACTION_COMMANDS, value)
    }

    pub fn complete_action_command(&self, value: &ActionCommand) -> StoreResult<()> {
        if value.status != ActionCommandStatus::Executed {
            return Err(StoreError::Conflict(
                "complete_action_command requires executed status".into(),
            ));
        }
        self.append_action_command(value)
    }

    pub fn fail_action_command(&self, value: &ActionCommand) -> StoreResult<()> {
        if value.status != ActionCommandStatus::Failed {
            return Err(StoreError::Conflict(
                "fail_action_command requires failed status".into(),
            ));
        }
        self.append_action_command(value)
    }

    /// Atomically append an Authorized command snapshot and its authorization
    /// audit events under one write lock. Every event is prevalidated before
    /// any ledger is changed; exact replay is a no-op.
    pub fn authorize_action_command_atomic(
        &self,
        value: &ActionCommand,
        events: &[AuditEvent],
    ) -> StoreResult<()> {
        if value.status != ActionCommandStatus::Authorized {
            return Err(StoreError::Conflict(
                "authorize_action_command_atomic requires authorized status".into(),
            ));
        }
        if events.is_empty()
            || events.iter().any(|event| {
                !matches!(
                    event.event_kind,
                    AuditEventKind::PolicyAuthorized | AuditEventKind::ApprovalAttached
                )
            })
        {
            return Err(StoreError::Conflict(
                "authorization requires PolicyAuthorized or ApprovalAttached audit events".into(),
            ));
        }
        self.append_action_transition_with_events_atomic(value, events)
    }

    /// Atomically append an Executed or Failed command snapshot and its
    /// terminal audit events under one write lock. Exact replay is safe.
    pub fn finish_action_command_atomic(
        &self,
        value: &ActionCommand,
        events: &[AuditEvent],
    ) -> StoreResult<()> {
        if !matches!(
            value.status,
            ActionCommandStatus::Executed | ActionCommandStatus::Failed
        ) {
            return Err(StoreError::Conflict(
                "finish_action_command_atomic requires executed or failed status".into(),
            ));
        }
        let expected_kind = if value.status == ActionCommandStatus::Executed {
            AuditEventKind::Executed
        } else {
            AuditEventKind::Failed
        };
        if events.is_empty() || !events.iter().any(|event| event.event_kind == expected_kind) {
            return Err(StoreError::Conflict(
                "terminal action transition requires a matching terminal audit event".into(),
            ));
        }
        self.append_action_transition_with_events_atomic(value, events)
    }

    /// Atomically install one Custom Page Definition and all of its server
    /// Action policies. Conflicts are discovered before either ledger changes.
    pub fn append_custom_page_bundle_atomic(
        &self,
        definition: &CustomPageDefinition,
        policies: &[ActionPolicyDefinition],
    ) -> StoreResult<()> {
        definition
            .validate()
            .map_err(|error| StoreError::CompanyOsValidation(error.to_string()))?;
        for policy in policies {
            policy
                .validate()
                .map_err(|error| StoreError::CompanyOsValidation(error.to_string()))?;
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;

        self.validate_custom_page_definition_refs_locked(definition)?;
        let existing_definition =
            self.find_by_id::<CustomPageDefinition>(CUSTOM_PAGE_DEFINITIONS, &definition.id)?;
        if existing_definition
            .as_ref()
            .is_some_and(|existing| existing != definition)
        {
            return Err(StoreError::Conflict(format!(
                "custom page definition id already has different content: {}",
                definition.id
            )));
        }

        let mut seen = std::collections::BTreeSet::new();
        let mut append_policies = Vec::new();
        for policy in policies {
            if !seen.insert(&policy.id) {
                return Err(StoreError::Conflict(format!(
                    "duplicate action policy id in bundle: {}",
                    policy.id
                )));
            }
            if policy.module_ref != definition.module_id
                || policy.definition_ref != definition.id
                || !definition
                    .action_command_refs
                    .contains(&policy.command_name)
            {
                return Err(StoreError::Conflict(format!(
                    "action policy {} does not belong to custom page definition {}",
                    policy.id, definition.id
                )));
            }
            match self
                .find_by_id::<ActionPolicyDefinition>(ACTION_POLICY_DEFINITIONS, &policy.id)?
            {
                Some(existing) if existing != *policy => {
                    return Err(StoreError::Conflict(format!(
                        "action policy id already has different content: {}",
                        policy.id
                    )));
                }
                Some(_) => {}
                None => append_policies.push(policy),
            }
        }
        if !definition.action_command_refs.iter().all(|command_name| {
            policies
                .iter()
                .any(|policy| policy.command_name == *command_name)
        }) {
            return Err(StoreError::Conflict(format!(
                "custom page definition {} has undeclared bundle policies",
                definition.id
            )));
        }

        if existing_definition.is_none() {
            self.append_jsonl_unlocked(CUSTOM_PAGE_DEFINITIONS, definition)?;
        }
        for policy in append_policies {
            self.append_jsonl_unlocked(ACTION_POLICY_DEFINITIONS, policy)?;
        }
        Ok(())
    }

    /// Audit events are immutable observations. Replaying the exact same row
    /// is a no-op; changing an existing event id is rejected.
    pub fn append_audit_event(&self, value: &AuditEvent) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::CompanyOsValidation(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let command = self
            .find_by_id::<ActionCommand>(ACTION_COMMANDS, &value.action_command_id)?
            .ok_or_else(|| missing("ActionCommand", &value.action_command_id))?;
        if command.subject_ref != value.subject_ref {
            return Err(StoreError::Conflict(format!(
                "audit event {} subject does not match command {}",
                value.id, command.id
            )));
        }
        self.require_audit_reservation_owner_if_reserved(&value.id, &value.action_command_id)?;
        self.require_authorized_actor_at(&value.actor_ref, &value.occurred_at)?;
        self.require_entity(&value.subject_ref)?;
        if let Some(previous) = self.find_by_id::<AuditEvent>(AUDIT_EVENTS, &value.id)? {
            if previous == *value {
                return Ok(());
            }
            return Err(StoreError::Conflict(format!(
                "audit event id is immutable: {}",
                value.id
            )));
        }
        self.append_jsonl_unlocked(AUDIT_EVENTS, value)
    }

    pub fn append_custom_page_package(&self, value: &CustomPagePackage) -> StoreResult<()> {
        self.append_company_row(CUSTOM_PAGE_PACKAGES, value, |store| {
            // Packages are allowed to arrive before their definition to break
            // the definition/package reference cycle. Once a definition exists,
            // however, an update cannot silently retarget it.
            if let Some(definition) = store
                .find_by_id::<CustomPageDefinition>(CUSTOM_PAGE_DEFINITIONS, &value.definition_id)?
            {
                if definition.package_ref != value.id || definition.package_version != value.version
                {
                    return Err(StoreError::Conflict(format!(
                        "custom page package {} does not match definition {}",
                        value.id, value.definition_id
                    )));
                }
            }
            Ok(())
        })
    }

    pub fn append_custom_page_definition(&self, value: &CustomPageDefinition) -> StoreResult<()> {
        self.append_company_row(CUSTOM_PAGE_DEFINITIONS, value, |store| {
            store.require_id::<BusinessModule>(
                BUSINESS_MODULES,
                &value.module_id,
                "BusinessModule",
            )?;
            store.require_id::<View>(VIEWS, &value.standard_view_fallback_ref, "View")?;
            store.require_actor(&value.owner)?;
            let package = store
                .find_by_id::<CustomPagePackage>(CUSTOM_PAGE_PACKAGES, &value.package_ref)?
                .ok_or_else(|| missing("CustomPagePackage", &value.package_ref))?;
            if package.definition_id != value.id || package.version != value.package_version {
                return Err(StoreError::Conflict(format!(
                    "custom page definition {} does not match package {}",
                    value.id, value.package_ref
                )));
            }
            Ok(())
        })
    }

    /// Unified latest actor projection for governed read paths and Actions.
    pub fn latest_actors(&self) -> StoreResult<Vec<CompanyActor>> {
        let mut actors = Vec::new();
        actors.extend(
            self.latest_human_members()?
                .into_iter()
                .map(CompanyActor::Human),
        );
        actors.extend(
            self.latest_standing_agents()?
                .into_iter()
                .map(CompanyActor::Agent),
        );
        actors.extend(
            self.latest_external_participants()?
                .into_iter()
                .map(CompanyActor::External),
        );
        actors.extend(
            self.latest_service_actors()?
                .into_iter()
                .map(CompanyActor::Service),
        );
        actors.sort_by_key(|actor| actor.actor_ref().actor_id);
        Ok(actors)
    }

    pub fn latest_actor(&self, reference: &ActorRef) -> StoreResult<Option<CompanyActor>> {
        let actor = match reference.actor_type {
            ActorType::Human => self
                .find_by_id::<HumanMember>(HUMAN_MEMBERS, &reference.actor_id)?
                .map(CompanyActor::Human),
            ActorType::Agent => self
                .find_by_id::<StandingAgent>(STANDING_AGENTS, &reference.actor_id)?
                .map(CompanyActor::Agent),
            ActorType::External => self
                .find_by_id::<ExternalParticipant>(EXTERNAL_PARTICIPANTS, &reference.actor_id)?
                .map(CompanyActor::External),
            ActorType::Service => self
                .find_by_id::<ServiceActor>(SERVICE_ACTORS, &reference.actor_id)?
                .map(CompanyActor::Service),
        };
        Ok(actor)
    }

    /// Resolve an actor for a governed action at a concrete time. Membership
    /// existence alone is insufficient: inactive actors and expired external
    /// participants cannot be authorized.
    pub fn authorized_actor_at(
        &self,
        reference: &ActorRef,
        as_of: &str,
    ) -> StoreResult<CompanyActor> {
        let actor = self
            .latest_actor(reference)?
            .ok_or_else(|| missing(actor_kind(reference.actor_type), &reference.actor_id))?;
        let active = match &actor {
            CompanyActor::Human(value) => value.status == MemberStatus::Active,
            CompanyActor::Agent(value) => value.status == MemberStatus::Active,
            CompanyActor::External(value) => {
                value.status == MemberStatus::Active
                    && timestamp_is_after(&value.access_expires_at, as_of)
            }
            CompanyActor::Service(value) => value.status == MemberStatus::Active,
        };
        if !active {
            return Err(StoreError::Conflict(format!(
                "actor is not authorized at {as_of}: {}:{}",
                actor_kind(reference.actor_type),
                reference.actor_id
            )));
        }
        Ok(actor)
    }

    pub fn latest_financial_records(&self) -> StoreResult<Vec<FinancialRecord>> {
        let mut records = Vec::new();
        records.extend(
            self.latest_commitments()?
                .into_iter()
                .map(FinancialRecord::Commitment),
        );
        records.extend(
            self.latest_payments()?
                .into_iter()
                .map(FinancialRecord::Payment),
        );
        records.sort_by(|left, right| left.id().cmp(right.id()));
        Ok(records)
    }

    /// Build the shared, read-only Work projection used by every Work view.
    /// Filtering never creates a second task record and unknown dimensions stay
    /// explicitly unclassified.
    pub fn work_projection(&self, query: &WorkQuery) -> StoreResult<WorkProjection> {
        let work_items = self
            .latest_work_items()?
            .into_iter()
            .filter(|item| work_matches(item, query))
            .collect::<Vec<_>>();
        let mut summary = WorkSummary::default();
        let mut board = BTreeMap::<String, Vec<String>>::new();
        let mut business_lines = BTreeMap::<String, Vec<String>>::new();
        let mut work_types = BTreeMap::<String, Vec<String>>::new();
        let mut workload = BTreeMap::<ActorRef, (u64, u64, u64, BTreeSet<String>)>::new();

        for item in &work_items {
            summary.total += 1;
            summary.active += u64::from(work_item_is_active(item.status));
            summary.completed += u64::from(item.status == WorkItemStatus::Completed);
            summary.blocked += u64::from(item.status == WorkItemStatus::Blocked);
            summary.waiting_for_approval +=
                u64::from(item.status == WorkItemStatus::WaitingForApproval);
            summary.unassigned += u64::from(item.assignees.is_empty());
            summary.without_milestone += u64::from(item.milestone_ref.is_none());
            summary.without_business_line += u64::from(item.business_module_ref.is_none());

            board
                .entry(work_status_key(item.status).into())
                .or_default()
                .push(item.id.clone());
            business_lines
                .entry(
                    item.business_module_ref
                        .clone()
                        .unwrap_or_else(|| "unclassified".into()),
                )
                .or_default()
                .push(item.id.clone());
            work_types
                .entry(work_type_key(item.work_type).into())
                .or_default()
                .push(item.id.clone());

            let accountable = workload
                .entry(item.accountable_owner.clone())
                .or_insert_with(|| (0, 0, 0, BTreeSet::new()));
            accountable.0 += 1;
            accountable.2 += u64::from(work_item_is_active(item.status));
            accountable.3.insert(item.id.clone());
            for assignee in &item.assignees {
                let assigned = workload
                    .entry(assignee.clone())
                    .or_insert_with(|| (0, 0, 0, BTreeSet::new()));
                assigned.1 += 1;
                assigned.2 += u64::from(work_item_is_active(item.status));
                assigned.3.insert(item.id.clone());
            }
        }

        let milestones = self
            .latest_milestones()?
            .into_iter()
            .filter(|milestone| {
                (query.milestone_refs.is_empty() || query.milestone_refs.contains(&milestone.id))
                    && (query.business_module_refs.is_empty()
                        || milestone
                            .business_module_ref
                            .as_ref()
                            .is_some_and(|id| query.business_module_refs.contains(id)))
            })
            .map(|milestone| {
                let linked = work_items
                    .iter()
                    .filter(|item| {
                        item.milestone_ref.as_deref() == Some(milestone.id.as_str())
                            || milestone.work_item_refs.contains(&item.id)
                    })
                    .collect::<Vec<_>>();
                let total = linked.len() as u64;
                let completed = linked
                    .iter()
                    .filter(|item| item.status == WorkItemStatus::Completed)
                    .count() as u64;
                MilestoneProgress {
                    milestone,
                    total_work_items: total,
                    completed_work_items: completed,
                    blocked_work_items: linked
                        .iter()
                        .filter(|item| item.status == WorkItemStatus::Blocked)
                        .count() as u64,
                    waiting_for_approval_work_items: linked
                        .iter()
                        .filter(|item| item.status == WorkItemStatus::WaitingForApproval)
                        .count() as u64,
                    progress_percent: if total == 0 {
                        0
                    } else {
                        ((completed * 100) / total) as u8
                    },
                }
            })
            .collect();

        let workload = workload
            .into_iter()
            .map(
                |(actor, (accountable_count, assigned_count, active_count, refs))| ActorWorkload {
                    actor,
                    accountable_count,
                    assigned_count,
                    active_count,
                    work_item_refs: refs.into_iter().collect(),
                },
            )
            .collect();

        Ok(WorkProjection {
            query: query.clone(),
            summary,
            work_items,
            milestones,
            board,
            business_lines,
            work_types,
            workload,
        })
    }

    /// Resolve only the Company OS entity kinds owned by this store. Evidence
    /// and executor references remain governed by their native stores.
    pub fn company_entity_exists(&self, reference: &EntityRef) -> StoreResult<bool> {
        match reference.kind {
            EntityKind::Actor => Ok(self
                .latest_actors()?
                .iter()
                .any(|actor| actor.actor_ref().actor_id == reference.id)),
            EntityKind::Document => Ok(self
                .find_by_id::<Document>(DOCUMENTS, &reference.id)?
                .is_some()),
            EntityKind::TypedRecord => Ok(self
                .find_by_id::<TypedRecord>(TYPED_RECORDS, &reference.id)?
                .is_some()),
            EntityKind::BusinessModule => Ok(self
                .find_by_id::<BusinessModule>(BUSINESS_MODULES, &reference.id)?
                .is_some()),
            EntityKind::Milestone => Ok(self
                .find_by_id::<Milestone>(MILESTONES, &reference.id)?
                .is_some()),
            EntityKind::WorkItem => Ok(self
                .find_by_id::<WorkItem>(WORK_ITEMS, &reference.id)?
                .is_some()),
            EntityKind::Approval => Ok(self
                .find_by_id::<Approval>(APPROVALS, &reference.id)?
                .is_some()),
            EntityKind::FinancialRecord => Ok(self
                .find_by_id::<Commitment>(COMMITMENTS, &reference.id)?
                .is_some()
                || self
                    .find_by_id::<Payment>(PAYMENTS, &reference.id)?
                    .is_some()),
            EntityKind::Evidence | EntityKind::Execution => Ok(false),
        }
    }

    fn append_company_row<T: Serialize + ValidateCompanyOs>(
        &self,
        file_name: &str,
        value: &T,
        validate_references: impl FnOnce(&Self) -> StoreResult<()>,
    ) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::CompanyOsValidation(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        validate_references(self)?;
        self.append_jsonl_unlocked(file_name, value)
    }

    fn find_by_id<T: DeserializeOwned + HasId>(
        &self,
        file_name: &str,
        id: &str,
    ) -> StoreResult<Option<T>> {
        Ok(latest_by_id(self.read_jsonl::<T>(file_name)?, |row| row.id().to_string()).remove(id))
    }

    fn require_id<T: DeserializeOwned + HasId>(
        &self,
        file_name: &str,
        id: &str,
        kind: &str,
    ) -> StoreResult<()> {
        if self.find_by_id::<T>(file_name, id)?.is_none() {
            return Err(missing(kind, id));
        }
        Ok(())
    }

    fn require_actor(&self, reference: &ActorRef) -> StoreResult<()> {
        if self.latest_actor(reference)?.is_none() {
            return Err(missing(
                actor_kind(reference.actor_type),
                &reference.actor_id,
            ));
        }
        Ok(())
    }

    fn require_authorized_actor_at(&self, reference: &ActorRef, as_of: &str) -> StoreResult<()> {
        self.authorized_actor_at(reference, as_of).map(|_| ())
    }

    fn require_audit_reservation_owner_if_reserved(
        &self,
        audit_id: &str,
        command_id: &str,
    ) -> StoreResult<()> {
        if let Some(reservation) =
            self.find_by_id::<ActionAuditReservation>(ACTION_AUDIT_RESERVATIONS, audit_id)?
        {
            if reservation.action_command_id != command_id {
                return Err(StoreError::Conflict(format!(
                    "audit id {audit_id} is reserved by command {}",
                    reservation.action_command_id
                )));
            }
        }
        Ok(())
    }

    fn prevalidate_audit_reservations_locked(
        &self,
        command_id: &str,
        ids: &[String],
    ) -> StoreResult<Vec<ActionAuditReservation>> {
        if command_id.trim().is_empty() || ids.is_empty() {
            return Err(StoreError::Conflict(
                "command id and at least one audit id are required".into(),
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut append = Vec::new();
        for id in ids {
            if id.trim().is_empty() || !seen.insert(id) {
                return Err(StoreError::Conflict(format!(
                    "audit reservation ids must be non-empty and unique: {id}"
                )));
            }
            if let Some(event) = self.find_by_id::<AuditEvent>(AUDIT_EVENTS, id)? {
                if event.action_command_id != command_id {
                    return Err(StoreError::Conflict(format!(
                        "audit id {id} already belongs to command {}",
                        event.action_command_id
                    )));
                }
            }
            match self.find_by_id::<ActionAuditReservation>(ACTION_AUDIT_RESERVATIONS, id)? {
                Some(existing) if existing.action_command_id != command_id => {
                    return Err(StoreError::Conflict(format!(
                        "audit id {id} is reserved by command {}",
                        existing.action_command_id
                    )));
                }
                Some(_) => {}
                None => append.push(ActionAuditReservation {
                    id: id.clone(),
                    action_command_id: command_id.to_string(),
                }),
            }
        }
        Ok(append)
    }

    fn validate_approval_transition(&self, value: &Approval) -> StoreResult<()> {
        let previous = self.find_by_id::<Approval>(APPROVALS, &value.id)?;
        match previous {
            None if value.status != ApprovalStatus::Requested => {
                return Err(StoreError::Conflict(
                    "new Approval must start in requested status".into(),
                ));
            }
            Some(previous) => {
                if previous.status == value.status
                    && previous.status != ApprovalStatus::Requested
                    && previous != *value
                {
                    return Err(StoreError::Conflict(format!(
                        "decided Approval is immutable: {}",
                        value.id
                    )));
                }
                if previous.subject_ref != value.subject_ref
                    || previous.action_summary != value.action_summary
                    || previous.requested_by != value.requested_by
                    || previous.required_approver_refs != value.required_approver_refs
                    || previous.required_actor_type != value.required_actor_type
                    || previous.policy_ref != value.policy_ref
                    || previous.requested_at != value.requested_at
                    || previous.expires_at != value.expires_at
                {
                    return Err(StoreError::Conflict(format!(
                        "immutable approval request changed: {}",
                        value.id
                    )));
                }
                if !approval_transition_allowed(previous.status, value.status) {
                    return Err(invalid_transition(
                        "Approval",
                        previous.status,
                        value.status,
                    ));
                }
            }
            None => {}
        }
        if value.status == ApprovalStatus::Requested
            && (!value.decided_by.is_empty()
                || value.decided_at.is_some()
                || value.decision_note.is_some())
        {
            return Err(StoreError::Conflict(
                "requested Approval cannot already contain a decision".into(),
            ));
        }
        if matches!(
            value.status,
            ApprovalStatus::Approved | ApprovalStatus::Rejected
        ) {
            let decided_at = value.decided_at.as_deref().ok_or_else(|| {
                StoreError::Conflict("decided Approval requires decided_at".into())
            })?;
            if value
                .expires_at
                .as_deref()
                .is_some_and(|expires_at| !timestamp_is_after(expires_at, decided_at))
            {
                return Err(StoreError::Conflict(format!(
                    "approval {} was decided after expiry",
                    value.id
                )));
            }
            if value
                .decided_by
                .iter()
                .any(|actor| !value.required_approver_refs.contains(actor))
            {
                return Err(StoreError::Conflict(format!(
                    "approval {} was decided by an actor outside required approvers",
                    value.id
                )));
            }
            if value.required_actor_type == Some(ActorType::Human)
                && !value
                    .decided_by
                    .iter()
                    .any(|actor| actor.actor_type == ActorType::Human)
            {
                return Err(StoreError::Conflict(format!(
                    "approval {} requires a Human decision",
                    value.id
                )));
            }
        }
        Ok(())
    }

    fn validate_commitment_transition(&self, value: &Commitment) -> StoreResult<()> {
        let previous = self.find_by_id::<Commitment>(COMMITMENTS, &value.id)?;
        match previous {
            None if value.status != CommitmentStatus::Proposed => {
                return Err(StoreError::Conflict(
                    "new Commitment must start in proposed status".into(),
                ));
            }
            Some(previous) => {
                if previous.status == value.status
                    && matches!(
                        previous.status,
                        CommitmentStatus::Fulfilled | CommitmentStatus::Cancelled
                    )
                    && previous != *value
                {
                    return Err(StoreError::Conflict(format!(
                        "terminal Commitment is immutable: {}",
                        value.id
                    )));
                }
                if previous.amount != value.amount
                    || previous.source_document_id != value.source_document_id
                    || previous.submitted_by != value.submitted_by
                    || previous.accountable_owner != value.accountable_owner
                    || previous.created_at != value.created_at
                {
                    return Err(StoreError::Conflict(format!(
                        "immutable Commitment fields changed: {}",
                        value.id
                    )));
                }
                if !commitment_transition_allowed(previous.status, value.status) {
                    return Err(invalid_transition(
                        "Commitment",
                        previous.status,
                        value.status,
                    ));
                }
            }
            None => {}
        }
        if matches!(
            value.status,
            CommitmentStatus::Approved | CommitmentStatus::Fulfilled
        ) {
            let subject = EntityRef {
                kind: EntityKind::FinancialRecord,
                id: value.id.clone(),
            };
            self.validate_approved_references(&value.approval_refs, Some(&subject), true)?;
        }
        Ok(())
    }

    fn validate_payment_transition(&self, value: &Payment) -> StoreResult<()> {
        if value.related_commitment_refs.is_empty() {
            return Err(StoreError::Conflict(
                "Payment requires at least one related Commitment".into(),
            ));
        }
        let previous = self.find_by_id::<Payment>(PAYMENTS, &value.id)?;
        match previous {
            None if value.status != PaymentStatus::Prepared => {
                return Err(StoreError::Conflict(
                    "new Payment must start in prepared status".into(),
                ));
            }
            Some(previous) => {
                if previous.status == value.status
                    && matches!(
                        previous.status,
                        PaymentStatus::Settled
                            | PaymentStatus::Failed
                            | PaymentStatus::Reversed
                            | PaymentStatus::Cancelled
                    )
                    && previous != *value
                {
                    return Err(StoreError::Conflict(format!(
                        "terminal Payment is immutable: {}",
                        value.id
                    )));
                }
                if previous.amount != value.amount
                    || previous.source_document_id != value.source_document_id
                    || previous.submitted_by != value.submitted_by
                    || previous.accountable_owner != value.accountable_owner
                    || previous.related_commitment_refs != value.related_commitment_refs
                    || previous.created_at != value.created_at
                {
                    return Err(StoreError::Conflict(format!(
                        "immutable Payment fields changed: {}",
                        value.id
                    )));
                }
                if !payment_transition_allowed(previous.status, value.status) {
                    return Err(invalid_transition("Payment", previous.status, value.status));
                }
            }
            None => {}
        }
        for commitment_id in &value.related_commitment_refs {
            let commitment = self
                .find_by_id::<Commitment>(COMMITMENTS, commitment_id)?
                .ok_or_else(|| missing("Commitment", commitment_id))?;
            if commitment.amount != value.amount
                || commitment.source_document_id != value.source_document_id
                || commitment.accountable_owner != value.accountable_owner
            {
                return Err(StoreError::Conflict(format!(
                    "Payment {} does not match Commitment {commitment_id}",
                    value.id
                )));
            }
        }
        if matches!(
            value.status,
            PaymentStatus::Processing | PaymentStatus::Settled | PaymentStatus::Reversed
        ) {
            let subject = EntityRef {
                kind: EntityKind::FinancialRecord,
                id: value.id.clone(),
            };
            self.validate_approved_references(&value.approval_refs, Some(&subject), true)?;
            if value.evidence_refs.is_empty() {
                return Err(StoreError::Conflict(format!(
                    "Payment {} requires evidence before execution",
                    value.id
                )));
            }
        }
        Ok(())
    }

    fn validate_action_request(&self, value: &ActionCommand) -> StoreResult<()> {
        self.require_entity(&value.subject_ref)?;
        self.require_authorized_actor_at(&value.requested_by, &value.requested_at)?;
        let policy = self
            .find_by_id::<ActionPolicyDefinition>(ACTION_POLICY_DEFINITIONS, &value.policy_ref)?
            .ok_or_else(|| missing("ActionPolicyDefinition", &value.policy_ref))?;
        if policy.command_name != value.command_name
            || policy.required_permission != value.required_permission
            || policy.risk_tier != value.risk_tier
            || policy.requires_human_approval != value.requires_human_approval
            || !policy
                .allowed_actor_kinds
                .contains(&value.requested_by.actor_type)
        {
            return Err(StoreError::Conflict(format!(
                "ActionCommand {} does not match server policy {}",
                value.id, policy.id
            )));
        }
        Ok(())
    }

    fn validate_action_transition_locked(
        &self,
        value: &ActionCommand,
        prospective_events: &[AuditEvent],
    ) -> StoreResult<bool> {
        let previous = self
            .find_by_id::<ActionCommand>(ACTION_COMMANDS, &value.id)?
            .ok_or_else(|| missing("ActionCommand", &value.id))?;
        if previous == *value {
            return Ok(false);
        }
        if !same_action_identity(&previous, value) {
            return Err(StoreError::Conflict(format!(
                "immutable action command request changed: {}",
                value.id
            )));
        }
        if !refs_only_extend(&previous.approval_refs, &value.approval_refs)
            || !refs_only_extend(&previous.audit_event_refs, &value.audit_event_refs)
        {
            return Err(StoreError::Conflict(format!(
                "action command governance references cannot be removed or rebound: {}",
                value.id
            )));
        }
        if previous.status == value.status
            && matches!(
                previous.status,
                ActionCommandStatus::Rejected
                    | ActionCommandStatus::Executed
                    | ActionCommandStatus::Failed
                    | ActionCommandStatus::Cancelled
            )
        {
            return Err(StoreError::Conflict(format!(
                "terminal ActionCommand is immutable: {}",
                value.id
            )));
        }
        if !action_transition_allowed(previous.status, value.status) {
            return Err(invalid_transition(
                "ActionCommand",
                previous.status,
                value.status,
            ));
        }
        self.validate_action_request(value)?;
        let requested_commitment_gate =
            self.validate_requested_commitment_approval_gate(value, &previous)?;
        if (value.requires_human_approval || !value.approval_refs.is_empty())
            && !requested_commitment_gate
        {
            self.validate_approved_references(
                &value.approval_refs,
                Some(&value.subject_ref),
                value.requires_human_approval,
            )?;
        }
        if matches!(
            value.status,
            ActionCommandStatus::Executed | ActionCommandStatus::Failed
        ) {
            for audit_id in &value.audit_event_refs {
                let event = prospective_events
                    .iter()
                    .find(|event| event.id == *audit_id)
                    .cloned()
                    .or(self.find_by_id::<AuditEvent>(AUDIT_EVENTS, audit_id)?)
                    .ok_or_else(|| missing("AuditEvent", audit_id))?;
                if event.action_command_id != value.id || event.subject_ref != value.subject_ref {
                    return Err(StoreError::Conflict(format!(
                        "audit event {audit_id} does not match action command {}",
                        value.id
                    )));
                }
            }
        }
        if value.status == ActionCommandStatus::Failed
            && (value.audit_event_refs.is_empty() || value.completed_at.is_none())
        {
            return Err(StoreError::Conflict(
                "failed ActionCommand requires an audit event and completed_at".into(),
            ));
        }
        Ok(true)
    }

    fn validate_requested_commitment_approval_gate(
        &self,
        command: &ActionCommand,
        previous_command: &ActionCommand,
    ) -> StoreResult<bool> {
        if command.command_name != "commitment.append" {
            return Ok(false);
        }
        let record = command.payload.get("record").ok_or_else(|| {
            StoreError::CompanyOsValidation("commitment.append payload.record is required".into())
        })?;
        let target: Commitment = serde_json::from_value(record.clone()).map_err(|error| {
            StoreError::CompanyOsValidation(format!(
                "commitment.append payload.record is invalid: {error}"
            ))
        })?;
        if target.status != CommitmentStatus::PendingApproval {
            return Ok(false);
        }
        let previous_commitment = self
            .find_by_id::<Commitment>(COMMITMENTS, &target.id)?
            .ok_or_else(|| missing("Commitment", &target.id))?;
        let entering_queue = previous_commitment.status == CommitmentStatus::Proposed;
        let finishing_same_queue_transition = previous_command.status
            == ActionCommandStatus::Authorized
            && previous_commitment == target;
        if !entering_queue && !finishing_same_queue_transition {
            return Ok(false);
        }
        if command.subject_ref
            != (EntityRef {
                kind: EntityKind::FinancialRecord,
                id: target.id.clone(),
            })
        {
            return Err(StoreError::Conflict(
                "commitment.append subject must match the pending Commitment".into(),
            ));
        }
        if command.approval_refs.is_empty() {
            return Err(StoreError::Conflict(
                "pending Commitment requires a Requested or Approved Human Approval".into(),
            ));
        }
        for approval_id in &command.approval_refs {
            let approval = self
                .find_by_id::<Approval>(APPROVALS, approval_id)?
                .ok_or_else(|| missing("Approval", approval_id))?;
            if !matches!(
                approval.status,
                ApprovalStatus::Requested | ApprovalStatus::Approved
            ) || approval.subject_ref != command.subject_ref
                || approval.policy_ref != command.policy_ref
                || approval.required_actor_type != Some(ActorType::Human)
                || approval.evidence_refs.is_empty()
                || !approval.action_summary.contains(&command.command_name)
                || approval.expires_at.as_deref().is_some_and(|expires_at| {
                    !timestamp_is_after(expires_at, &command.requested_at)
                })
            {
                return Err(StoreError::Conflict(format!(
                    "approval {approval_id} is not a matching evidence-backed Human queue gate"
                )));
            }
            let mut active_named_human = false;
            for actor in &approval.required_approver_refs {
                if actor.actor_type == ActorType::Human
                    && self
                        .authorized_actor_at(actor, &command.requested_at)
                        .is_ok()
                {
                    active_named_human = true;
                    break;
                }
            }
            if !active_named_human {
                return Err(StoreError::Conflict(format!(
                    "approval {approval_id} has no named active Human approver"
                )));
            }
        }
        Ok(true)
    }

    fn append_action_transition_with_events_atomic(
        &self,
        value: &ActionCommand,
        events: &[AuditEvent],
    ) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::CompanyOsValidation(error.to_string()))?;
        for event in events {
            event
                .validate()
                .map_err(|error| StoreError::CompanyOsValidation(error.to_string()))?;
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut seen = std::collections::BTreeSet::new();
        let mut append_events = Vec::new();
        for event in events {
            if !seen.insert(&event.id) {
                return Err(StoreError::Conflict(format!(
                    "duplicate audit event id in atomic transition: {}",
                    event.id
                )));
            }
            if event.action_command_id != value.id
                || event.subject_ref != value.subject_ref
                || !value.audit_event_refs.contains(&event.id)
            {
                return Err(StoreError::Conflict(format!(
                    "audit event {} does not belong to command {}",
                    event.id, value.id
                )));
            }
            self.require_audit_reservation_owner_if_reserved(&event.id, &event.action_command_id)?;
            self.require_authorized_actor_at(&event.actor_ref, &event.occurred_at)?;
            self.require_entity(&event.subject_ref)?;
            match self.find_by_id::<AuditEvent>(AUDIT_EVENTS, &event.id)? {
                Some(existing) if existing != *event => {
                    return Err(StoreError::Conflict(format!(
                        "audit event id is immutable: {}",
                        event.id
                    )));
                }
                Some(_) => {}
                None => append_events.push(event),
            }
        }
        let append_command = self.validate_action_transition_locked(value, events)?;
        for event in append_events {
            self.append_jsonl_unlocked(AUDIT_EVENTS, event)?;
        }
        if append_command {
            self.append_jsonl_unlocked(ACTION_COMMANDS, value)?;
        }
        Ok(())
    }

    fn validate_custom_page_definition_refs_locked(
        &self,
        value: &CustomPageDefinition,
    ) -> StoreResult<()> {
        self.require_id::<BusinessModule>(BUSINESS_MODULES, &value.module_id, "BusinessModule")?;
        self.require_id::<View>(VIEWS, &value.standard_view_fallback_ref, "View")?;
        self.require_actor(&value.owner)?;
        let package = self
            .find_by_id::<CustomPagePackage>(CUSTOM_PAGE_PACKAGES, &value.package_ref)?
            .ok_or_else(|| missing("CustomPagePackage", &value.package_ref))?;
        if package.definition_id != value.id || package.version != value.package_version {
            return Err(StoreError::Conflict(format!(
                "custom page definition {} does not match package {}",
                value.id, value.package_ref
            )));
        }
        Ok(())
    }

    fn validate_approved_references(
        &self,
        approval_ids: &[String],
        expected_subject: Option<&EntityRef>,
        require_human: bool,
    ) -> StoreResult<()> {
        if approval_ids.is_empty() {
            return Err(StoreError::Conflict(
                "an approved Human gate is required".into(),
            ));
        }
        let mut human_decision = false;
        for approval_id in approval_ids {
            let approval = self
                .find_by_id::<Approval>(APPROVALS, approval_id)?
                .ok_or_else(|| missing("Approval", approval_id))?;
            if approval.status != ApprovalStatus::Approved {
                return Err(StoreError::Conflict(format!(
                    "approval {approval_id} is not approved"
                )));
            }
            if expected_subject.is_some_and(|subject| approval.subject_ref != *subject) {
                return Err(StoreError::Conflict(format!(
                    "approval {approval_id} does not match the governed subject"
                )));
            }
            human_decision |= approval
                .decided_by
                .iter()
                .any(|actor| actor.actor_type == ActorType::Human);
        }
        if require_human && !human_decision {
            return Err(StoreError::Conflict(
                "approved references do not contain a Human decision".into(),
            ));
        }
        Ok(())
    }

    fn require_entity(&self, reference: &EntityRef) -> StoreResult<()> {
        // Evidence and execution ids are owned by their native stores. They
        // remain opaque references here instead of being falsely resolved via
        // a legacy or executor ledger.
        if matches!(reference.kind, EntityKind::Evidence | EntityKind::Execution) {
            return Ok(());
        }
        if !self.company_entity_exists(reference)? {
            return Err(missing(entity_kind(reference.kind), &reference.id));
        }
        Ok(())
    }
}

trait HasId {
    fn id(&self) -> &str;
}

macro_rules! impl_has_id {
    ($($type:ty),+ $(,)?) => {$ (
        impl HasId for $type {
            fn id(&self) -> &str { &self.id }
        }
    )+ };
}

impl_has_id!(
    Document,
    Block,
    TypedRecord,
    Relation,
    View,
    BusinessModule,
    HumanMember,
    StandingAgent,
    ExternalParticipant,
    ServiceActor,
    OrgUnit,
    OrganizationMembership,
    Milestone,
    WorkItem,
    Assignment,
    Approval,
    Commitment,
    Payment,
    CustomPageDefinition,
    CustomPagePackage,
    ActionCommand,
    ActionPolicyDefinition,
    AuditEvent,
    ActionAuditReservation,
);

fn missing(kind: &str, id: &str) -> StoreError {
    StoreError::CompanyOsMissingReference(format!("{kind}:{id}"))
}

fn actor_kind(kind: ActorType) -> &'static str {
    match kind {
        ActorType::Human => "HumanMember",
        ActorType::Agent => "StandingAgent",
        ActorType::External => "ExternalParticipant",
        ActorType::Service => "ServiceActor",
    }
}

fn entity_kind(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Actor => "Actor",
        EntityKind::Document => "Document",
        EntityKind::TypedRecord => "TypedRecord",
        EntityKind::BusinessModule => "BusinessModule",
        EntityKind::Milestone => "Milestone",
        EntityKind::WorkItem => "WorkItem",
        EntityKind::Approval => "Approval",
        EntityKind::FinancialRecord => "FinancialRecord",
        EntityKind::Evidence => "Evidence",
        EntityKind::Execution => "Execution",
    }
}

fn timestamp_is_after(left: &str, right: &str) -> bool {
    match (rfc3339_epoch_seconds(left), rfc3339_epoch_seconds(right)) {
        (Some(left), Some(right)) => left > right,
        _ => false,
    }
}

fn rfc3339_epoch_seconds(value: &str) -> Option<i64> {
    let (date, time_and_zone) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<i64>().ok()?;
    let day = date_parts.next()?.parse::<i64>().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (time, offset_seconds) = if let Some(time) = time_and_zone.strip_suffix('Z') {
        (time, 0_i64)
    } else {
        let zone_index = time_and_zone
            .char_indices()
            .rfind(|(index, character)| *index > 0 && matches!(character, '+' | '-'))?
            .0;
        let (time, zone) = time_and_zone.split_at(zone_index);
        let sign = if zone.starts_with('+') { 1_i64 } else { -1_i64 };
        let (hours, minutes) = zone[1..].split_once(':')?;
        let hours = hours.parse::<i64>().ok()?;
        let minutes = minutes.parse::<i64>().ok()?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        (time, sign * (hours * 3_600 + minutes * 60))
    };
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<i64>().ok()?;
    let minute = time_parts.next()?.parse::<i64>().ok()?;
    let second = time_parts.next()?.split('.').next()?.parse::<i64>().ok()?;
    if time_parts.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days_since_epoch = era * 146_097 + day_of_era - 719_468;
    Some(days_since_epoch * 86_400 + hour * 3_600 + minute * 60 + second - offset_seconds)
}

fn same_action_claim(left: &ActionCommand, right: &ActionCommand) -> bool {
    same_action_identity(left, right)
        && left.approval_refs == right.approval_refs
        && left.audit_event_refs == right.audit_event_refs
}

fn same_action_identity(left: &ActionCommand, right: &ActionCommand) -> bool {
    left.idempotency_key() == right.idempotency_key()
        && left.command_name == right.command_name
        && left.subject_ref == right.subject_ref
        && left.requested_by == right.requested_by
        && left.payload == right.payload
        && left.required_permission == right.required_permission
        && left.policy_ref == right.policy_ref
        && left.risk_tier == right.risk_tier
        && left.requires_human_approval == right.requires_human_approval
        && left.requested_at == right.requested_at
}

fn refs_only_extend(previous: &[String], next: &[String]) -> bool {
    previous.iter().all(|reference| next.contains(reference))
}

fn work_matches(item: &WorkItem, query: &WorkQuery) -> bool {
    (query.statuses.is_empty() || query.statuses.contains(&item.status))
        && (query.work_types.is_empty() || query.work_types.contains(&item.work_type))
        && (query.business_module_refs.is_empty()
            || item
                .business_module_ref
                .as_ref()
                .is_some_and(|id| query.business_module_refs.contains(id)))
        && (query.milestone_refs.is_empty()
            || item
                .milestone_ref
                .as_ref()
                .is_some_and(|id| query.milestone_refs.contains(id)))
        && query
            .accountable_owner
            .as_ref()
            .is_none_or(|actor| actor == &item.accountable_owner)
        && query
            .assignee
            .as_ref()
            .is_none_or(|actor| item.assignees.contains(actor))
}

fn work_item_is_active(status: WorkItemStatus) -> bool {
    !matches!(
        status,
        WorkItemStatus::Completed
            | WorkItemStatus::Cancelled
            | WorkItemStatus::Archived
            | WorkItemStatus::Draft
    )
}

fn work_status_key(status: WorkItemStatus) -> &'static str {
    match status {
        WorkItemStatus::Draft => "draft",
        WorkItemStatus::Submitted => "submitted",
        WorkItemStatus::Triaged => "triaged",
        WorkItemStatus::Accepted => "accepted",
        WorkItemStatus::InProgress => "in_progress",
        WorkItemStatus::WaitingForApproval => "waiting_for_approval",
        WorkItemStatus::Blocked => "blocked",
        WorkItemStatus::InReview => "in_review",
        WorkItemStatus::Completed => "completed",
        WorkItemStatus::Cancelled => "cancelled",
        WorkItemStatus::Archived => "archived",
    }
}

fn work_type_key(work_type: WorkType) -> &'static str {
    match work_type {
        WorkType::Development => "development",
        WorkType::Design => "design",
        WorkType::Research => "research",
        WorkType::Content => "content",
        WorkType::Legal => "legal",
        WorkType::Procurement => "procurement",
        WorkType::Finance => "finance",
        WorkType::Operations => "operations",
        WorkType::Governance => "governance",
        WorkType::HumanAction => "human_action",
        WorkType::General => "general",
    }
}

fn approval_transition_allowed(from: ApprovalStatus, to: ApprovalStatus) -> bool {
    from == to
        || matches!(
            (from, to),
            (
                ApprovalStatus::Requested,
                ApprovalStatus::Approved
                    | ApprovalStatus::Rejected
                    | ApprovalStatus::Expired
                    | ApprovalStatus::Cancelled
            )
        )
}

fn commitment_transition_allowed(from: CommitmentStatus, to: CommitmentStatus) -> bool {
    from == to
        || matches!(
            (from, to),
            (
                CommitmentStatus::Proposed,
                CommitmentStatus::PendingApproval
            ) | (CommitmentStatus::Proposed, CommitmentStatus::Cancelled)
                | (
                    CommitmentStatus::PendingApproval,
                    CommitmentStatus::Approved
                )
                | (
                    CommitmentStatus::PendingApproval,
                    CommitmentStatus::Cancelled
                )
                | (CommitmentStatus::Approved, CommitmentStatus::Fulfilled)
                | (CommitmentStatus::Approved, CommitmentStatus::Cancelled)
        )
}

fn payment_transition_allowed(from: PaymentStatus, to: PaymentStatus) -> bool {
    from == to
        || matches!(
            (from, to),
            (PaymentStatus::Prepared, PaymentStatus::PendingApproval)
                | (PaymentStatus::Prepared, PaymentStatus::Cancelled)
                | (PaymentStatus::PendingApproval, PaymentStatus::Processing)
                | (PaymentStatus::PendingApproval, PaymentStatus::Failed)
                | (PaymentStatus::PendingApproval, PaymentStatus::Cancelled)
                | (PaymentStatus::Processing, PaymentStatus::Settled)
                | (PaymentStatus::Processing, PaymentStatus::Failed)
                | (PaymentStatus::Settled, PaymentStatus::Reversed)
        )
}

fn action_transition_allowed(from: ActionCommandStatus, to: ActionCommandStatus) -> bool {
    from == to
        || matches!(
            (from, to),
            (
                ActionCommandStatus::Requested,
                ActionCommandStatus::Authorized
            ) | (
                ActionCommandStatus::Requested,
                ActionCommandStatus::Rejected
            ) | (
                ActionCommandStatus::Requested,
                ActionCommandStatus::Cancelled
            ) | (
                ActionCommandStatus::Authorized,
                ActionCommandStatus::Executed
            ) | (ActionCommandStatus::Authorized, ActionCommandStatus::Failed)
                | (
                    ActionCommandStatus::Authorized,
                    ActionCommandStatus::Cancelled
                )
        )
}

fn invalid_transition(kind: &str, from: impl fmt::Debug, to: impl fmt::Debug) -> StoreError {
    StoreError::Conflict(format!(
        "invalid {kind} status transition: {from:?} -> {to:?}"
    ))
}

impl fmt::Display for FinancialRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Commitment(record) => write!(formatter, "commitment:{}", record.id),
            Self::Payment(record) => write!(formatter, "payment:{}", record.id),
        }
    }
}
