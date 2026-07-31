use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use harness_core::{
    ActionCommand, ActionCommandStatus, ActionEffect, ActionPolicyDefinition, ActorRef, ActorType,
    AgentMember, AgentTeamRun, Approval, ApprovalStatus, Assignment, AssignmentDeliveryState,
    AuditEvent, AuditEventKind, Block, BlockKind, BusinessModule, Commitment, CommitmentStatus,
    CustomPageDefinition, CustomPagePackage, CustomPagePackageKind, DataQueryDeclaration,
    DeclaredAvailability, Document, DocumentKind, EntityKind, EntityRef, ExecutionMode,
    ExternalParticipant, HumanMember, LifecycleStatus, MemberRun, MemberStatus, Milestone,
    MilestoneStatus, ModuleCapability, Money, OrgUnit, OrgUnitStatus, OrganizationMembership,
    OrganizationMembershipRole, OrganizationMembershipStatus, Payment, PaymentStatus,
    PermissionModule, PermissionVerb, ProtectedEffect, Relation, RiskTier,
    SimpleActionPolicyDecision, SimpleDelegationError, SimplePermissionDecision,
    SimplePermissionDelegationRequest, SimplePermissionDenial, SimplePermissionExecutionBinding,
    SimplePermissionRequest, SimplePermissionState, SimpleRoleTemplate, StandingAgent, TeamMessage,
    TypedRecord, View, ViewMode, WorkItem, WorkItemStatus, WorkQuery, WorkType,
};
use harness_store::{
    ActionCommandClaimResult, CompanyActor, FinancialRecord, HarnessStore, StoreError,
};
use serde_json::json;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
const NOW: &str = "2026-07-20T10:00:00+08:00";

struct TestStore {
    root: PathBuf,
    store: HarnessStore,
}

impl TestStore {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "harness-company-os-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("initialize test store");
        Self { root, store }
    }
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn actor(actor_type: ActorType, actor_id: &str) -> ActorRef {
    ActorRef {
        actor_type,
        actor_id: actor_id.into(),
    }
}

fn human(id: &str) -> HumanMember {
    HumanMember {
        id: id.into(),
        display_name: "Brand Owner".into(),
        title: Some("Owner".into()),
        status: MemberStatus::Active,
        availability: None,
        membership_refs: vec![],
        responsibility_summary: "Accountable owner".into(),
        permission_policy_refs: vec![],
        authority_policy_refs: vec![],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

fn standing_agent(id: &str) -> StandingAgent {
    StandingAgent {
        id: id.into(),
        display_name: "Trademark Agent".into(),
        role: "trademark_agent".into(),
        execution_agent_member_ref: None,
        status: MemberStatus::Active,
        availability: DeclaredAvailability::Available,
        assignment_capacity: Some(2),
        exclusive_assignment_ref: None,
        membership_refs: vec![],
        responsibility_summary: "Prepare trademark filings".into(),
        capability_refs: vec![],
        system_prompt_ref: None,
        tool_refs: vec![],
        skill_refs: vec![],
        maintained_document_refs: vec![],
        accepted_work_type_refs: vec![],
        escalation_policy_ref: None,
        permission_policy_refs: vec![],
        runtime_refs: vec![],
        native_session_refs: vec![],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

#[test]
fn standing_agent_execution_identity_is_one_to_one() {
    let store = TestStore::new("standing-agent-execution-identity");
    let mut first = standing_agent("agent-one");
    first.execution_agent_member_ref = Some("execution-member".into());
    store.store.append_standing_agent(&first).unwrap();

    let mut duplicate = standing_agent("agent-two");
    duplicate.execution_agent_member_ref = Some("execution-member".into());
    let error = store
        .store
        .append_standing_agent(&duplicate)
        .expect_err("duplicate execution identity must fail");
    assert!(error.to_string().contains("relation must be one-to-one"));
}

#[test]
fn relinking_a_standing_agent_reuses_the_latest_row_and_frees_the_released_member() {
    let store = TestStore::new("standing-agent-execution-relink");
    let mut owner = standing_agent("agent-one");
    owner.execution_agent_member_ref = Some("execution-member".into());
    store.store.append_standing_agent(&owner).unwrap();

    // Re-appending the SAME agent with the same reference must not collide with
    // itself: the one-to-one guard is scoped to other Standing Agents.
    let mut refreshed = owner.clone();
    refreshed.responsibility_summary = "Prepare trademark filings and renewals".into();
    store.store.append_standing_agent(&refreshed).unwrap();

    // While the reference is held, a second agent still cannot steal it.
    let mut rival = standing_agent("agent-two");
    rival.execution_agent_member_ref = Some("execution-member".into());
    store
        .store
        .append_standing_agent(&rival)
        .expect_err("a held execution member must not be stealable");

    // Unlinking releases it, and only then may another agent claim it.
    let mut released = refreshed.clone();
    released.execution_agent_member_ref = None;
    store.store.append_standing_agent(&released).unwrap();
    store.store.append_standing_agent(&rival).unwrap();

    let latest = store.store.latest_standing_agents().unwrap();
    let projected = latest
        .iter()
        .map(|agent| {
            (
                agent.id.as_str(),
                agent.execution_agent_member_ref.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        projected,
        vec![("agent-one", None), ("agent-two", Some("execution-member"))],
        "latest-row-wins must reflect the unlink and the subsequent claim"
    );
    assert_eq!(
        latest
            .iter()
            .find(|agent| agent.id == "agent-one")
            .unwrap()
            .responsibility_summary,
        "Prepare trademark filings and renewals",
        "unrelated fields must survive the unlink re-append"
    );
}

fn document(id: &str, owner: &ActorRef) -> Document {
    Document {
        id: id.into(),
        space_id: "legal".into(),
        parent_document_id: None,
        title: "Trademark application CN-2026-018".into(),
        kind: DocumentKind::Page,
        lifecycle_status: LifecycleStatus::Active,
        block_ids: vec![],
        template_ref: None,
        permission_policy_refs: vec![],
        reference_refs: vec![],
        created_by: owner.clone(),
        updated_by: owner.clone(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

fn business_module(id: &str, document_id: &str, owner: &ActorRef) -> BusinessModule {
    BusinessModule {
        id: id.into(),
        name: "Trademark Management".into(),
        purpose: "Govern trademark applications".into(),
        root_document_ref: document_id.into(),
        record_types: vec!["trademark_application".into()],
        relation_rules: vec![],
        default_view_refs: vec![],
        policy_refs: vec![],
        lifecycle_rules: vec![],
        metric_definition_refs: vec![],
        custom_page_definition_refs: vec![],
        status: LifecycleStatus::Active,
        owner: owner.clone(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

fn work_item(id: &str, document_id: &str, human: &ActorRef, agent: &ActorRef) -> WorkItem {
    WorkItem {
        id: id.into(),
        title: "Trademark filing for Brand A".into(),
        objective: "Prepare and submit a governed filing package".into(),
        description: Some(
            "Prepare the package, preserve approval context, and return evidence.".into(),
        ),
        acceptance_criteria: vec!["Filing evidence is linked".into()],
        context_refs: vec![],
        deliverable_refs: vec![],
        status: WorkItemStatus::Submitted,
        source_document_ref: document_id.into(),
        source_record_refs: vec![],
        milestone_ref: None,
        work_type: WorkType::Legal,
        business_module_ref: None,
        result_document_ref: None,
        result_record_refs: vec![],
        submitted_by: agent.clone(),
        requested_by: Some(human.clone()),
        accountable_owner: human.clone(),
        assignees: vec![agent.clone()],
        contributors: vec![],
        reviewer: None,
        approver: Some(human.clone()),
        execution_mode: ExecutionMode::Direct,
        execution_refs: vec![],
        approval_refs: vec![],
        evidence_refs: vec![],
        artifact_refs: vec![],
        outcome_summary: None,
        due_at: None,
        priority: Some("high".into()),
        risk_level: Some("legal".into()),
        created_at: NOW.into(),
        updated_at: NOW.into(),
        completed_at: None,
    }
}

fn seed_people_and_document(store: &HarnessStore) -> (ActorRef, ActorRef, String) {
    store
        .append_human_member(&human("human-brand-owner"))
        .unwrap();
    store
        .append_standing_agent(&standing_agent("agent-trademark"))
        .unwrap();
    let human_ref = actor(ActorType::Human, "human-brand-owner");
    let agent_ref = actor(ActorType::Agent, "agent-trademark");
    let document_id = "doc-cn-2026-018".to_string();
    store
        .append_document(&document(&document_id, &human_ref))
        .unwrap();
    (human_ref, agent_ref, document_id)
}

#[test]
fn native_work_projection_preserves_milestone_type_and_business_line_truth() {
    let test = TestStore::new("work-projection");
    let (human_ref, agent_ref, document_id) = seed_people_and_document(&test.store);
    let module = business_module("module-trademark", &document_id, &human_ref);
    test.store.append_business_module(&module).unwrap();
    let milestone = Milestone {
        id: "milestone-trademark-submitted".into(),
        title: "Trademark application submitted".into(),
        outcome: "A governed CN filing has durable receipt evidence".into(),
        status: MilestoneStatus::Active,
        accountable_owner: human_ref.clone(),
        source_document_ref: Some(document_id.clone()),
        business_module_ref: Some(module.id.clone()),
        target_at: Some("2026-07-31T00:00:00+08:00".into()),
        acceptance_criteria: vec!["Filing receipt is linked".into()],
        work_item_refs: vec![],
        created_at: NOW.into(),
        updated_at: NOW.into(),
        achieved_at: None,
    };
    test.store.append_milestone(&milestone).unwrap();
    let mut work = work_item(
        "work-trademark-filing",
        &document_id,
        &human_ref,
        &agent_ref,
    );
    work.milestone_ref = Some(milestone.id.clone());
    work.business_module_ref = Some(module.id.clone());
    work.work_type = WorkType::Legal;
    work.status = WorkItemStatus::Blocked;
    test.store.append_work_item(&work).unwrap();

    let projection = test.store.work_projection(&WorkQuery::default()).unwrap();
    assert_eq!(projection.summary.total, 1);
    assert_eq!(projection.summary.blocked, 1);
    assert_eq!(projection.summary.without_milestone, 0);
    assert_eq!(projection.board["blocked"], vec![work.id.clone()]);
    assert_eq!(projection.business_lines[&module.id], vec![work.id.clone()]);
    assert_eq!(projection.work_types["legal"], vec![work.id.clone()]);
    assert_eq!(projection.milestones[0].total_work_items, 1);
    assert_eq!(projection.milestones[0].blocked_work_items, 1);
    assert_eq!(projection.workload.len(), 2);

    let content_module = business_module("module-content", &document_id, &human_ref);
    test.store.append_business_module(&content_module).unwrap();
    let mut content_work = work_item("work-publish-video", &document_id, &human_ref, &agent_ref);
    content_work.title = "Publish launch video".into();
    content_work.work_type = WorkType::Content;
    content_work.business_module_ref = Some(content_module.id.clone());
    content_work.status = WorkItemStatus::InProgress;
    test.store.append_work_item(&content_work).unwrap();
    let content_projection = test
        .store
        .work_projection(&WorkQuery {
            business_module_refs: vec![content_module.id.clone()],
            ..WorkQuery::default()
        })
        .unwrap();
    assert_eq!(content_projection.summary.total, 1);
    assert_eq!(content_projection.work_items[0].id, content_work.id);
    assert_eq!(
        content_projection.work_types["content"],
        vec![content_work.id]
    );

    let filtered = test
        .store
        .work_projection(&WorkQuery {
            work_types: vec![WorkType::Development],
            ..WorkQuery::default()
        })
        .unwrap();
    assert_eq!(filtered.summary.total, 0);
}

fn requested_approval(
    id: &str,
    subject_ref: EntityRef,
    requester: &ActorRef,
    human_approver: &ActorRef,
) -> Approval {
    Approval {
        id: id.into(),
        subject_ref,
        action_summary: "Authorize governed effect".into(),
        requested_by: requester.clone(),
        required_approver_refs: vec![human_approver.clone()],
        required_actor_type: Some(ActorType::Human),
        policy_ref: "policy-human-gate".into(),
        status: ApprovalStatus::Requested,
        decided_by: vec![],
        decision_note: None,
        evidence_refs: vec![],
        requested_at: NOW.into(),
        decided_at: None,
        expires_at: Some("2026-07-21T10:00:00+08:00".into()),
    }
}

fn approved(mut value: Approval, human_approver: &ActorRef) -> Approval {
    value.status = ApprovalStatus::Approved;
    value.decided_by = vec![human_approver.clone()];
    value.decision_note = Some("Human approved".into());
    value.decided_at = Some("2026-07-20T11:00:00+08:00".into());
    value
}

fn seed_action_policy(
    store: &HarnessStore,
) -> (ActorRef, ActorRef, String, ActionPolicyDefinition) {
    let (human_ref, agent_ref, document_id) = seed_people_and_document(store);
    let module = business_module("module-trademark", &document_id, &human_ref);
    store.append_business_module(&module).unwrap();
    let fallback = View {
        id: "view-action-fallback".into(),
        module_id: Some(module.id.clone()),
        title: "Governed records".into(),
        mode: ViewMode::Table,
        source_kinds: vec![EntityKind::TypedRecord],
        query: json!({}),
        owner: human_ref.clone(),
        policy_refs: vec![],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    store.append_view(&fallback).unwrap();
    let package = CustomPagePackage {
        id: "package-action-page".into(),
        definition_id: "definition-action-page".into(),
        version: "1.0.0".into(),
        kind: CustomPagePackageKind::React,
        artifact_ref: "artifact://action-page".into(),
        entrypoint: "index.js".into(),
        integrity_digest: "sha256:action".into(),
        built_at: NOW.into(),
    };
    store.append_custom_page_package(&package).unwrap();
    let definition = CustomPageDefinition {
        id: package.definition_id.clone(),
        module_id: module.id.clone(),
        purpose: "Governed action page".into(),
        allowed_data_queries: vec![DataQueryDeclaration {
            id: "query-action-subject".into(),
            source_kind: EntityKind::Document,
            source_scope: format!("document:{document_id}"),
            permission_policy_ref: "policy-action-read".into(),
        }],
        approved_ui_components: vec!["ActionButton".into()],
        action_command_refs: vec!["policy-update-trademark".into()],
        standard_view_fallback_ref: fallback.id,
        owner: human_ref.clone(),
        package_ref: package.id,
        package_version: package.version,
        fixture_ref: "fixture://action".into(),
        visual_contract_ref: "visual://action".into(),
        policy_refs: vec!["policy-action-read".into()],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    store.append_custom_page_definition(&definition).unwrap();
    let policy = ActionPolicyDefinition {
        id: "policy-update-trademark".into(),
        module_ref: module.id,
        definition_ref: definition.id,
        command_name: "update_trademark".into(),
        required_permission: "trademark:update".into(),
        risk_tier: RiskTier::R1,
        requires_human_approval: false,
        allowed_actor_kinds: vec![ActorType::Agent],
        allowed_effects: vec![ActionEffect::UpdateRecord],
    };
    store.append_action_policy_definition(&policy).unwrap();
    (human_ref, agent_ref, document_id, policy)
}

#[test]
fn append_only_ledgers_project_latest_rows_and_preserve_company_links() {
    let test = TestStore::new("latest");
    let (human_ref, agent_ref, document_id) = seed_people_and_document(&test.store);

    let block = Block {
        id: "block-overview".into(),
        document_id: document_id.clone(),
        kind: BlockKind::RichText,
        position: 0,
        content: json!({"text": "Application overview"}),
        referenced_entities: vec![],
        created_by: human_ref.clone(),
        updated_by: human_ref.clone(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_block(&block).unwrap();

    let mut updated_document = document(&document_id, &human_ref);
    updated_document.title = "Trademark application CN-2026-018 — reviewed".into();
    updated_document.block_ids = vec![block.id.clone()];
    test.store.append_document(&updated_document).unwrap();

    let module = business_module("module-trademark", &document_id, &human_ref);
    test.store.append_business_module(&module).unwrap();
    let record = TypedRecord {
        id: "record-cn-2026-018".into(),
        module_id: module.id.clone(),
        record_type: "trademark_application".into(),
        title: "CN-2026-018".into(),
        fields: json!({"jurisdiction": "CN"}),
        lifecycle_status: "draft".into(),
        source_document_ref: Some(document_id.clone()),
        created_by: agent_ref.clone(),
        updated_by: agent_ref.clone(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_typed_record(&record).unwrap();
    let relation = Relation {
        id: "relation-document-record".into(),
        from_ref: EntityRef {
            kind: EntityKind::Document,
            id: document_id.clone(),
        },
        relation_type: "describes".into(),
        to_ref: EntityRef {
            kind: EntityKind::TypedRecord,
            id: record.id.clone(),
        },
        provenance_ref: Some(EntityRef {
            kind: EntityKind::Document,
            id: document_id.clone(),
        }),
        lifecycle_status: None,
        created_by: agent_ref.clone(),
        created_at: NOW.into(),
    };
    test.store.append_relation(&relation).unwrap();
    let view = View {
        id: "view-trademark-table".into(),
        module_id: Some(module.id.clone()),
        title: "Trademark applications".into(),
        mode: ViewMode::Table,
        source_kinds: vec![EntityKind::TypedRecord],
        query: json!({"record_type": "trademark_application"}),
        owner: human_ref.clone(),
        policy_refs: vec![],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_view(&view).unwrap();
    let mut updated_module = module;
    updated_module.default_view_refs = vec![view.id.clone()];
    test.store.append_business_module(&updated_module).unwrap();

    let org_unit = OrgUnit {
        id: "org-legal".into(),
        organization_id: "company".into(),
        name: "Legal".into(),
        purpose: "Own legal operations".into(),
        parent_unit_id: None,
        status: OrgUnitStatus::Active,
        human_lead_actor_ref: Some(human_ref.clone()),
        agent_lead_actor_ref: None,
        policy_refs: vec![],
        document_space_ref: Some("legal".into()),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_org_unit(&org_unit).unwrap();
    test.store
        .append_organization_membership(&OrganizationMembership {
            id: "membership-owner-legal".into(),
            organization_id: "company".into(),
            org_unit_id: org_unit.id,
            actor_ref: human_ref.clone(),
            membership_role: OrganizationMembershipRole::Lead,
            title_or_function: Some("Brand Owner".into()),
            status: OrganizationMembershipStatus::Active,
            starts_at: NOW.into(),
            ends_at: None,
            authority_policy_refs: vec![],
            created_by_actor_ref: human_ref.clone(),
            created_at: NOW.into(),
        })
        .unwrap();

    let work = work_item(
        "work-trademark-filing",
        &document_id,
        &human_ref,
        &agent_ref,
    );
    test.store.append_work_item(&work).unwrap();
    test.store
        .append_assignment(&Assignment {
            id: "assignment-trademark-agent".into(),
            work_item_id: work.id,
            recipient: agent_ref,
            sender: human_ref,
            assigned_role: "preparer".into(),
            scope: Some("Prepare filing package".into()),
            delivery_state: AssignmentDeliveryState::Pending,
            delivery_policy_ref: "policy-assignment".into(),
            correlation_id: "corr-cn-2026-018".into(),
            delivery_evidence_ref: None,
            assigned_at: NOW.into(),
            delivered_at: None,
            acknowledged_at: None,
        })
        .unwrap();

    assert_eq!(test.store.documents().unwrap().len(), 2);
    assert_eq!(
        test.store.latest_documents().unwrap(),
        vec![updated_document]
    );
    assert_eq!(test.store.latest_blocks().unwrap(), vec![block]);
    assert_eq!(test.store.latest_typed_records().unwrap(), vec![record]);
    assert_eq!(test.store.latest_relations().unwrap(), vec![relation]);
    assert_eq!(test.store.latest_views().unwrap(), vec![view]);
    assert_eq!(
        test.store.latest_business_modules().unwrap(),
        vec![updated_module]
    );
    assert_eq!(
        test.store.latest_organization_memberships().unwrap().len(),
        1
    );
    assert_eq!(test.store.latest_assignments().unwrap().len(), 1);
    assert_eq!(test.store.actors().unwrap().len(), 2);
    assert!(matches!(
        test.store.latest_actors().unwrap()[0],
        CompanyActor::Agent(_) | CompanyActor::Human(_)
    ));
}

#[test]
fn append_rejects_missing_or_wrong_kind_references() {
    let test = TestStore::new("missing-ref");
    test.store
        .append_human_member(&human("human-brand-owner"))
        .unwrap();
    let owner = actor(ActorType::Human, "human-brand-owner");
    let block = Block {
        id: "orphan-block".into(),
        document_id: "missing-document".into(),
        kind: BlockKind::RichText,
        position: 0,
        content: json!({}),
        referenced_entities: vec![],
        created_by: owner.clone(),
        updated_by: owner,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    assert!(matches!(
        test.store.append_block(&block),
        Err(StoreError::CompanyOsMissingReference(message)) if message == "Document:missing-document"
    ));
    assert!(test.store.blocks().unwrap().is_empty());
}

#[test]
fn approved_human_boundary_requires_a_real_human_actor() {
    let test = TestStore::new("human-approval");
    let (human_ref, agent_ref, document_id) = seed_people_and_document(&test.store);
    let subject = EntityRef {
        kind: EntityKind::Document,
        id: document_id,
    };

    let requested = Approval {
        id: "approval-human-required".into(),
        subject_ref: subject.clone(),
        action_summary: "Authorize legal filing".into(),
        requested_by: agent_ref.clone(),
        required_approver_refs: vec![human_ref.clone()],
        required_actor_type: Some(ActorType::Human),
        policy_ref: "policy-human-legal".into(),
        status: ApprovalStatus::Requested,
        decided_by: vec![],
        decision_note: None,
        evidence_refs: vec![],
        requested_at: NOW.into(),
        decided_at: None,
        expires_at: None,
    };
    test.store.append_approval(&requested).unwrap();
    let agent_decision = Approval {
        status: ApprovalStatus::Approved,
        decided_by: vec![agent_ref.clone()],
        decision_note: Some("Agent attempted approval".into()),
        decided_at: Some(NOW.into()),
        ..requested.clone()
    };
    assert!(matches!(
        test.store.append_approval(&agent_decision),
        Err(StoreError::CompanyOsValidation(_))
    ));

    let mut forged_human = agent_decision.clone();
    forged_human.decided_by = vec![actor(ActorType::Human, "agent-trademark")];
    assert!(matches!(
        test.store.append_approval(&forged_human),
        Err(StoreError::Conflict(_)) | Err(StoreError::CompanyOsMissingReference(_))
    ));

    let valid = Approval {
        decided_by: vec![human_ref],
        decision_note: Some("Brand Owner approved filing".into()),
        ..agent_decision
    };
    test.store.append_approval(&valid).unwrap();
    assert_eq!(test.store.latest_approvals().unwrap(), vec![valid]);
}

#[test]
fn commitment_and_payment_are_separate_explicit_ledgers() {
    let test = TestStore::new("finance-separation");
    let (human_ref, agent_ref, document_id) = seed_people_and_document(&test.store);
    let proposed = Commitment {
        id: "commitment-cn-2026-018".into(),
        amount: Money {
            amount: "3000".into(),
            currency: "CNY".into(),
        },
        status: CommitmentStatus::Proposed,
        source_document_id: document_id.clone(),
        submitted_by: agent_ref.clone(),
        accountable_owner: human_ref.clone(),
        relation_ids: vec![],
        evidence_refs: vec![],
        approval_refs: vec![],
        audit_event_ids: vec![],
        due_at: None,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_commitment(&proposed).unwrap();
    let commitment = Commitment {
        status: CommitmentStatus::PendingApproval,
        ..proposed
    };
    test.store.append_commitment(&commitment).unwrap();

    assert_eq!(
        test.store.latest_commitments().unwrap(),
        vec![commitment.clone()]
    );
    assert!(test.store.payments().unwrap().is_empty());
    assert_eq!(
        test.store.financial_records().unwrap(),
        vec![
            FinancialRecord::Commitment(Commitment {
                status: CommitmentStatus::Proposed,
                ..commitment.clone()
            }),
            FinancialRecord::Commitment(commitment.clone()),
        ]
    );
    assert_eq!(
        test.store.latest_financial_records().unwrap(),
        vec![FinancialRecord::Commitment(commitment.clone())]
    );

    let payment = Payment {
        id: "payment-cn-2026-018".into(),
        amount: commitment.amount.clone(),
        status: PaymentStatus::Prepared,
        source_document_id: document_id,
        submitted_by: agent_ref,
        accountable_owner: human_ref,
        related_commitment_refs: vec![commitment.id],
        relation_ids: vec![],
        evidence_refs: vec![],
        approval_refs: vec![],
        audit_event_ids: vec![],
        occurred_at: None,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_payment(&payment).unwrap();
    assert_eq!(test.store.latest_payments().unwrap(), vec![payment]);
    assert_eq!(test.store.latest_financial_records().unwrap().len(), 2);
}

#[test]
fn custom_page_requires_a_real_standard_view_fallback_and_matching_package() {
    let test = TestStore::new("custom-fallback");
    let (human_ref, _, document_id) = seed_people_and_document(&test.store);
    let module = business_module("module-trademark", &document_id, &human_ref);
    test.store.append_business_module(&module).unwrap();
    let fallback = View {
        id: "view-trademark-standard".into(),
        module_id: Some(module.id.clone()),
        title: "Trademark standard table".into(),
        mode: ViewMode::Table,
        source_kinds: vec![EntityKind::TypedRecord],
        query: json!({}),
        owner: human_ref.clone(),
        policy_refs: vec![],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_view(&fallback).unwrap();
    let package = CustomPagePackage {
        id: "package-trademark-v1".into(),
        definition_id: "page-trademark".into(),
        version: "1.0.0".into(),
        kind: CustomPagePackageKind::React,
        artifact_ref: "artifact://trademark-v1".into(),
        entrypoint: "index.js".into(),
        integrity_digest: "sha256:abc123".into(),
        built_at: NOW.into(),
    };
    test.store.append_custom_page_package(&package).unwrap();
    let definition = CustomPageDefinition {
        id: "page-trademark".into(),
        module_id: module.id,
        purpose: "Combine legal, work, approval, and finance context".into(),
        allowed_data_queries: vec![DataQueryDeclaration {
            id: "query-applications".into(),
            source_kind: EntityKind::TypedRecord,
            source_scope: "module:module-trademark".into(),
            permission_policy_ref: "policy-trademark-read".into(),
        }],
        approved_ui_components: vec!["RecordTable".into()],
        action_command_refs: vec![],
        standard_view_fallback_ref: "missing-view".into(),
        owner: human_ref,
        package_ref: package.id.clone(),
        package_version: package.version.clone(),
        fixture_ref: "fixture://company-os-trademark-v1".into(),
        visual_contract_ref: "visual://company-os-v1".into(),
        policy_refs: vec![],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    assert!(matches!(
        test.store.append_custom_page_definition(&definition),
        Err(StoreError::CompanyOsMissingReference(message)) if message == "View:missing-view"
    ));

    let valid = CustomPageDefinition {
        standard_view_fallback_ref: fallback.id,
        ..definition
    };
    test.store.append_custom_page_definition(&valid).unwrap();
    assert_eq!(
        test.store.latest_custom_page_definitions().unwrap(),
        vec![valid]
    );
    assert_eq!(
        test.store.latest_custom_page_packages().unwrap(),
        vec![package]
    );
}

#[test]
fn approval_is_monotonic_human_decided_and_not_expired() {
    let test = TestStore::new("approval-transition");
    let (human_ref, agent_ref, document_id) = seed_people_and_document(&test.store);
    let subject = EntityRef {
        kind: EntityKind::Document,
        id: document_id,
    };
    let requested = requested_approval(
        "approval-transition",
        subject.clone(),
        &agent_ref,
        &human_ref,
    );
    test.store.append_approval(&requested).unwrap();
    let accepted = approved(requested.clone(), &human_ref);
    test.store.append_approval(&accepted).unwrap();

    assert!(matches!(
        test.store.append_approval(&requested),
        Err(StoreError::Conflict(message)) if message.contains("status transition")
    ));

    let mut expiring = requested_approval("approval-expired", subject, &agent_ref, &human_ref);
    expiring.expires_at = Some("2026-07-20T10:30:00+08:00".into());
    test.store.append_approval(&expiring).unwrap();
    let expired_decision = approved(expiring, &human_ref);
    assert!(matches!(
        test.store.append_approval(&expired_decision),
        Err(StoreError::Conflict(message)) if message.contains("after expiry")
    ));
}

#[test]
fn finance_transitions_reject_mutation_rejected_approval_and_unmatched_payment() {
    let test = TestStore::new("finance-gates");
    let (human_ref, agent_ref, document_id) = seed_people_and_document(&test.store);
    let proposed = Commitment {
        id: "commitment-governed".into(),
        amount: Money {
            amount: "3000".into(),
            currency: "CNY".into(),
        },
        status: CommitmentStatus::Proposed,
        source_document_id: document_id.clone(),
        submitted_by: agent_ref.clone(),
        accountable_owner: human_ref.clone(),
        relation_ids: vec![],
        evidence_refs: vec![],
        approval_refs: vec![],
        audit_event_ids: vec![],
        due_at: None,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_commitment(&proposed).unwrap();
    let pending = Commitment {
        status: CommitmentStatus::PendingApproval,
        ..proposed.clone()
    };
    test.store.append_commitment(&pending).unwrap();

    let subject = EntityRef {
        kind: EntityKind::FinancialRecord,
        id: proposed.id.clone(),
    };
    let requested = requested_approval(
        "approval-commitment-rejected",
        subject,
        &agent_ref,
        &human_ref,
    );
    test.store.append_approval(&requested).unwrap();
    let mut rejected = requested;
    rejected.status = ApprovalStatus::Rejected;
    rejected.decided_by = vec![human_ref.clone()];
    rejected.decision_note = Some("Rejected by owner".into());
    rejected.decided_at = Some("2026-07-20T11:00:00+08:00".into());
    test.store.append_approval(&rejected).unwrap();
    let should_not_approve = Commitment {
        status: CommitmentStatus::Approved,
        approval_refs: vec![rejected.id],
        ..pending.clone()
    };
    assert!(matches!(
        test.store.append_commitment(&should_not_approve),
        Err(StoreError::Conflict(message)) if message.contains("is not approved")
    ));

    let mutated = Commitment {
        amount: Money {
            amount: "3001".into(),
            currency: "CNY".into(),
        },
        ..pending.clone()
    };
    assert!(matches!(
        test.store.append_commitment(&mutated),
        Err(StoreError::Conflict(message)) if message.contains("immutable Commitment")
    ));

    let unmatched_payment = Payment {
        id: "payment-unmatched".into(),
        amount: Money {
            amount: "2999".into(),
            currency: "CNY".into(),
        },
        status: PaymentStatus::Prepared,
        source_document_id: document_id,
        submitted_by: agent_ref,
        accountable_owner: human_ref,
        related_commitment_refs: vec![proposed.id.clone()],
        relation_ids: vec![],
        evidence_refs: vec![],
        approval_refs: vec![],
        audit_event_ids: vec![],
        occurred_at: None,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    assert!(matches!(
        test.store.append_payment(&unmatched_payment),
        Err(StoreError::Conflict(message)) if message.contains("does not match Commitment")
    ));

    let prepared = Payment {
        id: "payment-governed".into(),
        amount: proposed.amount.clone(),
        status: PaymentStatus::Prepared,
        source_document_id: proposed.source_document_id.clone(),
        submitted_by: proposed.submitted_by.clone(),
        accountable_owner: proposed.accountable_owner.clone(),
        related_commitment_refs: vec![proposed.id.clone()],
        relation_ids: vec![],
        evidence_refs: vec![],
        approval_refs: vec![],
        audit_event_ids: vec![],
        occurred_at: None,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_payment(&prepared).unwrap();
    let illegal_jump = Payment {
        status: PaymentStatus::Settled,
        evidence_refs: vec!["evidence-settlement".into()],
        approval_refs: vec!["approval-placeholder".into()],
        occurred_at: Some("2026-07-20T12:00:00+08:00".into()),
        ..prepared.clone()
    };
    assert!(matches!(
        test.store.append_payment(&illegal_jump),
        Err(StoreError::Conflict(message)) if message.contains("status transition")
    ));

    let payment_subject = EntityRef {
        kind: EntityKind::FinancialRecord,
        id: prepared.id.clone(),
    };
    let payment_approval = requested_approval(
        "approval-payment-rejected",
        payment_subject,
        &proposed.submitted_by,
        &proposed.accountable_owner,
    );
    test.store.append_approval(&payment_approval).unwrap();
    let mut payment_rejected = payment_approval;
    payment_rejected.status = ApprovalStatus::Rejected;
    payment_rejected.decided_by = vec![proposed.accountable_owner.clone()];
    payment_rejected.decision_note = Some("Payment rejected".into());
    payment_rejected.decided_at = Some("2026-07-20T11:00:00+08:00".into());
    test.store.append_approval(&payment_rejected).unwrap();
    let pending_payment = Payment {
        status: PaymentStatus::PendingApproval,
        ..prepared
    };
    test.store.append_payment(&pending_payment).unwrap();
    let cannot_process = Payment {
        status: PaymentStatus::Processing,
        approval_refs: vec![payment_rejected.id],
        evidence_refs: vec!["evidence-preparation".into()],
        ..pending_payment
    };
    assert!(matches!(
        test.store.append_payment(&cannot_process),
        Err(StoreError::Conflict(message)) if message.contains("is not approved")
    ));
}

#[test]
fn inactive_and_expired_actors_cannot_be_authorized() {
    let test = TestStore::new("actor-authority");
    test.store
        .append_human_member(&human("human-sponsor"))
        .unwrap();
    let sponsor = actor(ActorType::Human, "human-sponsor");
    let external = ExternalParticipant {
        id: "external-lawyer".into(),
        display_name_or_organization: "External Lawyer".into(),
        engagement_scope: "Trademark filing review".into(),
        sponsor_actor_ref: sponsor.clone(),
        access_expires_at: "2026-07-19T10:00:00+08:00".into(),
        confidentiality_or_contract_refs: vec![],
        membership_refs: vec![],
        restricted_permission_refs: vec![],
        status: MemberStatus::Active,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_external_participant(&external).unwrap();
    assert!(matches!(
        test.store.authorized_actor_at(
            &actor(ActorType::External, "external-lawyer"),
            NOW,
        ),
        Err(StoreError::Conflict(message)) if message.contains("not authorized")
    ));

    let mut paused = human("human-sponsor");
    paused.status = MemberStatus::Paused;
    test.store.append_human_member(&paused).unwrap();
    assert!(matches!(
        test.store.authorized_actor_at(&sponsor, NOW),
        Err(StoreError::Conflict(message)) if message.contains("not authorized")
    ));
}

#[test]
fn action_command_claim_is_atomic_idempotent_and_audited() {
    let test = TestStore::new("action-command");
    let (_, agent_ref, document_id, policy) = seed_action_policy(&test.store);
    let subject = EntityRef {
        kind: EntityKind::Document,
        id: document_id,
    };
    let requested = ActionCommand {
        id: "command-update-cn-2026-018".into(),
        command_name: policy.command_name.clone(),
        subject_ref: subject.clone(),
        requested_by: agent_ref.clone(),
        payload: json!({"status": "ready_for_review"}),
        required_permission: policy.required_permission.clone(),
        policy_ref: policy.id,
        risk_tier: policy.risk_tier,
        requires_human_approval: false,
        approval_refs: vec![],
        status: ActionCommandStatus::Requested,
        audit_event_refs: vec![],
        requested_at: NOW.into(),
        completed_at: None,
    };
    assert!(matches!(
        test.store.claim_action_command(&requested).unwrap(),
        ActionCommandClaimResult::Claimed(_)
    ));
    assert!(matches!(
        test.store.claim_action_command(&requested).unwrap(),
        ActionCommandClaimResult::Replay(_)
    ));
    let conflicting = ActionCommand {
        payload: json!({"status": "cancelled"}),
        ..requested.clone()
    };
    assert!(matches!(
        test.store.claim_action_command(&conflicting).unwrap(),
        ActionCommandClaimResult::Conflict(_)
    ));
    let governance_rebind = ActionCommand {
        approval_refs: vec!["approval-not-in-original-claim".into()],
        audit_event_refs: vec!["audit-not-in-original-claim".into()],
        ..requested.clone()
    };
    assert!(matches!(
        test.store.claim_action_command(&governance_rebind).unwrap(),
        ActionCommandClaimResult::Conflict(_)
    ));
    assert_eq!(test.store.action_commands().unwrap().len(), 1);

    let authorized = ActionCommand {
        status: ActionCommandStatus::Authorized,
        // Authorized may declare the audit id that the dispatcher appends
        // immediately after authorization; existence is gated at terminal.
        audit_event_refs: vec!["audit-command-authorized".into()],
        ..requested.clone()
    };
    test.store.append_action_command(&authorized).unwrap();
    let authorized_event = AuditEvent {
        id: "audit-command-authorized".into(),
        action_command_id: requested.id.clone(),
        event_kind: AuditEventKind::PolicyAuthorized,
        actor_ref: agent_ref.clone(),
        subject_ref: subject.clone(),
        detail: json!({"policy": "matched"}),
        evidence_refs: vec![],
        occurred_at: "2026-07-20T10:05:00+08:00".into(),
    };
    test.store.append_audit_event(&authorized_event).unwrap();

    let executed_event = AuditEvent {
        id: "audit-command-executed".into(),
        event_kind: AuditEventKind::Executed,
        occurred_at: "2026-07-20T10:10:00+08:00".into(),
        ..authorized_event.clone()
    };
    test.store.append_audit_event(&executed_event).unwrap();
    test.store.append_audit_event(&executed_event).unwrap();
    let rebound_terminal = ActionCommand {
        status: ActionCommandStatus::Executed,
        audit_event_refs: vec![executed_event.id.clone()],
        completed_at: Some(executed_event.occurred_at.clone()),
        ..authorized.clone()
    };
    assert!(matches!(
        test.store.complete_action_command(&rebound_terminal),
        Err(StoreError::Conflict(message)) if message.contains("cannot be removed or rebound")
    ));
    let executed = ActionCommand {
        status: ActionCommandStatus::Executed,
        audit_event_refs: vec![authorized_event.id, executed_event.id.clone()],
        completed_at: Some(executed_event.occurred_at.clone()),
        ..authorized
    };
    test.store.complete_action_command(&executed).unwrap();
    assert_eq!(test.store.action_commands().unwrap().len(), 3);
    assert_eq!(
        test.store.latest_action_commands().unwrap(),
        vec![executed.clone()]
    );
    assert_eq!(
        test.store.latest_action_command(&executed.id).unwrap(),
        Some(executed.clone())
    );
    assert_eq!(
        test.store
            .latest_action_policy_definition(&requested.policy_ref)
            .unwrap(),
        Some(ActionPolicyDefinition {
            id: requested.policy_ref.clone(),
            command_name: requested.command_name.clone(),
            required_permission: requested.required_permission.clone(),
            risk_tier: requested.risk_tier,
            requires_human_approval: requested.requires_human_approval,
            module_ref: "module-trademark".into(),
            definition_ref: "definition-action-page".into(),
            allowed_actor_kinds: vec![ActorType::Agent],
            allowed_effects: vec![ActionEffect::UpdateRecord],
        })
    );
    assert_eq!(test.store.audit_events().unwrap().len(), 2);
    assert_eq!(
        test.store.latest_audit_event(&executed_event.id).unwrap(),
        Some(executed_event.clone())
    );

    assert!(matches!(
        test.store.append_action_command(&ActionCommand {
            status: ActionCommandStatus::Authorized,
            ..executed.clone()
        }),
        Err(StoreError::Conflict(message)) if message.contains("status transition")
    ));
    assert!(matches!(
        test.store.append_audit_event(&AuditEvent {
            detail: json!({"policy": "tampered"}),
            ..executed_event
        }),
        Err(StoreError::Conflict(message)) if message.contains("immutable")
    ));
}

#[test]
fn atomic_action_transitions_reject_audit_collision_without_partial_state() {
    let test = TestStore::new("atomic-action");
    let (_, agent_ref, document_id, policy) = seed_action_policy(&test.store);
    let subject = EntityRef {
        kind: EntityKind::Document,
        id: document_id,
    };
    let requested = ActionCommand {
        id: "command-atomic".into(),
        command_name: policy.command_name,
        subject_ref: subject.clone(),
        requested_by: agent_ref.clone(),
        payload: json!({"status": "ready"}),
        required_permission: policy.required_permission,
        policy_ref: policy.id,
        risk_tier: policy.risk_tier,
        requires_human_approval: false,
        approval_refs: vec![],
        status: ActionCommandStatus::Requested,
        audit_event_refs: vec![],
        requested_at: NOW.into(),
        completed_at: None,
    };
    let first_reservations = vec![
        "audit-shared-reservation".to_string(),
        "command-atomic:executed".to_string(),
        "command-atomic:failed".to_string(),
    ];
    assert!(matches!(
        test.store
            .claim_action_command_with_audit_reservations(&requested, &first_reservations)
            .unwrap(),
        ActionCommandClaimResult::Claimed(_)
    ));
    assert!(matches!(
        test.store
            .claim_action_command_with_audit_reservations(&requested, &first_reservations)
            .unwrap(),
        ActionCommandClaimResult::Replay(_)
    ));
    let second_command = ActionCommand {
        id: "command-atomic-second".into(),
        ..requested.clone()
    };
    test.store.claim_action_command(&second_command).unwrap();
    assert!(matches!(
        test.store.reserve_action_audit_ids(
            &second_command.id,
            &[
                "second-command-would-be-partial".into(),
                "audit-shared-reservation".into(),
            ],
        ),
        Err(StoreError::Conflict(message)) if message.contains("reserved by command")
    ));
    assert_eq!(
        test.store.latest_action_audit_reservations().unwrap().len(),
        3
    );
    assert!(test
        .store
        .latest_action_audit_reservations()
        .unwrap()
        .iter()
        .all(|reservation| reservation.id != "second-command-would-be-partial"));
    let unclaimed_collision = ActionCommand {
        id: "command-atomic-no-partial-claim".into(),
        ..requested.clone()
    };
    assert!(matches!(
        test.store.claim_action_command_with_audit_reservations(
            &unclaimed_collision,
            &["audit-shared-reservation".into()],
        ),
        Err(StoreError::Conflict(message)) if message.contains("reserved by command")
    ));
    assert_eq!(
        test.store
            .latest_action_command(&unclaimed_collision.id)
            .unwrap(),
        None
    );
    let occupied = AuditEvent {
        id: "audit-atomic-authorized".into(),
        action_command_id: requested.id.clone(),
        event_kind: AuditEventKind::Requested,
        actor_ref: agent_ref.clone(),
        subject_ref: subject.clone(),
        detail: json!({"stage": "request"}),
        evidence_refs: vec![],
        occurred_at: "2026-07-20T10:01:00+08:00".into(),
    };
    test.store.append_audit_event(&occupied).unwrap();
    let conflicting_event = AuditEvent {
        event_kind: AuditEventKind::PolicyAuthorized,
        detail: json!({"stage": "authorize"}),
        ..occupied.clone()
    };
    let conflicting_authorized = ActionCommand {
        status: ActionCommandStatus::Authorized,
        audit_event_refs: vec![conflicting_event.id.clone()],
        ..requested.clone()
    };
    assert!(matches!(
        test.store.authorize_action_command_atomic(
            &conflicting_authorized,
            &[conflicting_event],
        ),
        Err(StoreError::Conflict(message)) if message.contains("audit event id is immutable")
    ));
    assert_eq!(
        test.store.latest_action_command(&requested.id).unwrap(),
        Some(requested.clone())
    );
    assert_eq!(test.store.audit_events().unwrap().len(), 1);

    let authorized_event = AuditEvent {
        id: "audit-atomic-authorized-good".into(),
        event_kind: AuditEventKind::PolicyAuthorized,
        detail: json!({"stage": "authorize"}),
        occurred_at: "2026-07-20T10:02:00+08:00".into(),
        ..occupied.clone()
    };
    let authorized = ActionCommand {
        status: ActionCommandStatus::Authorized,
        audit_event_refs: vec![authorized_event.id.clone()],
        ..requested
    };
    test.store
        .authorize_action_command_atomic(&authorized, std::slice::from_ref(&authorized_event))
        .unwrap();
    test.store
        .authorize_action_command_atomic(&authorized, std::slice::from_ref(&authorized_event))
        .unwrap();

    let terminal_event = AuditEvent {
        id: "audit-atomic-executed".into(),
        event_kind: AuditEventKind::Executed,
        detail: json!({"stage": "executed"}),
        occurred_at: "2026-07-20T10:03:00+08:00".into(),
        ..occupied
    };
    let executed = ActionCommand {
        status: ActionCommandStatus::Executed,
        audit_event_refs: vec![authorized_event.id, terminal_event.id.clone()],
        completed_at: Some(terminal_event.occurred_at.clone()),
        ..authorized
    };
    test.store
        .finish_action_command_atomic(&executed, std::slice::from_ref(&terminal_event))
        .unwrap();
    test.store
        .finish_action_command_atomic(&executed, std::slice::from_ref(&terminal_event))
        .unwrap();
    assert_eq!(test.store.action_commands().unwrap().len(), 4);
    assert_eq!(test.store.audit_events().unwrap().len(), 3);
}

#[test]
fn custom_page_bundle_conflict_leaves_no_partial_definition_or_policy() {
    let test = TestStore::new("atomic-page-bundle");
    let (human_ref, _, _, existing_policy) = seed_action_policy(&test.store);
    let package = CustomPagePackage {
        id: "package-action-page-b".into(),
        definition_id: "definition-action-page-b".into(),
        version: "1.0.0".into(),
        kind: CustomPagePackageKind::React,
        artifact_ref: "artifact://action-page-b".into(),
        entrypoint: "index.js".into(),
        integrity_digest: "sha256:action-b".into(),
        built_at: NOW.into(),
    };
    test.store.append_custom_page_package(&package).unwrap();
    let definition = CustomPageDefinition {
        id: package.definition_id.clone(),
        module_id: "module-trademark".into(),
        purpose: "Atomic governed action page".into(),
        allowed_data_queries: vec![DataQueryDeclaration {
            id: "query-action-subject-b".into(),
            source_kind: EntityKind::Document,
            source_scope: "document:doc-cn-2026-018".into(),
            permission_policy_ref: "policy-action-read".into(),
        }],
        approved_ui_components: vec!["ActionButton".into()],
        action_command_refs: vec!["update_trademark".into()],
        standard_view_fallback_ref: "view-action-fallback".into(),
        owner: human_ref,
        package_ref: package.id,
        package_version: package.version,
        fixture_ref: "fixture://action-b".into(),
        visual_contract_ref: "visual://action-b".into(),
        policy_refs: vec!["policy-action-read".into()],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    let conflicting_policy = ActionPolicyDefinition {
        id: existing_policy.id.clone(),
        module_ref: definition.module_id.clone(),
        definition_ref: definition.id.clone(),
        command_name: "update_trademark".into(),
        required_permission: "trademark:update".into(),
        risk_tier: RiskTier::R1,
        requires_human_approval: false,
        allowed_actor_kinds: vec![ActorType::Agent],
        allowed_effects: vec![ActionEffect::UpdateRecord],
    };
    assert!(matches!(
        test.store.append_custom_page_bundle_atomic(
            &definition,
            std::slice::from_ref(&conflicting_policy),
        ),
        Err(StoreError::Conflict(message)) if message.contains("action policy id already")
    ));
    assert!(test
        .store
        .latest_custom_page_definitions()
        .unwrap()
        .iter()
        .all(|row| row.id != definition.id));
    assert_eq!(test.store.action_policy_definitions().unwrap().len(), 1);

    let policy = ActionPolicyDefinition {
        id: "definition-action-page-b:update_trademark".into(),
        ..conflicting_policy
    };
    test.store
        .append_custom_page_bundle_atomic(&definition, std::slice::from_ref(&policy))
        .unwrap();
    test.store
        .append_custom_page_bundle_atomic(&definition, std::slice::from_ref(&policy))
        .unwrap();
    assert_eq!(
        test.store.latest_custom_page_definitions().unwrap().len(),
        2
    );
    assert_eq!(
        test.store.latest_action_policy_definitions().unwrap().len(),
        2
    );
}

#[test]
fn commitment_pending_action_accepts_only_matching_evidenced_requested_human_gate() {
    let test = TestStore::new("commitment-requested-gate");
    let (human_ref, agent_ref, document_id, _) = seed_action_policy(&test.store);
    let policy = ActionPolicyDefinition {
        id: "page-trademark:commitment.append".into(),
        module_ref: "module-trademark".into(),
        definition_ref: "definition-action-page".into(),
        command_name: "commitment.append".into(),
        required_permission: "finance.commitment.write".into(),
        risk_tier: RiskTier::R3,
        requires_human_approval: true,
        allowed_actor_kinds: vec![ActorType::Agent],
        allowed_effects: vec![ActionEffect::CreateCommitment],
    };
    test.store.append_action_policy_definition(&policy).unwrap();
    let proposed = Commitment {
        id: "commitment-requested-gate".into(),
        amount: Money {
            amount: "3000".into(),
            currency: "CNY".into(),
        },
        status: CommitmentStatus::Proposed,
        source_document_id: document_id,
        submitted_by: agent_ref.clone(),
        accountable_owner: human_ref.clone(),
        relation_ids: vec![],
        evidence_refs: vec![],
        approval_refs: vec![],
        audit_event_ids: vec!["audit-commitment-proposed".into()],
        due_at: None,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    test.store.append_commitment(&proposed).unwrap();
    let subject = EntityRef {
        kind: EntityKind::FinancialRecord,
        id: proposed.id.clone(),
    };
    let mut gate = requested_approval(
        "approval-commitment-requested-gate",
        subject.clone(),
        &agent_ref,
        &human_ref,
    );
    gate.action_summary = "Authorize commitment.append for filing fee".into();
    gate.policy_ref = policy.id.clone();
    gate.evidence_refs = vec!["evidence-fee-quote".into()];
    test.store.append_approval(&gate).unwrap();
    let pending = Commitment {
        status: CommitmentStatus::PendingApproval,
        approval_refs: vec![gate.id.clone()],
        audit_event_ids: vec![
            "audit-commitment-proposed".into(),
            "audit-commitment-pending".into(),
        ],
        ..proposed.clone()
    };
    let command = ActionCommand {
        id: "command-commitment-requested-gate".into(),
        command_name: policy.command_name.clone(),
        subject_ref: subject.clone(),
        requested_by: agent_ref.clone(),
        payload: json!({
            "definition_id": policy.definition_ref,
            "record": pending,
        }),
        required_permission: policy.required_permission,
        policy_ref: policy.id.clone(),
        risk_tier: policy.risk_tier,
        requires_human_approval: true,
        approval_refs: vec![gate.id.clone()],
        status: ActionCommandStatus::Requested,
        audit_event_refs: vec!["audit-command-commitment-authorized".into()],
        requested_at: NOW.into(),
        completed_at: None,
    };
    test.store
        .claim_action_command_with_audit_reservations(
            &command,
            &[
                "audit-command-commitment-authorized".into(),
                "audit-command-commitment-executed".into(),
            ],
        )
        .unwrap();
    let authorization_event = AuditEvent {
        id: "audit-command-commitment-authorized".into(),
        action_command_id: command.id.clone(),
        event_kind: AuditEventKind::PolicyAuthorized,
        actor_ref: agent_ref.clone(),
        subject_ref: subject.clone(),
        detail: json!({"gate": "requested_human_approval"}),
        evidence_refs: vec!["evidence-fee-quote".into()],
        occurred_at: NOW.into(),
    };
    let authorized = ActionCommand {
        status: ActionCommandStatus::Authorized,
        ..command.clone()
    };
    test.store
        .authorize_action_command_atomic(&authorized, std::slice::from_ref(&authorization_event))
        .unwrap();
    let target: Commitment = serde_json::from_value(command.payload["record"].clone()).unwrap();
    test.store.append_commitment(&target).unwrap();
    let terminal_event = AuditEvent {
        id: "audit-command-commitment-executed".into(),
        event_kind: AuditEventKind::Executed,
        detail: json!({"result": "pending_approval"}),
        ..authorization_event
    };
    let executed = ActionCommand {
        status: ActionCommandStatus::Executed,
        audit_event_refs: vec![
            "audit-command-commitment-authorized".into(),
            terminal_event.id.clone(),
        ],
        completed_at: Some(NOW.into()),
        ..authorized
    };
    test.store
        .finish_action_command_atomic(&executed, &[terminal_event])
        .unwrap();

    let second_proposed = Commitment {
        id: "commitment-bad-requested-gate".into(),
        status: CommitmentStatus::Proposed,
        approval_refs: vec![],
        audit_event_ids: vec!["audit-second-proposed".into()],
        ..proposed
    };
    test.store.append_commitment(&second_proposed).unwrap();
    let second_subject = EntityRef {
        kind: EntityKind::FinancialRecord,
        id: second_proposed.id.clone(),
    };
    let second_target = Commitment {
        status: CommitmentStatus::PendingApproval,
        approval_refs: vec![gate.id.clone()],
        audit_event_ids: vec!["audit-second-pending".into()],
        ..second_proposed
    };
    let wrong_subject_command = ActionCommand {
        id: "command-wrong-approval-subject".into(),
        subject_ref: second_subject,
        payload: json!({
            "definition_id": "definition-action-page",
            "record": second_target,
        }),
        audit_event_refs: vec!["audit-wrong-subject-authorized".into()],
        ..command.clone()
    };
    test.store
        .claim_action_command(&wrong_subject_command)
        .unwrap();
    let wrong_event = AuditEvent {
        id: "audit-wrong-subject-authorized".into(),
        action_command_id: wrong_subject_command.id.clone(),
        event_kind: AuditEventKind::PolicyAuthorized,
        actor_ref: agent_ref,
        subject_ref: wrong_subject_command.subject_ref.clone(),
        detail: json!({}),
        evidence_refs: vec![],
        occurred_at: NOW.into(),
    };
    assert!(matches!(
        test.store.authorize_action_command_atomic(
            &ActionCommand {
                status: ActionCommandStatus::Authorized,
                ..wrong_subject_command
            },
            &[wrong_event],
        ),
        Err(StoreError::Conflict(message)) if message.contains("matching evidence-backed Human queue gate")
    ));

    let third_proposed = Commitment {
        id: "commitment-no-evidence-gate".into(),
        status: CommitmentStatus::Proposed,
        approval_refs: vec![],
        audit_event_ids: vec!["audit-third-proposed".into()],
        ..target
    };
    test.store.append_commitment(&third_proposed).unwrap();
    let third_subject = EntityRef {
        kind: EntityKind::FinancialRecord,
        id: third_proposed.id.clone(),
    };
    let third_target = Commitment {
        status: CommitmentStatus::PendingApproval,
        approval_refs: vec!["approval-no-evidence".into()],
        audit_event_ids: vec!["audit-third-pending".into()],
        ..third_proposed
    };
    let mut no_evidence_gate = gate;
    no_evidence_gate.id = "approval-no-evidence".into();
    no_evidence_gate.subject_ref = third_subject.clone();
    no_evidence_gate.evidence_refs.clear();
    test.store.append_approval(&no_evidence_gate).unwrap();
    let no_evidence_command = ActionCommand {
        id: "command-no-evidence-approval".into(),
        subject_ref: third_subject,
        payload: json!({
            "definition_id": "definition-action-page",
            "record": third_target,
        }),
        approval_refs: vec![no_evidence_gate.id],
        audit_event_refs: vec!["audit-no-evidence-authorized".into()],
        ..command
    };
    test.store
        .claim_action_command(&no_evidence_command)
        .unwrap();
    let no_evidence_event = AuditEvent {
        id: "audit-no-evidence-authorized".into(),
        action_command_id: no_evidence_command.id.clone(),
        event_kind: AuditEventKind::PolicyAuthorized,
        actor_ref: no_evidence_command.requested_by.clone(),
        subject_ref: no_evidence_command.subject_ref.clone(),
        detail: json!({}),
        evidence_refs: vec![],
        occurred_at: NOW.into(),
    };
    assert!(matches!(
        test.store.authorize_action_command_atomic(
            &ActionCommand {
                status: ActionCommandStatus::Authorized,
                ..no_evidence_command
            },
            &[no_evidence_event],
        ),
        Err(StoreError::Conflict(message)) if message.contains("matching evidence-backed Human queue gate")
    ));
}

fn seed_simple_permission_runtime(
    store: &HarnessStore,
) -> (SimplePermissionState, SimplePermissionRequest, ActorRef) {
    let human_member = human("human-permission-owner");
    let human_ref = actor(ActorType::Human, &human_member.id);
    store.append_human_member(&human_member).unwrap();

    let mut company_agent = standing_agent("agent-simple-permission");
    company_agent.execution_agent_member_ref = Some("agent-member-simple-permission".into());
    let agent_ref = actor(ActorType::Agent, &company_agent.id);
    store.append_standing_agent(&company_agent).unwrap();

    let document_id = "doc-simple-permission";
    store
        .append_document(&document(document_id, &human_ref))
        .unwrap();
    let work = work_item(
        "work-simple-permission",
        document_id,
        &human_ref,
        &agent_ref,
    );
    store.append_work_item(&work).unwrap();

    let member: AgentMember = serde_json::from_value(json!({
        "id": "agent-member-simple-permission",
        "name": "Simple Permission Runtime",
        "description": "Bound execution identity",
        "role": "domain_lead",
        "provider": "codex",
        "model": null,
        "profile": null,
        "capabilities": [],
        "team_ids": ["team-simple-permission"],
        "prompt_ref": null,
        "skill_refs": [],
        "workspace_policy": null,
        "status": "running",
        "current_task_id": null,
        "current_proposal_id": null,
        "provider_runtime_id": null,
        "provider_thread_id": null,
        "control_endpoint": null,
        "created_at": NOW,
        "last_seen_at": NOW
    }))
    .unwrap();
    store.append_member(&member).unwrap();

    let team_run: AgentTeamRun = serde_json::from_value(json!({
        "id": "team-run-simple-permission",
        "agent_team_id": "team-simple-permission",
        "project_binding_id": "project-simple-permission",
        "host_surface": "codex-app",
        "objective": "Exercise exact simple permission binding",
        "status": "running",
        "member_run_ids": ["member-run-simple-permission"],
        "created_at": NOW,
        "updated_at": NOW
    }))
    .unwrap();
    store.append_team_run(&team_run).unwrap();

    let member_run: MemberRun = serde_json::from_value(json!({
        "id": "member-run-simple-permission",
        "team_run_id": team_run.id,
        "agent_member_id": member.id,
        "name": member.name,
        "role": member.role,
        "provider": "codex",
        "status": "running",
        "native_session": {
            "provider": "codex",
            "execution_mode": "codex_app_server",
            "native_session_id": "native-session-simple-permission",
            "native_locator_kind": "codex_rollout",
            "adapter_contract_version": "codex-app-server-v1",
            "availability": "available",
            "supports_resume": true
        },
        "started_at": NOW
    }))
    .unwrap();
    store.append_member_run(&member_run).unwrap();

    let team_message: TeamMessage = serde_json::from_value(json!({
        "id": "team-message-simple-permission",
        "team_run_id": "team-run-simple-permission",
        "from_member_id": "host",
        "to_member_ids": ["member-run-simple-permission"],
        "kind": "assignment",
        "body": "Execute bounded permission work",
        "correlation_id": "correlation-simple-permission",
        "deliveries": [{
            "member_id": "member-run-simple-permission",
            "policy": "queue",
            "status": "delivered",
            "attempt": 1,
            "updated_at": "unix-ms:1"
        }],
        "created_at": "unix-ms:1"
    }))
    .unwrap();
    store.append_team_message(&team_message).unwrap();

    let assignment = Assignment {
        id: "assignment-simple-permission".into(),
        work_item_id: work.id.clone(),
        recipient: agent_ref.clone(),
        sender: human_ref.clone(),
        assigned_role: "domain_lead".into(),
        scope: Some("work-simple-permission".into()),
        delivery_state: AssignmentDeliveryState::Delivered,
        delivery_policy_ref: "policy-assignment-delivery".into(),
        correlation_id: team_message.correlation_id.clone(),
        delivery_evidence_ref: Some(team_message.id.clone()),
        assigned_at: NOW.into(),
        delivered_at: Some(NOW.into()),
        acknowledged_at: None,
    };
    store.append_assignment(&assignment).unwrap();

    let lease = store
        .acquire_team_supervisor_lease(
            &team_run.id,
            "supervisor-simple-permission",
            std::process::id(),
            "test://simple-permission",
            1,
            u64::MAX,
        )
        .unwrap();
    let binding = SimplePermissionExecutionBinding {
        standing_agent_ref: agent_ref,
        agent_member_id: member.id,
        team_run_id: team_run.id,
        member_run_id: member_run.id,
        native_session_id: "native-session-simple-permission".into(),
        project_binding_id: "project-simple-permission".into(),
        assignment_id: assignment.id,
        assignment_team_message_id: team_message.id,
        assignment_correlation_id: team_message.correlation_id,
        supervisor_id: lease.supervisor_id,
        supervisor_generation: lease.generation,
    };
    let state = SimplePermissionState {
        id: "simple-permission-state".into(),
        company_id: "company-example".into(),
        actor_ref: binding.standing_agent_ref.clone(),
        role_template: SimpleRoleTemplate::DomainLead,
        module_capabilities: vec![ModuleCapability {
            module: PermissionModule::Work,
            verbs: vec![
                PermissionVerb::Read,
                PermissionVerb::Execute,
                PermissionVerb::Delegate,
            ],
            scope_refs: vec![work.id],
        }],
        execution_binding: binding.clone(),
        created_by: human_ref.clone(),
        created_at: NOW.into(),
    };
    let request = SimplePermissionRequest {
        execution_binding: binding,
        module: PermissionModule::Work,
        verb: PermissionVerb::Execute,
        scope_ref: "work-simple-permission".into(),
        subject_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: "work-simple-permission".into(),
        },
        subject_scope_evidence_ref: None,
        transport_authenticated: true,
        action_policy_decision: SimpleActionPolicyDecision::Allowed,
        protected_effect: None,
        human_approval_ref: None,
        evaluated_at: NOW.into(),
    };
    (state, request, human_ref)
}

#[test]
fn simple_permission_store_re_resolves_identity_assignment_and_supervisor() {
    let test = TestStore::new("simple-permission");
    let (state, mut request, human_ref) = seed_simple_permission_runtime(&test.store);
    test.store.append_simple_permission_state(&state).unwrap();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Allowed
    );
    request.transport_authenticated = false;
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::TransportUnauthenticated)
    );
    request.transport_authenticated = true;
    assert!(matches!(
        test.store.append_simple_permission_state(&state),
        Err(StoreError::Conflict(message)) if message.contains("already exists")
    ));

    let current_agent = test
        .store
        .latest_standing_agents()
        .unwrap()
        .into_iter()
        .find(|agent| agent.id == state.actor_ref.actor_id)
        .unwrap();
    let mut paused_agent = current_agent.clone();
    paused_agent.status = MemberStatus::Paused;
    test.store.append_standing_agent(&paused_agent).unwrap();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::InactiveIdentity)
    );
    test.store.append_standing_agent(&current_agent).unwrap();

    let current_member_run = test
        .store
        .member_runs()
        .unwrap()
        .into_iter()
        .rev()
        .find(|run| run.id == state.execution_binding.member_run_id)
        .unwrap();
    let mut missing_session = current_member_run.clone();
    missing_session.native_session = None;
    test.store.append_member_run(&missing_session).unwrap();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::MissingNativeSession)
    );
    test.store.append_member_run(&current_member_run).unwrap();

    let current_assignment = test
        .store
        .latest_assignments()
        .unwrap()
        .into_iter()
        .find(|assignment| assignment.id == state.execution_binding.assignment_id)
        .unwrap();
    let mut mismatched_assignment = current_assignment.clone();
    mismatched_assignment.correlation_id = "different-correlation".into();
    test.store
        .append_assignment(&mismatched_assignment)
        .unwrap();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::AssignmentMismatch)
    );
    test.store.append_assignment(&current_assignment).unwrap();

    request.protected_effect = Some(ProtectedEffect::PolicyUnknown);
    request.human_approval_ref = Some("approval-simple-permission".into());
    request.evaluated_at = "2026-07-20T12:00:00+08:00".into();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::ProtectedEffectApprovalRequired)
    );
    let requested = requested_approval(
        "approval-simple-permission",
        EntityRef {
            kind: EntityKind::WorkItem,
            id: "work-simple-permission".into(),
        },
        &state.actor_ref,
        &human_ref,
    );
    let requested = Approval {
        policy_ref: ProtectedEffect::PolicyUnknown.as_str().into(),
        ..requested
    };
    test.store.append_approval(&requested).unwrap();
    let approval = approved(requested, &human_ref);
    test.store.append_approval(&approval).unwrap();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Allowed
    );
    let other_subject = work_item(
        "work-other",
        "doc-simple-permission",
        &human_ref,
        &state.actor_ref,
    );
    test.store.append_work_item(&other_subject).unwrap();
    let other_subject_relation = Relation {
        id: "scope-relation-work-other".into(),
        from_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: "work-simple-permission".into(),
        },
        relation_type: "scope_contains/work".into(),
        to_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: other_subject.id.clone(),
        },
        provenance_ref: None,
        lifecycle_status: Some("active".into()),
        created_by: human_ref.clone(),
        created_at: NOW.into(),
    };
    test.store.append_relation(&other_subject_relation).unwrap();
    request.subject_ref.id = other_subject.id;
    request.subject_scope_evidence_ref = Some(other_subject_relation.id);
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::ProtectedEffectApprovalRequired)
    );
    request.subject_ref.id = "work-simple-permission".into();
    request.subject_scope_evidence_ref = None;

    test.store
        .release_team_supervisor_lease(
            &state.execution_binding.team_run_id,
            &state.execution_binding.supervisor_id,
            state.execution_binding.supervisor_generation,
            2,
        )
        .unwrap();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::StaleSupervisor)
    );
}

#[test]
fn simple_permission_store_binds_ordinary_subject_to_authorized_scope() {
    let test = TestStore::new("simple-permission-subject-scope");
    let (state, mut request, human_ref) = seed_simple_permission_runtime(&test.store);
    test.store.append_simple_permission_state(&state).unwrap();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Allowed,
        "an exact module-appropriate subject that exists in current Store truth is allowed"
    );
    request.subject_ref.kind = EntityKind::Document;
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::SubjectScopeMismatch),
        "same-id cross-kind resemblance must not authorize"
    );
    request.subject_ref.kind = EntityKind::WorkItem;

    let child = work_item(
        "work-simple-permission-subject",
        "doc-simple-permission",
        &human_ref,
        &state.actor_ref,
    );
    test.store.append_work_item(&child).unwrap();
    let unrelated = work_item(
        "work-simple-permission-other-subject",
        "doc-simple-permission",
        &human_ref,
        &state.actor_ref,
    );
    test.store.append_work_item(&unrelated).unwrap();

    request.subject_ref = EntityRef {
        kind: EntityKind::WorkItem,
        id: child.id.clone(),
    };
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::SubjectScopeEvidenceMissing),
        "ordinary non-protected actions fail closed without canonical subject containment"
    );

    request.subject_scope_evidence_ref = Some("fabricated-subject-relation".into());
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::SubjectScopeEvidenceMissing)
    );

    let mismatched = Relation {
        id: "subject-scope-relation-mismatched".into(),
        from_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: "work-simple-permission".into(),
        },
        relation_type: "scope_contains/work".into(),
        to_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: unrelated.id,
        },
        provenance_ref: None,
        lifecycle_status: Some("active".into()),
        created_by: human_ref.clone(),
        created_at: NOW.into(),
    };
    test.store.append_relation(&mismatched).unwrap();
    request.subject_scope_evidence_ref = Some(mismatched.id);
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::SubjectScopeMismatch)
    );

    let valid = Relation {
        id: "subject-scope-relation-valid".into(),
        from_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: "work-simple-permission".into(),
        },
        relation_type: "scope_contains/work".into(),
        to_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: child.id,
        },
        provenance_ref: None,
        lifecycle_status: Some("active".into()),
        created_by: human_ref,
        created_at: NOW.into(),
    };
    test.store.append_relation(&valid).unwrap();
    request.subject_scope_evidence_ref = Some(valid.id.clone());
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Allowed,
        "a current Store-owned relation may bind an ordinary action subject to its scope"
    );

    let stale = Relation {
        lifecycle_status: Some("archived".into()),
        ..valid
    };
    test.store.append_relation(&stale).unwrap();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::SubjectScopeEvidenceStale)
    );
}

#[test]
fn simple_permission_store_denies_same_id_missing_scope_subject() {
    let test = TestStore::new("simple-permission-missing-exact-subject");
    let (mut state, mut request, _human_ref) = seed_simple_permission_runtime(&test.store);
    state.module_capabilities[0].scope_refs = vec!["work-missing-exact-subject".into()];
    request.scope_ref = "work-missing-exact-subject".into();
    request.subject_ref = EntityRef {
        kind: EntityKind::WorkItem,
        id: "work-missing-exact-subject".into(),
    };
    test.store.append_simple_permission_state(&state).unwrap();
    assert_eq!(
        test.store
            .evaluate_simple_permission(&state.id, &request)
            .unwrap(),
        SimplePermissionDecision::Denied(SimplePermissionDenial::SubjectScopeUnresolved),
        "same-id scope and subject must resolve to a current canonical Store entity"
    );
}

#[test]
fn simple_permission_delegation_uses_only_current_store_scope_relations() {
    let test = TestStore::new("simple-permission-delegation");
    let (state, _request, human_ref) = seed_simple_permission_runtime(&test.store);
    test.store.append_simple_permission_state(&state).unwrap();

    let child = work_item(
        "work-simple-permission-child",
        "doc-simple-permission",
        &human_ref,
        &state.actor_ref,
    );
    test.store.append_work_item(&child).unwrap();
    let unrelated = work_item(
        "work-simple-permission-unrelated",
        "doc-simple-permission",
        &human_ref,
        &state.actor_ref,
    );
    test.store.append_work_item(&unrelated).unwrap();

    let mut delegation = SimplePermissionDelegationRequest {
        child_role: SimpleRoleTemplate::ExecutionMember,
        child_capabilities: vec![ModuleCapability {
            module: PermissionModule::Work,
            verbs: vec![PermissionVerb::Read, PermissionVerb::Execute],
            scope_refs: vec![child.id.clone()],
        }],
        scope_evidence_refs: vec![],
    };
    assert_eq!(
        test.store
            .validate_simple_permission_child_delegation(&state.id, &delegation)
            .unwrap(),
        Err(SimpleDelegationError::ScopeEvidenceMissing)
    );

    delegation.scope_evidence_refs = vec!["fabricated-scope-relation".into()];
    assert_eq!(
        test.store
            .validate_simple_permission_child_delegation(&state.id, &delegation)
            .unwrap(),
        Err(SimpleDelegationError::ScopeEvidenceMissing),
        "a caller-invented evidence_ref must never authorize"
    );

    let mismatched = Relation {
        id: "scope-relation-mismatched".into(),
        from_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: unrelated.id,
        },
        relation_type: "scope_contains/work".into(),
        to_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: child.id.clone(),
        },
        provenance_ref: None,
        lifecycle_status: Some("active".into()),
        created_by: human_ref.clone(),
        created_at: NOW.into(),
    };
    test.store.append_relation(&mismatched).unwrap();
    delegation.scope_evidence_refs = vec![mismatched.id];
    assert_eq!(
        test.store
            .validate_simple_permission_child_delegation(&state.id, &delegation)
            .unwrap(),
        Err(SimpleDelegationError::ScopeEvidenceMismatch)
    );

    let wrong_module = Relation {
        id: "scope-relation-wrong-module".into(),
        from_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: "work-simple-permission".into(),
        },
        relation_type: "scope_contains/docs".into(),
        to_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: child.id.clone(),
        },
        provenance_ref: None,
        lifecycle_status: Some("active".into()),
        created_by: human_ref.clone(),
        created_at: NOW.into(),
    };
    test.store.append_relation(&wrong_module).unwrap();
    delegation.scope_evidence_refs = vec![wrong_module.id];
    assert_eq!(
        test.store
            .validate_simple_permission_child_delegation(&state.id, &delegation)
            .unwrap(),
        Err(SimpleDelegationError::ScopeEvidenceMismatch)
    );

    let valid = Relation {
        id: "scope-relation-valid".into(),
        from_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: "work-simple-permission".into(),
        },
        relation_type: "scope_contains/work".into(),
        to_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: child.id,
        },
        provenance_ref: None,
        lifecycle_status: Some("active".into()),
        created_by: human_ref,
        created_at: NOW.into(),
    };
    test.store.append_relation(&valid).unwrap();
    delegation.scope_evidence_refs = vec![valid.id.clone()];
    assert_eq!(
        test.store
            .validate_simple_permission_child_delegation(&state.id, &delegation)
            .unwrap(),
        Ok(()),
        "the Store may normalize a current canonical relation into an exact parent scope"
    );

    let stale = Relation {
        lifecycle_status: Some("archived".into()),
        ..valid
    };
    test.store.append_relation(&stale).unwrap();
    assert_eq!(
        test.store
            .validate_simple_permission_child_delegation(&state.id, &delegation)
            .unwrap(),
        Err(SimpleDelegationError::ScopeEvidenceStale)
    );
}
