use harness_core::{
    ActionCommand, ActionCommandStatus, ActionEffect, ActionPolicyDefinition, ActorRef, ActorType,
    Approval, ApprovalStatus, Assignment, AssignmentDeliveryState, AuditEvent, AuditEventKind,
    Block, BlockKind, BusinessModule, Commitment, CommitmentStatus, CompanyOsValidationError,
    CustomPageDefinition, CustomPagePackage, CustomPagePackageKind, DataQueryDeclaration, Document,
    DocumentKind, EntityKind, EntityRef, ExecutionMode, ExternalParticipant, HumanMember,
    LifecycleStatus, MemberStatus, Milestone, MilestoneStatus, Money, OrgUnit, OrgUnitStatus,
    OrganizationMembership, OrganizationMembershipRole, OrganizationMembershipStatus, Payment,
    PaymentStatus, Relation, RelationRule, RiskTier, ServiceActor, StandingAgent, TypedRecord,
    ValidateCompanyOs, View, ViewMode, WorkItem, WorkItemStatus, WorkType,
};
use serde_json::json;

const NOW: &str = "2026-07-20T10:00:00Z";

fn actor(actor_type: ActorType, id: &str) -> ActorRef {
    ActorRef {
        actor_type,
        actor_id: id.into(),
    }
}

fn human() -> ActorRef {
    actor(ActorType::Human, "human-brand-owner")
}

fn agent() -> ActorRef {
    actor(ActorType::Agent, "agent-trademark")
}

#[test]
fn historical_standing_agent_rows_default_new_configuration_references() {
    let legacy = json!({
        "id": "agent-legacy",
        "display_name": "Legacy Agent",
        "role": "operations",
        "status": "active",
        "availability": "unknown",
        "assignment_capacity": null,
        "exclusive_assignment_ref": null,
        "membership_refs": [],
        "responsibility_summary": "Historical row written before configuration references existed.",
        "capability_refs": [],
        "permission_policy_refs": [],
        "runtime_refs": [],
        "native_session_refs": [],
        "created_at": NOW,
        "updated_at": NOW
    });

    let agent: StandingAgent = serde_json::from_value(legacy).unwrap();
    agent.validate().unwrap();
    assert_eq!(agent.system_prompt_ref, None);
    assert!(agent.tool_refs.is_empty());
    assert!(agent.skill_refs.is_empty());
    assert!(agent.maintained_document_refs.is_empty());
    assert!(agent.accepted_work_type_refs.is_empty());
    assert_eq!(agent.escalation_policy_ref, None);
}

#[test]
fn actor_types_remain_distinct_on_the_wire() {
    let human_member = HumanMember {
        id: "human-brand-owner".into(),
        display_name: "Brand Owner".into(),
        title: Some("Owner".into()),
        status: MemberStatus::Active,
        availability: None,
        membership_refs: vec!["membership-brand-ip".into()],
        responsibility_summary: "Owns Brand A legal and financial outcomes.".into(),
        permission_policy_refs: vec!["policy-brand".into()],
        authority_policy_refs: vec!["policy-human-gate".into()],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    let standing_agent = StandingAgent {
        id: "agent-trademark".into(),
        display_name: "Trademark Agent".into(),
        role: "trademark_operations".into(),
        status: MemberStatus::Active,
        availability: harness_core::DeclaredAvailability::Available,
        assignment_capacity: Some(3),
        exclusive_assignment_ref: None,
        membership_refs: vec!["membership-brand-ip-agent".into()],
        responsibility_summary: "Prepares governed trademark work.".into(),
        capability_refs: vec!["capability-trademark-search".into()],
        system_prompt_ref: Some("document-agent-prompt-trademark".into()),
        tool_refs: vec!["tool-trademark-search".into()],
        skill_refs: vec!["skill-trademark-filing".into()],
        maintained_document_refs: vec!["document-trademark-register".into()],
        accepted_work_type_refs: vec!["work-type-legal-filing".into()],
        escalation_policy_ref: Some("policy-trademark-escalation".into()),
        permission_policy_refs: vec!["policy-agent-preparation".into()],
        runtime_refs: vec![],
        native_session_refs: vec![],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    let external = ExternalParticipant {
        id: "external-lawyer".into(),
        display_name_or_organization: "External Lawyer".into(),
        engagement_scope: "Review CN-2026-018 only".into(),
        sponsor_actor_ref: human(),
        access_expires_at: "2026-08-20T00:00:00Z".into(),
        confidentiality_or_contract_refs: vec!["contract-legal-2026".into()],
        membership_refs: vec!["membership-brand-ip-advisor".into()],
        restricted_permission_refs: vec!["policy-cn-2026-018-read".into()],
        status: MemberStatus::Active,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    let service = ServiceActor {
        id: "service-finance-sync".into(),
        display_name: "Finance Sync".into(),
        service_kind: "ledger_integration".into(),
        owner_actor_ref: human(),
        credential_boundary: "Read finance records; no approval authority".into(),
        permission_policy_refs: vec!["policy-finance-sync".into()],
        audit_policy_ref: "policy-service-audit".into(),
        status: MemberStatus::Active,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };

    human_member.validate().unwrap();
    standing_agent.validate().unwrap();
    external.validate().unwrap();
    service.validate().unwrap();

    assert_eq!(
        serde_json::to_value(human()).unwrap()["actor_type"],
        "human"
    );
    assert_eq!(
        serde_json::to_value(agent()).unwrap()["actor_type"],
        "agent"
    );
    assert_eq!(
        serde_json::to_value(actor(ActorType::External, "external-lawyer")).unwrap()["actor_type"],
        "external"
    );
    assert_eq!(
        serde_json::to_value(actor(ActorType::Service, "finance-sync")).unwrap()["actor_type"],
        "service"
    );
}

#[test]
fn organization_is_flat_first_but_keeps_explicit_typed_leads_and_membership() {
    let unit = OrgUnit {
        id: "unit-company".into(),
        organization_id: "organization-star".into(),
        name: "Company".into(),
        purpose: "Top-level mixed organization".into(),
        parent_unit_id: None,
        status: OrgUnitStatus::Active,
        human_lead_actor_ref: Some(human()),
        agent_lead_actor_ref: Some(actor(ActorType::Agent, "agent-company-lead")),
        policy_refs: vec!["policy-company".into()],
        document_space_ref: Some("space-company".into()),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    unit.validate().unwrap();

    let mut membership = OrganizationMembership {
        id: "membership-external-lawyer".into(),
        organization_id: unit.organization_id.clone(),
        org_unit_id: unit.id.clone(),
        actor_ref: actor(ActorType::External, "external-lawyer"),
        membership_role: OrganizationMembershipRole::Lead,
        title_or_function: Some("CN trademark counsel".into()),
        status: OrganizationMembershipStatus::Active,
        starts_at: NOW.into(),
        ends_at: Some("2026-08-20T00:00:00Z".into()),
        authority_policy_refs: vec!["policy-external-counsel".into()],
        created_by_actor_ref: human(),
        created_at: NOW.into(),
    };
    assert!(matches!(
        membership.validate(),
        Err(CompanyOsValidationError::Invalid {
            field: "OrganizationMembership.actor_ref",
            ..
        })
    ));
    membership.membership_role = OrganizationMembershipRole::ExternalPartner;
    membership.validate().unwrap();
}

#[test]
fn document_record_relation_and_view_are_one_substrate() {
    let document = Document {
        id: "doc-trademark-cn-2026-018".into(),
        space_id: "space-brand-ip".into(),
        parent_document_id: None,
        title: "Trademark application CN-2026-018".into(),
        kind: DocumentKind::Record,
        lifecycle_status: LifecycleStatus::Active,
        block_ids: vec!["block-overview".into()],
        template_ref: Some("template-trademark-application".into()),
        permission_policy_refs: vec!["policy-brand-ip".into()],
        reference_refs: vec![],
        created_by: agent(),
        updated_by: agent(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    let block = Block {
        id: "block-overview".into(),
        document_id: document.id.clone(),
        kind: BlockKind::RichText,
        position: 0,
        content: json!({"text": "Application brief"}),
        referenced_entities: vec![],
        created_by: agent(),
        updated_by: agent(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    let record = TypedRecord {
        id: "record-cn-2026-018".into(),
        module_id: "module-trademark".into(),
        record_type: "trademark_application".into(),
        title: document.title.clone(),
        fields: json!({"jurisdiction": "CN", "application_number": "CN-2026-018"}),
        lifecycle_status: "preparing".into(),
        source_document_ref: Some(document.id.clone()),
        created_by: agent(),
        updated_by: agent(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    let relation = Relation {
        id: "relation-doc-record".into(),
        from_ref: EntityRef {
            kind: EntityKind::Document,
            id: document.id.clone(),
        },
        relation_type: "describes".into(),
        to_ref: EntityRef {
            kind: EntityKind::TypedRecord,
            id: record.id.clone(),
        },
        provenance_ref: Some(EntityRef {
            kind: EntityKind::Document,
            id: document.id.clone(),
        }),
        created_by: agent(),
        created_at: NOW.into(),
    };
    let view = View {
        id: "view-trademark-table".into(),
        module_id: Some("module-trademark".into()),
        title: "Trademark applications".into(),
        mode: ViewMode::Table,
        source_kinds: vec![EntityKind::TypedRecord],
        query: json!({"record_type": "trademark_application"}),
        owner: agent(),
        policy_refs: vec!["policy-brand-ip".into()],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };

    document.validate().unwrap();
    block.validate().unwrap();
    record.validate().unwrap();
    relation.validate().unwrap();
    view.validate().unwrap();
    assert_eq!(serde_json::to_value(&view).unwrap()["mode"], "table");
}

fn trademark_work_item() -> WorkItem {
    WorkItem {
        id: "work-trademark-filing".into(),
        title: "Trademark filing for Brand A".into(),
        objective: "Prepare and submit application CN-2026-018 after approval.".into(),
        status: WorkItemStatus::WaitingForApproval,
        source_document_ref: "doc-trademark-cn-2026-018".into(),
        source_record_refs: vec!["record-cn-2026-018".into()],
        milestone_ref: Some("milestone-trademark-filed".into()),
        work_type: WorkType::Legal,
        business_module_ref: Some("module-trademark".into()),
        result_document_ref: Some("doc-trademark-cn-2026-018".into()),
        result_record_refs: vec![],
        submitted_by: agent(),
        requested_by: Some(human()),
        accountable_owner: human(),
        assignees: vec![agent()],
        contributors: vec![actor(ActorType::External, "external-lawyer")],
        reviewer: Some(actor(ActorType::Agent, "agent-finance")),
        approver: Some(human()),
        execution_mode: ExecutionMode::Direct,
        execution_refs: vec![],
        approval_refs: vec!["approval-filing".into()],
        evidence_refs: vec![],
        artifact_refs: vec![],
        outcome_summary: None,
        due_at: Some("2026-07-31T00:00:00Z".into()),
        priority: Some("high".into()),
        risk_level: Some("r3".into()),
        created_at: NOW.into(),
        updated_at: NOW.into(),
        completed_at: None,
    }
}

#[test]
fn work_item_has_business_provenance_without_requiring_an_executor() {
    let work = trademark_work_item();
    work.validate().unwrap();
    assert!(work.execution_refs.is_empty());
    assert_eq!(work.accountable_owner.actor_type, ActorType::Human);
    assert_ne!(work.submitted_by, work.accountable_owner);
    assert!(serde_json::to_string(&work)
        .unwrap()
        .contains("waiting_for_approval"));
}

#[test]
fn milestone_is_a_company_checkpoint_not_an_execution_wave() {
    let milestone = Milestone {
        id: "milestone-trademark-filed".into(),
        title: "Trademark application submitted".into(),
        outcome: "The CN application has a durable filing receipt".into(),
        status: MilestoneStatus::Active,
        accountable_owner: human(),
        source_document_ref: Some("doc-trademark-cn-2026-018".into()),
        business_module_ref: Some("module-trademark".into()),
        target_at: Some("2026-07-31T00:00:00Z".into()),
        acceptance_criteria: vec!["Receipt evidence is linked".into()],
        work_item_refs: vec!["work-trademark-filing".into()],
        created_at: NOW.into(),
        updated_at: NOW.into(),
        achieved_at: None,
    };
    milestone.validate().unwrap();
    let wire = serde_json::to_value(milestone).unwrap();
    assert!(wire.get("wave_id").is_none());
    assert_eq!(wire["business_module_ref"], "module-trademark");
}

#[test]
fn historical_work_rows_default_to_general_without_inventing_relations() {
    let mut wire = serde_json::to_value(trademark_work_item()).unwrap();
    let object = wire.as_object_mut().unwrap();
    object.remove("work_type");
    object.remove("milestone_ref");
    object.remove("business_module_ref");
    let decoded: WorkItem = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded.work_type, WorkType::General);
    assert_eq!(decoded.milestone_ref, None);
    assert_eq!(decoded.business_module_ref, None);
}

#[test]
fn agent_assignment_needs_durable_delivery_evidence() {
    let mut assignment = Assignment {
        id: "assignment-trademark-agent".into(),
        work_item_id: "work-trademark-filing".into(),
        recipient: agent(),
        sender: human(),
        assigned_role: "assignee".into(),
        scope: Some("Prepare filing package".into()),
        delivery_state: AssignmentDeliveryState::Delivered,
        delivery_policy_ref: "policy-agent-message".into(),
        correlation_id: "corr-filing-001".into(),
        delivery_evidence_ref: None,
        assigned_at: NOW.into(),
        delivered_at: Some(NOW.into()),
        acknowledged_at: None,
    };
    assert_eq!(
        assignment.validate(),
        Err(CompanyOsValidationError::Required {
            field: "Assignment.delivery_evidence_ref"
        })
    );
    assignment.delivery_evidence_ref = Some("team-message-assignment-001".into());
    assignment.validate().unwrap();
}

#[test]
fn required_human_approval_cannot_be_satisfied_by_an_agent() {
    let mut approval = Approval {
        id: "approval-filing".into(),
        subject_ref: EntityRef {
            kind: EntityKind::WorkItem,
            id: "work-trademark-filing".into(),
        },
        action_summary: "Authorize legal filing and ¥3,000 commitment".into(),
        requested_by: agent(),
        required_approver_refs: vec![human()],
        required_actor_type: Some(ActorType::Human),
        policy_ref: "policy-r3-human-gate".into(),
        status: ApprovalStatus::Approved,
        decided_by: vec![agent()],
        decision_note: Some("Ready to file".into()),
        evidence_refs: vec!["evidence-legal-review".into()],
        requested_at: NOW.into(),
        decided_at: Some(NOW.into()),
        expires_at: None,
    };
    assert!(matches!(
        approval.validate(),
        Err(CompanyOsValidationError::Invalid {
            field: "Approval.decided_by",
            ..
        })
    ));
    approval.decided_by = vec![human()];
    approval.validate().unwrap();
}

#[test]
fn commitment_is_not_payment_and_settlement_needs_payment_evidence() {
    let commitment = Commitment {
        id: "commitment-cn-2026-018".into(),
        amount: Money {
            amount: "3000.00".into(),
            currency: "CNY".into(),
        },
        status: CommitmentStatus::PendingApproval,
        source_document_id: "doc-trademark-cn-2026-018".into(),
        submitted_by: agent(),
        accountable_owner: human(),
        relation_ids: vec!["relation-commitment-application".into()],
        evidence_refs: vec![],
        approval_refs: vec![],
        audit_event_ids: vec!["audit-commitment-created".into()],
        due_at: Some("2026-07-31T00:00:00Z".into()),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    commitment.validate().unwrap();

    let mut payment = Payment {
        id: "payment-cn-2026-018".into(),
        amount: commitment.amount.clone(),
        status: PaymentStatus::Settled,
        source_document_id: commitment.source_document_id.clone(),
        submitted_by: agent(),
        accountable_owner: human(),
        related_commitment_refs: vec![commitment.id.clone()],
        relation_ids: vec!["relation-payment-application".into()],
        evidence_refs: vec![],
        approval_refs: vec!["approval-payment".into()],
        audit_event_ids: vec!["audit-payment-settled".into()],
        occurred_at: Some(NOW.into()),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    assert_eq!(
        payment.validate(),
        Err(CompanyOsValidationError::Required {
            field: "Payment.evidence_refs"
        })
    );
    payment.evidence_refs.push("bank-confirmation-001".into());
    payment.validate().unwrap();
    assert_ne!(
        serde_json::to_value(commitment).unwrap()["status"],
        serde_json::to_value(payment).unwrap()["status"]
    );
}

#[test]
fn module_and_custom_page_keep_fallback_and_governed_action_boundaries() {
    let module = BusinessModule {
        id: "module-trademark".into(),
        name: "Trademark Management".into(),
        purpose: "Govern trademark applications and related work.".into(),
        root_document_ref: "doc-trademark-home".into(),
        record_types: vec!["trademark_application".into()],
        relation_rules: vec![RelationRule {
            relation_type: "incurs".into(),
            from_kind: EntityKind::TypedRecord,
            to_kind: EntityKind::FinancialRecord,
            required: false,
            cross_module: true,
        }],
        default_view_refs: vec!["view-trademark-table".into()],
        policy_refs: vec!["policy-trademark".into()],
        lifecycle_rules: vec!["rule-application-lifecycle".into()],
        metric_definition_refs: vec![],
        custom_page_definition_refs: vec!["page-trademark-home".into()],
        status: LifecycleStatus::Active,
        owner: human(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    module.validate().unwrap();

    let definition = CustomPageDefinition {
        id: "page-trademark-home".into(),
        module_id: module.id.clone(),
        purpose: "Show applications, approval and finance context together.".into(),
        allowed_data_queries: vec![DataQueryDeclaration {
            id: "query-applications".into(),
            source_kind: EntityKind::TypedRecord,
            source_scope: "module:module-trademark".into(),
            permission_policy_ref: "policy-trademark-read".into(),
        }],
        approved_ui_components: vec!["record_table".into(), "approval_card".into()],
        action_command_refs: vec!["command-submit-filing".into()],
        standard_view_fallback_ref: "view-trademark-table".into(),
        owner: human(),
        package_ref: "package-trademark-home".into(),
        package_version: "1.0.0".into(),
        fixture_ref: "fixture-company-os-trademark-v1".into(),
        visual_contract_ref: "visual-company-os-v1".into(),
        policy_refs: vec!["policy-custom-page".into()],
        created_at: NOW.into(),
        updated_at: NOW.into(),
    };
    definition.validate().unwrap();
    CustomPagePackage {
        id: "package-trademark-home".into(),
        definition_id: definition.id.clone(),
        version: "1.0.0".into(),
        kind: CustomPagePackageKind::React,
        artifact_ref: "artifacts/trademark-home-1.0.0.tgz".into(),
        entrypoint: "dist/index.js".into(),
        integrity_digest: "sha256:fixture".into(),
        built_at: NOW.into(),
    }
    .validate()
    .unwrap();

    let mut command = ActionCommand {
        id: "command-submit-filing-001".into(),
        command_name: "submit_trademark_filing".into(),
        subject_ref: EntityRef {
            kind: EntityKind::TypedRecord,
            id: "record-cn-2026-018".into(),
        },
        requested_by: agent(),
        payload: json!({"jurisdiction": "CN"}),
        required_permission: "trademark.filing.submit".into(),
        policy_ref: "policy-r3-legal-filing".into(),
        risk_tier: RiskTier::R3,
        requires_human_approval: true,
        approval_refs: vec![],
        status: ActionCommandStatus::Authorized,
        audit_event_refs: vec![],
        requested_at: NOW.into(),
        completed_at: None,
    };
    assert_eq!(
        command.validate(),
        Err(CompanyOsValidationError::Required {
            field: "ActionCommand.approval_refs"
        })
    );
    command.approval_refs.push("approval-filing".into());
    command.validate().unwrap();
}

#[test]
fn action_policy_is_server_authority_and_command_id_is_idempotency_key() {
    let policy = ActionPolicyDefinition {
        id: "policy-action-submit-filing".into(),
        module_ref: "module-trademark".into(),
        definition_ref: "page-trademark-home".into(),
        command_name: "submit_trademark_filing".into(),
        required_permission: "trademark.filing.submit".into(),
        risk_tier: RiskTier::R3,
        requires_human_approval: true,
        allowed_actor_kinds: vec![ActorType::Human, ActorType::Agent],
        allowed_effects: vec![ActionEffect::SubmitLegalFiling],
    };
    policy.validate().unwrap();

    let mut request = ActionCommand {
        id: "idempotency:submit-filing:cn-2026-018:v1".into(),
        command_name: policy.command_name.clone(),
        subject_ref: EntityRef {
            kind: EntityKind::TypedRecord,
            id: "record-cn-2026-018".into(),
        },
        requested_by: agent(),
        payload: json!({"jurisdiction": "CN"}),
        required_permission: policy.required_permission.clone(),
        policy_ref: policy.id.clone(),
        risk_tier: RiskTier::R0,
        requires_human_approval: false,
        approval_refs: vec![],
        status: ActionCommandStatus::Requested,
        audit_event_refs: vec![],
        requested_at: NOW.into(),
        completed_at: None,
    };
    assert_eq!(request.idempotency_key(), request.id);
    assert!(matches!(
        request.validate_against_policy(&policy, ActionEffect::SubmitLegalFiling),
        Err(CompanyOsValidationError::Invalid {
            field: "ActionCommand.risk_tier",
            ..
        })
    ));
    request.risk_tier = policy.risk_tier;
    request.requires_human_approval = policy.requires_human_approval;
    request
        .validate_against_policy(&policy, ActionEffect::SubmitLegalFiling)
        .unwrap();
    assert!(request
        .validate_against_policy(&policy, ActionEffect::SettlePayment)
        .is_err());
}

#[test]
fn audit_event_is_a_durable_typed_action_record() {
    let event = AuditEvent {
        id: "audit-submit-filing-requested".into(),
        action_command_id: "idempotency:submit-filing:cn-2026-018:v1".into(),
        event_kind: AuditEventKind::Requested,
        actor_ref: agent(),
        subject_ref: EntityRef {
            kind: EntityKind::TypedRecord,
            id: "record-cn-2026-018".into(),
        },
        detail: json!({"summary": "Requested governed CN trademark filing"}),
        evidence_refs: vec!["evidence-filing-package".into()],
        occurred_at: NOW.into(),
    };
    event.validate().unwrap();
    let round_trip: AuditEvent =
        serde_json::from_value(serde_json::to_value(&event).unwrap()).unwrap();
    assert_eq!(round_trip.id, event.id);
    assert_eq!(round_trip.event_kind, AuditEventKind::Requested);
}
