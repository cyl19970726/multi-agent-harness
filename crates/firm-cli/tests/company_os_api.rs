//! End-to-end HTTP acceptance for the Company OS projection and governed Actions.

mod firm_env;
use firm_env::{current_project_id, run_firm, ServeHandle, TempHome};
use serde_json::{json, Value};

const NOW: &str = "2026-07-20T10:00:00+08:00";
const TEST_TOKEN: &str = "company-os-api-test-capability";

fn init_project(home: &TempHome) -> String {
    let root = home.base().join("company");
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init failed: {out:?}");
    current_project_id(home)
}

fn post_ok(serve: &ServeHandle, path: &str, body: Value) -> Value {
    let (status, response) = post_json(serve, path, &body);
    assert_eq!(status, 200, "POST {path}: {response}");
    assert_eq!(response["ok"], true, "POST {path}: {response}");
    response["result"].clone()
}

fn post_json(serve: &ServeHandle, path: &str, body: &Value) -> (u16, Value) {
    serve.post_json_with_token(path, body, TEST_TOKEN)
}

fn actor(kind: &str, id: &str) -> Value {
    json!({"actor_type": kind, "actor_id": id})
}

fn admin(record: Value) -> Value {
    json!({
        "mode": "administrative",
        "authority": actor("human", "human-brand-owner"),
        "record": record
    })
}

#[allow(clippy::too_many_arguments)]
fn action(
    id: &str,
    command_name: &str,
    subject: Value,
    record: Value,
    policy_ref: &str,
    required_permission: &str,
    risk_tier: &str,
    requires_human_approval: bool,
    approval_refs: Vec<&str>,
    audit_event: &str,
) -> Value {
    action_by(
        id,
        command_name,
        subject,
        record,
        actor("human", "human-brand-owner"),
        policy_ref,
        required_permission,
        risk_tier,
        requires_human_approval,
        approval_refs,
        audit_event,
    )
}

#[allow(clippy::too_many_arguments)]
fn action_by(
    id: &str,
    command_name: &str,
    subject: Value,
    record: Value,
    requested_by: Value,
    policy_ref: &str,
    required_permission: &str,
    risk_tier: &str,
    requires_human_approval: bool,
    approval_refs: Vec<&str>,
    audit_event: &str,
) -> Value {
    json!({
        "id": id,
        "command_name": command_name,
        "subject_ref": subject,
        "requested_by": requested_by,
        "payload": {"definition_id": "page-trademark", "record": record},
        "required_permission": required_permission,
        "policy_ref": policy_ref,
        "risk_tier": risk_tier,
        "requires_human_approval": requires_human_approval,
        "approval_refs": approval_refs,
        "status": "requested",
        "audit_event_refs": [audit_event],
        "requested_at": NOW,
        "completed_at": null
    })
}

fn human(id: &str, name: &str) -> Value {
    json!({
        "actor_type": "human",
        "actor": {
            "id": id,
            "display_name": name,
            "title": "Brand owner",
            "status": "active",
            "availability": "available",
            "membership_refs": [],
            "responsibility_summary": "Accountable company owner",
            "permission_policy_refs": [
                "company_os.admin",
                "finance.commitment.write",
                "finance.payment.write",
                "company.records.write",
                "company.work.execute"
            ],
            "authority_policy_refs": ["company.approve", "policy-finance"],
            "created_at": NOW,
            "updated_at": NOW
        }
    })
}

fn agent_record(id: &str, name: &str, role: &str) -> Value {
    json!({
        "actor_type": "agent",
        "actor": {
            "id": id,
            "display_name": name,
            "role": role,
            "status": "active",
            "availability": "available",
            "assignment_capacity": 4,
            "exclusive_assignment_ref": null,
            "membership_refs": [],
            "responsibility_summary": role,
            "capability_refs": ["company.records.write", "company.work.execute"],
            "system_prompt_ref": format!("document-prompt-{id}"),
            "tool_refs": ["tool-company-records"],
            "skill_refs": ["skill-governed-work"],
            "maintained_document_refs": [],
            "accepted_work_type_refs": ["work-type-general"],
            "escalation_policy_ref": "policy-lead-escalation",
            "permission_policy_refs": ["company.records.write", "company.work.execute"],
            "runtime_refs": [],
            "native_session_refs": [],
            "created_at": NOW,
            "updated_at": NOW
        }
    })
}

#[test]
fn trademark_chain_projection_actions_and_payment_boundaries() {
    let home = TempHome::new("company-os-api");
    let project_id = init_project(&home);
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("FIRM_COMPANY_OS_TOKEN", TEST_TOKEN)],
    );
    let query = format!("?project={project_id}");

    let (status, body) = serve.post_json(
        "/v1/company-os/actors",
        &human("unauthenticated", "Unauthenticated"),
    );
    assert_eq!(status, 403, "{body}");
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actors?project=missing-project",
        &human("wrong-project", "Wrong Project"),
    );
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], "project_not_found");

    post_ok(
        &serve,
        "/v1/company-os/actors",
        human("human-brand-owner", "Brand Owner"),
    );
    post_ok(
        &serve,
        "/v1/company-os/actors",
        admin(agent_record(
            "agent-sales",
            "Sales Agent",
            "Run merchant outreach and return durable business evidence",
        )),
    );
    post_ok(
        &serve,
        "/v1/company-os/actors",
        admin(agent_record(
            "agent-company-lead",
            "Lead Agent",
            "Coordinate company priorities and governance",
        )),
    );
    post_ok(
        &serve,
        "/v1/company-os/actors",
        admin(agent_record(
            "agent-work-governance",
            "Work Governance Agent",
            "Create, classify, and route durable company work",
        )),
    );
    post_ok(
        &serve,
        "/v1/company-os/actors",
        admin(agent_record(
            "agent-trademark",
            "Trademark Agent",
            "Prepare trademark filing",
        )),
    );
    let mut finance_agent = agent_record(
        "agent-finance",
        "Finance Agent",
        "Review financial commitments",
    );
    finance_agent["actor"]["permission_policy_refs"] = json!([
        "company.records.write",
        "company.work.execute",
        "finance.payment.write"
    ]);
    post_ok(
        &serve,
        "/v1/company-os/actors",
        admin(finance_agent.clone()),
    );
    post_ok(
        &serve,
        "/v1/company-os/actors",
        admin(json!({
            "actor_type": "external",
            "actor": {
                "id": "external-lawyer",
                "display_name_or_organization": "External Lawyer",
                "engagement_scope": "Trademark filing review",
                "sponsor_actor_ref": actor("human", "human-brand-owner"),
                "access_expires_at": "2026-12-31T23:59:59+08:00",
                "confidentiality_or_contract_refs": ["contract-lawyer-2026"],
                "membership_refs": [],
                "restricted_permission_refs": ["trademark.review"],
                "status": "active",
                "created_at": NOW,
                "updated_at": NOW
            }
        })),
    );

    let document = json!({
        "id": "document-trademark-cn-2026-018",
        "space_id": "brand-ip",
        "parent_document_id": null,
        "title": "Trademark application CN-2026-018",
        "kind": "record",
        "lifecycle_status": "active",
        "block_ids": [],
        "template_ref": null,
        "permission_policy_refs": ["company.records.write"],
        "reference_refs": [],
        "created_by": actor("human", "human-brand-owner"),
        "updated_by": actor("human", "human-brand-owner"),
        "created_at": NOW,
        "updated_at": NOW
    });
    let (status, body) = post_json(&serve, "/v1/company-os/documents", &document);
    assert_eq!(status, 403, "{body}");
    post_ok(&serve, "/v1/company-os/documents", admin(document.clone()));
    let mut outside_document = document.clone();
    outside_document["id"] = json!("document-outside-trademark-module");
    outside_document["space_id"] = json!("operations");
    outside_document["title"] = json!("Outside module");
    post_ok(&serve, "/v1/company-os/documents", admin(outside_document));
    post_ok(
        &serve,
        "/v1/company-os/blocks",
        admin(json!({
            "id": "block-trademark-summary",
            "document_id": document["id"],
            "kind": "rich_text",
            "position": 0,
            "content": {"text": "Prepare CN trademark filing after approval."},
            "referenced_entities": [],
            "created_by": actor("agent", "agent-trademark"),
            "updated_by": actor("agent", "agent-trademark"),
            "created_at": NOW,
            "updated_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/org-units",
        admin(json!({
            "id": "org-brand-ip",
            "organization_id": "company",
            "name": "Brand & IP",
            "purpose": "Own brand and intellectual property",
            "parent_unit_id": null,
            "status": "active",
            "human_lead_actor_ref": actor("human", "human-brand-owner"),
            "agent_lead_actor_ref": null,
            "policy_refs": ["policy-finance"],
            "document_space_ref": "brand-ip",
            "created_at": NOW,
            "updated_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/memberships",
        admin(json!({
            "id": "membership-trademark-agent",
            "organization_id": "company",
            "org_unit_id": "org-brand-ip",
            "actor_ref": actor("agent", "agent-trademark"),
            "membership_role": "member",
            "title_or_function": "Trademark filing",
            "status": "active",
            "starts_at": NOW,
            "ends_at": null,
            "authority_policy_refs": [],
            "created_by_actor_ref": actor("human", "human-brand-owner"),
            "created_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/business-modules",
        admin(json!({
            "id": "module-trademark",
            "name": "Trademark Management",
            "purpose": "Govern trademark applications",
            "root_document_ref": document["id"],
            "record_types": ["trademark_application"],
            "relation_rules": [],
            "default_view_refs": [],
            "policy_refs": ["policy-finance"],
            "lifecycle_rules": ["proposal_before_activation"],
            "metric_definition_refs": [],
            "custom_page_definition_refs": [],
            "status": "draft",
            "owner": actor("human", "human-brand-owner"),
            "created_at": NOW,
            "updated_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/views",
        admin(json!({
            "id": "view-trademark-standard",
            "module_id": "module-trademark",
            "title": "Trademark applications",
            "mode": "table",
            "source_kinds": ["typed_record", "work_item", "financial_record"],
            "query": {"module_id": "module-trademark"},
            "owner": actor("human", "human-brand-owner"),
            "policy_refs": ["policy-finance"],
            "created_at": NOW,
            "updated_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/custom-page-packages",
        admin(json!({
            "id": "package-trademark",
            "definition_id": "page-trademark",
            "version": "1.0.0",
            "kind": "react",
            "artifact_ref": "artifact://trademark-page",
            "entrypoint": "TrademarkPage",
            "integrity_digest": "sha256:test",
            "built_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/custom-page-definitions",
        admin(json!({
            "id": "page-trademark",
            "module_id": "module-trademark",
            "purpose": "Present governed trademark records",
            "allowed_data_queries": [{
                "id": "query-finance",
                "source_kind": "financial_record",
                "source_scope": "module-trademark",
                "permission_policy_ref": "policy-finance"
            }],
            "approved_ui_components": ["FinancialRecordCard", "ApprovalCard"],
            "action_command_refs": [
                "typed_record.append", "relation.append",
                "work_item.append", "work_item.transition", "assignment.append",
                "commitment.propose", "commitment.append",
                "approval.request", "approval.decide", "payment.append"
            ],
            "standard_view_fallback_ref": "view-trademark-standard",
            "owner": actor("human", "human-brand-owner"),
            "package_ref": "package-trademark",
            "package_version": "1.0.0",
            "fixture_ref": "company-os-trademark-v1",
            "visual_contract_ref": "visual-contract-v1",
            "policy_refs": [
                "page-trademark:typed_record.append",
                "page-trademark:relation.append",
                "page-trademark:work_item.append",
                "page-trademark:approval.decide",
                "page-trademark:approval.request",
                "page-trademark:work_item.transition",
                "page-trademark:assignment.append",
                "page-trademark:commitment.propose",
                "page-trademark:commitment.append",
                "page-trademark:payment.append"
            ],
            "created_at": NOW,
            "updated_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/typed-records",
        admin(json!({
            "id": "trademark-application-cn-2026-018",
            "module_id": "module-trademark",
            "record_type": "trademark_application",
            "title": "CN-2026-018",
            "fields": {"jurisdiction": "CN", "mark": "Brand A"},
            "lifecycle_status": "filing_preparation",
            "source_document_ref": document["id"],
            "created_by": actor("agent", "agent-trademark"),
            "updated_by": actor("agent", "agent-trademark"),
            "created_at": NOW,
            "updated_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/milestones",
        admin(json!({
            "id": "milestone-trademark-submitted",
            "title": "Trademark application submitted",
            "outcome": "The governed filing has durable receipt evidence",
            "status": "active",
            "accountable_owner": actor("human", "human-brand-owner"),
            "source_document_ref": document["id"],
            "business_module_ref": "module-trademark",
            "target_at": "2026-07-31T18:00:00+08:00",
            "acceptance_criteria": ["Filing receipt is linked"],
            "work_item_refs": [],
            "created_at": NOW,
            "updated_at": NOW,
            "achieved_at": null
        })),
    );
    let work_item = json!({
        "id": "work-trademark-filing",
        "title": "Trademark filing for Brand A",
        "objective": "Prepare and submit the filing after approval",
        "status": "submitted",
        "source_document_ref": document["id"],
        "source_record_refs": ["trademark-application-cn-2026-018"],
        "milestone_ref": "milestone-trademark-submitted",
        "work_type": "legal",
        "business_module_ref": "module-trademark",
        "result_document_ref": null,
        "result_record_refs": [],
        "submitted_by": actor("agent", "agent-work-governance"),
        "requested_by": actor("agent", "agent-company-lead"),
        "accountable_owner": actor("human", "human-brand-owner"),
        "assignees": [actor("agent", "agent-trademark")],
        "contributors": [actor("external", "external-lawyer")],
        "reviewer": actor("agent", "agent-finance"),
        "approver": actor("human", "human-brand-owner"),
        "execution_mode": "mixed",
        "execution_refs": [],
        "approval_refs": [],
        "evidence_refs": ["evidence-filing-package"],
        "artifact_refs": [],
        "outcome_summary": null,
        "due_at": "2026-07-31T18:00:00+08:00",
        "priority": "high",
        "risk_level": "legal",
        "created_at": NOW,
        "updated_at": NOW,
        "completed_at": null
    });
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-create-trademark-work",
            "work_item.append",
            json!({"kind": "document", "id": "document-trademark-cn-2026-018"}),
            work_item.clone(),
            actor("agent", "agent-work-governance"),
            "page-trademark:work_item.append",
            "company.records.write",
            "r1",
            false,
            vec![],
            "audit-create-trademark-work",
        ),
    );
    let assignment = json!({
        "id": "assignment-trademark-agent",
        "work_item_id": "work-trademark-filing",
        "recipient": actor("agent", "agent-trademark"),
        "sender": actor("agent", "agent-work-governance"),
        "assigned_role": "filing owner",
        "scope": "Prepare CN filing",
        "delivery_state": "delivered",
        "delivery_policy_ref": "company.records.write",
        "correlation_id": "corr-trademark-018",
        "delivery_evidence_ref": "evidence-assignment-delivered",
        "assigned_at": NOW,
        "delivered_at": NOW,
        "acknowledged_at": null
    });
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-assign-trademark-work",
            "assignment.append",
            json!({"kind": "work_item", "id": "work-trademark-filing"}),
            assignment,
            actor("agent", "agent-work-governance"),
            "page-trademark:assignment.append",
            "company.records.write",
            "r1",
            false,
            vec![],
            "audit-assign-trademark-work",
        ),
    );

    // A non-financial path follows the same Lead -> Work Governance ->
    // Business Agent contract without creating a Commitment or Approval.
    let outreach_document = json!({
        "id": "document-merchant-outreach",
        "space_id": "brand-ip",
        "parent_document_id": document["id"],
        "title": "Merchant outreach brief",
        "kind": "page",
        "lifecycle_status": "active",
        "block_ids": [],
        "template_ref": null,
        "permission_policy_refs": ["company.records.write"],
        "reference_refs": [],
        "created_by": actor("human", "human-brand-owner"),
        "updated_by": actor("human", "human-brand-owner"),
        "created_at": NOW,
        "updated_at": NOW
    });
    post_ok(
        &serve,
        "/v1/company-os/documents",
        admin(outreach_document.clone()),
    );
    let outreach_work = json!({
        "id": "work-merchant-outreach",
        "title": "Contact candidate merchants",
        "objective": "Collect non-binding merchant interest and return structured notes",
        "status": "submitted",
        "source_document_ref": outreach_document["id"],
        "source_record_refs": [],
        "milestone_ref": null,
        "work_type": "operations",
        "business_module_ref": "module-trademark",
        "result_document_ref": null,
        "result_record_refs": [],
        "submitted_by": actor("agent", "agent-work-governance"),
        "requested_by": actor("agent", "agent-company-lead"),
        "accountable_owner": actor("human", "human-brand-owner"),
        "assignees": [actor("agent", "agent-sales")],
        "contributors": [],
        "reviewer": actor("human", "human-brand-owner"),
        "approver": null,
        "execution_mode": "direct",
        "execution_refs": [],
        "approval_refs": [],
        "evidence_refs": [],
        "artifact_refs": [],
        "outcome_summary": null,
        "due_at": null,
        "priority": "normal",
        "risk_level": "low",
        "created_at": NOW,
        "updated_at": NOW,
        "completed_at": null
    });
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-create-merchant-outreach",
            "work_item.append",
            json!({"kind": "document", "id": "document-merchant-outreach"}),
            outreach_work.clone(),
            actor("agent", "agent-work-governance"),
            "page-trademark:work_item.append",
            "company.records.write",
            "r1",
            false,
            vec![],
            "audit-create-merchant-outreach",
        ),
    );
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-assign-merchant-outreach",
            "assignment.append",
            json!({"kind": "work_item", "id": "work-merchant-outreach"}),
            json!({
                "id": "assignment-sales-outreach",
                "work_item_id": "work-merchant-outreach",
                "recipient": actor("agent", "agent-sales"),
                "sender": actor("agent", "agent-work-governance"),
                "assigned_role": "Merchant outreach owner",
                "scope": "Contact the approved candidate list; do not make a purchase or monetary promise",
                "delivery_state": "delivered",
                "delivery_policy_ref": "company.records.write",
                "correlation_id": "corr-merchant-outreach",
                "delivery_evidence_ref": "evidence-outreach-assignment-delivered",
                "assigned_at": NOW,
                "delivered_at": NOW,
                "acknowledged_at": null
            }),
            actor("agent", "agent-work-governance"),
            "page-trademark:assignment.append",
            "company.records.write",
            "r1",
            false,
            vec![],
            "audit-assign-merchant-outreach",
        ),
    );
    let mut outreach_in_progress = outreach_work.clone();
    outreach_in_progress["status"] = json!("in_progress");
    outreach_in_progress["updated_at"] = json!("2026-07-20T10:02:00+08:00");
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-start-merchant-outreach",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-merchant-outreach"}),
            outreach_in_progress.clone(),
            actor("agent", "agent-sales"),
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-start-merchant-outreach",
        ),
    );
    let mut outreach_in_review = outreach_in_progress;
    outreach_in_review["status"] = json!("in_review");
    outreach_in_review["result_document_ref"] = outreach_document["id"].clone();
    outreach_in_review["evidence_refs"] = json!(["evidence-merchant-conversation-notes"]);
    outreach_in_review["outcome_summary"] = json!(
        "Three merchants contacted; two requested a follow-up. No monetary commitment was made."
    );
    outreach_in_review["updated_at"] = json!("2026-07-20T10:12:00+08:00");
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-submit-merchant-outreach",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-merchant-outreach"}),
            outreach_in_review.clone(),
            actor("agent", "agent-sales"),
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-submit-merchant-outreach",
        ),
    );
    let mut outreach_completed = outreach_in_review;
    outreach_completed["status"] = json!("completed");
    outreach_completed["completed_at"] = json!("2026-07-20T10:14:00+08:00");
    outreach_completed["updated_at"] = json!("2026-07-20T10:14:00+08:00");
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-complete-merchant-outreach",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-merchant-outreach"}),
            outreach_completed,
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-complete-merchant-outreach",
        ),
    );
    let (status, nonfinancial_snapshot) =
        serve.get_json(&format!("/v1/company-os/snapshot{query}"));
    assert_eq!(status, 200, "{nonfinancial_snapshot}");
    assert_eq!(nonfinancial_snapshot["result"]["commitments"], json!([]));
    assert_eq!(nonfinancial_snapshot["result"]["approvals"], json!([]));

    post_ok(
        &serve,
        "/v1/company-os/relations",
        admin(json!({
            "id": "relation-application-work",
            "from_ref": {"kind": "typed_record", "id": "trademark-application-cn-2026-018"},
            "relation_type": "implemented_by",
            "to_ref": {"kind": "work_item", "id": "work-trademark-filing"},
            "provenance_ref": {"kind": "document", "id": "document-trademark-cn-2026-018"},
            "created_by": actor("human", "human-brand-owner"),
            "created_at": NOW
        })),
    );

    let commitment = json!({
        "id": "commitment-trademark-fee",
        "amount": {"amount": "3000", "currency": "CNY"},
        "status": "proposed",
        "source_document_id": document["id"],
        "submitted_by": actor("agent", "agent-finance"),
        "accountable_owner": actor("human", "human-brand-owner"),
        "relation_ids": ["relation-application-work"],
        "evidence_refs": ["evidence-fee-quote"],
        "approval_refs": [],
        "audit_event_ids": ["audit-commitment-created"],
        "due_at": "2026-07-31T18:00:00+08:00",
        "created_at": NOW,
        "updated_at": NOW
    });
    let (status, body) = post_json(&serve, "/v1/company-os/documents", &document);
    assert_eq!(status, 403, "{body}");
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-propose-trademark-commitment",
            "commitment.propose",
            json!({"kind": "work_item", "id": "work-trademark-filing"}),
            commitment.clone(),
            "page-trademark:commitment.propose",
            "finance.commitment.write",
            "r2",
            false,
            vec![],
            "audit-commitment-created",
        ),
    );

    // A Payment is never inferred from a Commitment, and a direct append cannot
    // bypass the Human approval/evidence/commitment gates.
    let invalid_payment = json!({
        "id": "payment-trademark-fee",
        "amount": {"amount": "3000", "currency": "CNY"},
        "status": "prepared",
        "source_document_id": document["id"],
        "submitted_by": actor("human", "human-brand-owner"),
        "accountable_owner": actor("human", "human-brand-owner"),
        "related_commitment_refs": [],
        "relation_ids": [],
        "evidence_refs": [],
        "approval_refs": [],
        "audit_event_ids": ["audit-payment"],
        "occurred_at": null,
        "created_at": NOW,
        "updated_at": NOW
    });
    let (status, error) = post_json(&serve, "/v1/company-os/payments", &invalid_payment);
    assert_eq!(status, 403, "{error}");
    assert_eq!(error["error"], "forbidden");

    let requested_commitment_approval = json!({
        "id": "approval-trademark-commitment",
        "subject_ref": {"kind": "financial_record", "id": "commitment-trademark-fee"},
        "action_summary": "Authorize commitment.append for the trademark fee",
        "requested_by": actor("agent", "agent-finance"),
        "required_approver_refs": [actor("human", "human-brand-owner")],
        "required_actor_type": "human",
        "policy_ref": "page-trademark:commitment.append",
        "status": "requested",
        "decided_by": [],
        "decision_note": null,
        "evidence_refs": ["evidence-fee-quote"],
        "requested_at": NOW,
        "decided_at": null,
        "expires_at": "2026-12-31T23:59:59+08:00"
    });
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-request-trademark-commitment-approval",
            "approval.request",
            json!({"kind": "financial_record", "id": "commitment-trademark-fee"}),
            requested_commitment_approval.clone(),
            actor("agent", "agent-finance"),
            "page-trademark:approval.request",
            "company.records.write",
            "r1",
            false,
            vec![],
            "audit-request-trademark-commitment-approval",
        ),
    );
    let mut queued_commitment = commitment.clone();
    queued_commitment["status"] = json!("pending_approval");
    queued_commitment["approval_refs"] = json!(["approval-trademark-commitment"]);
    queued_commitment["audit_event_ids"] = json!(["audit-commitment-entered-queue"]);
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-trademark-commitment-enter-queue",
            "commitment.append",
            json!({"kind": "financial_record", "id": "commitment-trademark-fee"}),
            queued_commitment,
            "page-trademark:commitment.append",
            "finance.commitment.write",
            "r3",
            true,
            vec!["approval-trademark-commitment"],
            "audit-commitment-entered-queue",
        ),
    );

    // WorkItem execution is a governed transition, not a broad append. The
    // assignee can start and submit durable results, but cannot complete work
    // while a linked Approval is still requested.
    let mut in_progress_work = work_item.clone();
    in_progress_work["approval_refs"] = json!(["approval-trademark-commitment"]);
    in_progress_work["status"] = json!("in_progress");
    in_progress_work["updated_at"] = json!("2026-07-20T10:05:00+08:00");
    let mut rewritten_work = in_progress_work.clone();
    rewritten_work["title"] = json!("Rewritten outside the lifecycle contract");
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &action_by(
            "action-rewrite-trademark-work",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-trademark-filing"}),
            rewritten_work,
            actor("agent", "agent-trademark"),
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-rewrite-trademark-work",
        ),
    );
    assert_eq!(status, 409, "{body}");
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &action_by(
            "action-unowned-trademark-work",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-trademark-filing"}),
            in_progress_work.clone(),
            actor("agent", "agent-finance"),
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-unowned-trademark-work",
        ),
    );
    assert_eq!(status, 403, "{body}");
    assert!(body["detail"]
        .as_str()
        .is_some_and(|detail| detail.contains("does not own")));
    let mut outside_result_work = in_progress_work.clone();
    outside_result_work["result_document_ref"] = json!("document-outside-trademark-module");
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &action_by(
            "action-cross-module-trademark-work",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-trademark-filing"}),
            outside_result_work,
            actor("agent", "agent-trademark"),
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-cross-module-trademark-work",
        ),
    );
    assert_eq!(status, 403, "{body}");
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-start-trademark-work",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-trademark-filing"}),
            in_progress_work.clone(),
            actor("agent", "agent-trademark"),
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-start-trademark-work",
        ),
    );
    let mut in_review_work = in_progress_work;
    in_review_work["status"] = json!("in_review");
    in_review_work["result_document_ref"] = document["id"].clone();
    in_review_work["result_record_refs"] = json!(["trademark-application-cn-2026-018"]);
    in_review_work["evidence_refs"] = json!(["evidence-filing-package", "evidence-filing-receipt"]);
    in_review_work["artifact_refs"] = json!(["artifact://trademark/filing-package-v1"]);
    in_review_work["outcome_summary"] =
        json!("Filing package prepared and submitted for accountable review.");
    in_review_work["updated_at"] = json!("2026-07-20T10:10:00+08:00");
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-submit-trademark-result",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-trademark-filing"}),
            in_review_work.clone(),
            actor("agent", "agent-trademark"),
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-submit-trademark-result",
        ),
    );
    let mut completed_work = in_review_work;
    completed_work["status"] = json!("completed");
    completed_work["completed_at"] = json!("2026-07-20T10:20:00+08:00");
    completed_work["updated_at"] = json!("2026-07-20T10:20:00+08:00");
    let premature_complete = action(
        "action-complete-trademark-work-before-approval",
        "work_item.transition",
        json!({"kind": "work_item", "id": "work-trademark-filing"}),
        completed_work.clone(),
        "page-trademark:work_item.transition",
        "company.work.execute",
        "r2",
        false,
        vec![],
        "audit-complete-trademark-work-before-approval",
    );
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &premature_complete,
    );
    assert_eq!(status, 403, "{body}");
    assert!(body["detail"]
        .as_str()
        .is_some_and(|detail| detail.contains("every linked Approval")));

    let mut approved_commitment_approval = requested_commitment_approval;
    approved_commitment_approval["status"] = json!("approved");
    approved_commitment_approval["decided_by"] = json!([actor("human", "human-brand-owner")]);
    approved_commitment_approval["decision_note"] = json!("Approved against the fee quote");
    approved_commitment_approval["decided_at"] = json!(NOW);
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/approvals",
        &admin(approved_commitment_approval.clone()),
    );
    assert_eq!(status, 403, "{body}");
    let result = post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-approve-trademark-commitment",
            "approval.decide",
            json!({"kind": "approval", "id": "approval-trademark-commitment"}),
            approved_commitment_approval,
            "page-trademark:approval.decide",
            "company.approve",
            "r2",
            false,
            vec![],
            "audit-approve-trademark-commitment",
        ),
    );
    assert_eq!(result["command"]["status"], "executed");

    let completed = post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-complete-trademark-work",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-trademark-filing"}),
            completed_work.clone(),
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-complete-trademark-work",
        ),
    );
    assert_eq!(completed["record"]["status"], "completed");
    let replay = post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-complete-trademark-work",
            "work_item.transition",
            json!({"kind": "work_item", "id": "work-trademark-filing"}),
            completed_work,
            "page-trademark:work_item.transition",
            "company.work.execute",
            "r2",
            false,
            vec![],
            "audit-complete-trademark-work",
        ),
    );
    assert_eq!(replay["idempotent_replay"], true);

    // The accepted outcome returns to the original Docs truth. Since R3 retired
    // the document.append/block.append Actions, the return is written through the
    // generic document/block resource endpoints (latest-row-wins appends).
    let result_block = json!({
        "id": "block-trademark-filing-result",
        "document_id": document["id"],
        "kind": "callout",
        "position": 1,
        "content": {
            "title": "Filing completed",
            "text": "CN filing receipt returned from WorkItem work-trademark-filing.",
            "evidence_refs": ["evidence-filing-receipt"]
        },
        "referenced_entities": [
            {"kind": "work_item", "id": "work-trademark-filing"},
            {"kind": "typed_record", "id": "trademark-application-cn-2026-018"}
        ],
        "created_by": actor("agent", "agent-trademark"),
        "updated_by": actor("agent", "agent-trademark"),
        "created_at": "2026-07-20T10:21:00+08:00",
        "updated_at": "2026-07-20T10:21:00+08:00"
    });
    post_ok(&serve, "/v1/company-os/blocks", admin(result_block));
    let mut returned_document = document.clone();
    returned_document["block_ids"] = json!(["block-trademark-filing-result"]);
    returned_document["reference_refs"] = json!([
        {"kind": "work_item", "id": "work-trademark-filing"},
        {"kind": "typed_record", "id": "trademark-application-cn-2026-018"},
        {"kind": "financial_record", "id": "commitment-trademark-fee"}
    ]);
    returned_document["updated_by"] = actor("agent", "agent-trademark");
    returned_document["updated_at"] = json!("2026-07-20T10:21:00+08:00");
    post_ok(&serve, "/v1/company-os/documents", admin(returned_document));
    let mut returned_record = json!({
        "id": "trademark-application-cn-2026-018",
        "module_id": "module-trademark",
        "record_type": "trademark_application",
        "title": "CN-2026-018",
        "fields": {
            "jurisdiction": "CN",
            "mark": "Brand A",
            "filing_status": "filed",
            "filing_receipt_ref": "evidence-filing-receipt",
            "work_item_ref": "work-trademark-filing"
        },
        "lifecycle_status": "filed",
        "source_document_ref": document["id"],
        "created_by": actor("agent", "agent-trademark"),
        "updated_by": actor("agent", "agent-trademark"),
        "created_at": NOW,
        "updated_at": "2026-07-20T10:21:00+08:00"
    });
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action_by(
            "action-return-trademark-typed-record",
            "typed_record.append",
            json!({"kind": "typed_record", "id": "trademark-application-cn-2026-018"}),
            returned_record.take(),
            actor("agent", "agent-trademark"),
            "page-trademark:typed_record.append",
            "company.records.write",
            "r1",
            false,
            vec![],
            "audit-return-trademark-typed-record",
        ),
    );

    let mut second_commitment = commitment.clone();
    second_commitment["id"] = json!("commitment-other-fee");
    second_commitment["audit_event_ids"] = json!(["audit-other-commitment-created"]);
    post_ok(
        &serve,
        "/v1/company-os/commitments",
        admin(second_commitment.clone()),
    );
    second_commitment["status"] = json!("pending_approval");
    second_commitment["approval_refs"] = json!(["approval-trademark-commitment"]);
    second_commitment["audit_event_ids"] = json!(["audit-other-commitment-pending"]);
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &action(
            "action-cross-commitment-approval",
            "commitment.append",
            json!({"kind": "financial_record", "id": "commitment-trademark-fee"}),
            second_commitment,
            "page-trademark:commitment.append",
            "finance.commitment.write",
            "r3",
            true,
            vec!["approval-trademark-commitment"],
            "audit-other-commitment-pending",
        ),
    );
    assert_eq!(status, 403, "{body}");

    // Commitment state transitions remain append-only and separately audited.
    let mut pending_commitment = commitment.clone();
    pending_commitment["status"] = json!("pending_approval");
    pending_commitment["approval_refs"] = json!(["approval-trademark-commitment"]);
    pending_commitment["audit_event_ids"] =
        json!(["audit-commitment-created", "audit-commitment-pending"]);
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-trademark-commitment-pending",
            "commitment.append",
            json!({"kind": "financial_record", "id": "commitment-trademark-fee"}),
            pending_commitment.clone(),
            "page-trademark:commitment.append",
            "finance.commitment.write",
            "r3",
            true,
            vec!["approval-trademark-commitment"],
            "audit-commitment-pending",
        ),
    );
    let mut approved_commitment = pending_commitment;
    approved_commitment["status"] = json!("approved");
    approved_commitment["audit_event_ids"] = json!([
        "audit-commitment-created",
        "audit-commitment-pending",
        "audit-commitment-approved"
    ]);
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-trademark-commitment-approved",
            "commitment.append",
            json!({"kind": "financial_record", "id": "commitment-trademark-fee"}),
            approved_commitment,
            "page-trademark:commitment.append",
            "finance.commitment.write",
            "r3",
            true,
            vec!["approval-trademark-commitment"],
            "audit-commitment-approved",
        ),
    );

    let requested_payment_approval = json!({
        "id": "approval-trademark-payment",
        "subject_ref": {"kind": "financial_record", "id": "commitment-trademark-fee"},
        "action_summary": "Authorize payment.append for the trademark filing",
        "requested_by": actor("agent", "agent-finance"),
        "required_approver_refs": [actor("human", "human-brand-owner")],
        "required_actor_type": "human",
        "policy_ref": "page-trademark:payment.append",
        "status": "requested",
        "decided_by": [],
        "decision_note": null,
        "evidence_refs": ["evidence-payment-review"],
        "requested_at": NOW,
        "decided_at": null,
        "expires_at": "2026-12-31T23:59:59+08:00"
    });
    post_ok(
        &serve,
        "/v1/company-os/approvals",
        admin(requested_payment_approval.clone()),
    );
    let mut approved_payment_approval = requested_payment_approval;
    approved_payment_approval["status"] = json!("approved");
    approved_payment_approval["decided_by"] = json!([actor("human", "human-brand-owner")]);
    approved_payment_approval["decision_note"] = json!("Approved against the fee quote");
    approved_payment_approval["decided_at"] = json!(NOW);
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-approve-trademark-payment",
            "approval.decide",
            json!({"kind": "approval", "id": "approval-trademark-payment"}),
            approved_payment_approval,
            "page-trademark:approval.decide",
            "company.approve",
            "r2",
            false,
            vec![],
            "audit-approve-trademark-payment",
        ),
    );
    let payment = json!({
        "id": "payment-trademark-fee",
        "amount": {"amount": "3000", "currency": "CNY"},
        "status": "prepared",
        "source_document_id": document["id"],
        "submitted_by": actor("human", "human-brand-owner"),
        "accountable_owner": actor("human", "human-brand-owner"),
        "related_commitment_refs": ["commitment-trademark-fee"],
        "relation_ids": [],
        "evidence_refs": ["evidence-payment-instruction"],
        "approval_refs": ["approval-trademark-payment"],
        "audit_event_ids": ["audit-payment"],
        "occurred_at": null,
        "created_at": NOW,
        "updated_at": NOW
    });
    let payment_action = action(
        "action-pay-trademark-fee",
        "payment.append",
        json!({"kind": "financial_record", "id": "commitment-trademark-fee"}),
        payment.clone(),
        "page-trademark:payment.append",
        "finance.payment.write",
        "r3",
        true,
        vec!["approval-trademark-payment"],
        "audit-payment",
    );
    let result = post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        payment_action.clone(),
    );
    assert_eq!(result["command"]["status"], "executed");
    let replay = post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        payment_action.clone(),
    );
    assert_eq!(replay["idempotent_replay"], true);

    // Later Payment transitions use a Payment-scoped Human approval, not the
    // earlier Commitment-scoped approval that authorized creation.
    let requested_processing_approval = json!({
        "id": "approval-trademark-payment-processing",
        "subject_ref": {"kind": "financial_record", "id": "payment-trademark-fee"},
        "action_summary": "Authorize payment.append processing transition",
        "requested_by": actor("agent", "agent-finance"),
        "required_approver_refs": [actor("human", "human-brand-owner")],
        "required_actor_type": "human",
        "policy_ref": "page-trademark:payment.append",
        "status": "requested",
        "decided_by": [],
        "decision_note": null,
        "evidence_refs": ["evidence-processing-review"],
        "requested_at": NOW,
        "decided_at": null,
        "expires_at": "2026-12-31T23:59:59+08:00"
    });
    post_ok(
        &serve,
        "/v1/company-os/approvals",
        admin(requested_processing_approval.clone()),
    );
    let mut approved_processing_approval = requested_processing_approval;
    approved_processing_approval["status"] = json!("approved");
    approved_processing_approval["decided_by"] = json!([actor("human", "human-brand-owner")]);
    approved_processing_approval["decision_note"] = json!("Approved for processing");
    approved_processing_approval["decided_at"] = json!(NOW);
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-approve-payment-processing",
            "approval.decide",
            json!({"kind": "approval", "id": "approval-trademark-payment-processing"}),
            approved_processing_approval,
            "page-trademark:approval.decide",
            "company.approve",
            "r2",
            false,
            vec![],
            "audit-approve-payment-processing",
        ),
    );
    let mut pending_payment = payment.clone();
    pending_payment["status"] = json!("pending_approval");
    pending_payment["approval_refs"] = json!(["approval-trademark-payment-processing"]);
    pending_payment["audit_event_ids"] = json!(["audit-payment", "audit-payment-pending"]);
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-payment-pending",
            "payment.append",
            json!({"kind": "financial_record", "id": "payment-trademark-fee"}),
            pending_payment.clone(),
            "page-trademark:payment.append",
            "finance.payment.write",
            "r3",
            true,
            vec!["approval-trademark-payment-processing"],
            "audit-payment-pending",
        ),
    );
    let mut processing_payment = pending_payment;
    processing_payment["status"] = json!("processing");
    processing_payment["audit_event_ids"] = json!([
        "audit-payment",
        "audit-payment-pending",
        "audit-payment-processing"
    ]);
    post_ok(
        &serve,
        "/v1/company-os/actions/dispatch",
        action(
            "action-payment-processing",
            "payment.append",
            json!({"kind": "financial_record", "id": "payment-trademark-fee"}),
            processing_payment,
            "page-trademark:payment.append",
            "finance.payment.write",
            "r3",
            true,
            vec!["approval-trademark-payment-processing"],
            "audit-payment-processing",
        ),
    );

    let mut conflicting_retry = payment_action.clone();
    conflicting_retry["payload"]["record"]["evidence_refs"] = json!(["different-evidence"]);
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &conflicting_retry,
    );
    assert_eq!(status, 409, "{body}");
    let mut governance_ref_conflict = payment_action.clone();
    governance_ref_conflict["audit_event_refs"] = json!(["audit-payment-rebound"]);
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &governance_ref_conflict,
    );
    assert_eq!(status, 409, "{body}");

    // Audit IDs are claimed before the effect. Rebinding a Payment audit ID
    // cannot partially transition the Commitment.
    let (status, commitment_detail) =
        serve.get_json("/v1/company-os/commitments/commitment-trademark-fee");
    assert_eq!(status, 200, "{commitment_detail}");
    let mut forged_fulfillment = commitment_detail["result"].clone();
    forged_fulfillment["status"] = json!("fulfilled");
    forged_fulfillment["audit_event_ids"] = json!(["audit-payment"]);
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &action(
            "action-rebind-audit-before-effect",
            "commitment.append",
            json!({"kind": "financial_record", "id": "commitment-trademark-fee"}),
            forged_fulfillment,
            "page-trademark:commitment.append",
            "finance.commitment.write",
            "r3",
            true,
            vec!["approval-trademark-commitment"],
            "audit-payment",
        ),
    );
    assert_eq!(status, 409, "{body}");
    let (status, commitment_detail) =
        serve.get_json("/v1/company-os/commitments/commitment-trademark-fee");
    assert_eq!(status, 200, "{commitment_detail}");
    assert_eq!(commitment_detail["result"]["status"], "approved");

    let mut out_of_scope_payment = payment.clone();
    out_of_scope_payment["id"] = json!("payment-outside-module");
    out_of_scope_payment["source_document_id"] = json!("document-outside-trademark-module");
    out_of_scope_payment["audit_event_ids"] = json!(["audit-payment-outside-module"]);
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &action(
            "action-payment-outside-module",
            "payment.append",
            json!({"kind": "financial_record", "id": "commitment-trademark-fee"}),
            out_of_scope_payment,
            "page-trademark:payment.append",
            "finance.payment.write",
            "r3",
            true,
            vec!["approval-trademark-payment"],
            "audit-payment-outside-module",
        ),
    );
    assert_eq!(status, 403, "{body}");

    let expired_request = json!({
        "id": "approval-expired-same-day",
        "subject_ref": {"kind": "financial_record", "id": "commitment-trademark-fee"},
        "action_summary": "Authorize payment.append after expiry",
        "requested_by": actor("agent", "agent-finance"),
        "required_approver_refs": [actor("human", "human-brand-owner")],
        "required_actor_type": "human",
        "policy_ref": "page-trademark:payment.append",
        "status": "requested",
        "decided_by": [],
        "decision_note": null,
        "evidence_refs": ["evidence-expired-request"],
        "requested_at": "2026-07-19T23:00:00+08:00",
        "decided_at": null,
        "expires_at": "2026-07-20T00:00:00+08:00"
    });
    post_ok(
        &serve,
        "/v1/company-os/approvals",
        admin(expired_request.clone()),
    );
    let mut expired_decision = expired_request;
    expired_decision["status"] = json!("approved");
    expired_decision["decided_by"] = json!([actor("human", "human-brand-owner")]);
    expired_decision["decision_note"] = json!("too late");
    expired_decision["decided_at"] = json!(NOW);
    let (status, body) = post_json(
        &serve,
        "/v1/company-os/actions/dispatch",
        &action(
            "action-expired-same-day",
            "approval.decide",
            json!({"kind": "approval", "id": "approval-expired-same-day"}),
            expired_decision,
            "page-trademark:approval.decide",
            "company.approve",
            "r2",
            false,
            vec![],
            "audit-expired-same-day",
        ),
    );
    assert_eq!(status, 403, "{body}");

    // Risk, permission, and approval requirements are resolved from the
    // server-owned ActionPolicyDefinition, never trusted from request flags.
    let mut forged_policy = action(
        "action-forged-payment-policy",
        "payment.append",
        json!({"kind": "financial_record", "id": "commitment-trademark-fee"}),
        payment,
        "page-trademark:payment.append",
        "finance.payment.write",
        "r1",
        false,
        vec![],
        "audit-forged-payment-policy",
    );
    let (status, body) = post_json(&serve, "/v1/company-os/actions/dispatch", &forged_policy);
    assert_eq!(status, 403, "{body}");
    // Actor lifecycle is authoritative at dispatch time.
    finance_agent["actor"]["status"] = json!("paused");
    finance_agent["actor"]["availability"] = json!("paused");
    post_ok(&serve, "/v1/company-os/actors", admin(finance_agent));
    forged_policy["id"] = json!("action-paused-agent-payment");
    forged_policy["requested_by"] = actor("agent", "agent-finance");
    forged_policy["risk_tier"] = json!("r3");
    forged_policy["requires_human_approval"] = json!(true);
    forged_policy["approval_refs"] = json!(["approval-trademark-payment"]);
    forged_policy["audit_event_refs"] = json!(["audit-paused-agent-payment"]);
    let (status, body) = post_json(&serve, "/v1/company-os/actions/dispatch", &forged_policy);
    assert_eq!(status, 403, "{body}");
    let replay_after_pause = post_ok(&serve, "/v1/company-os/actions/dispatch", payment_action);
    assert_eq!(replay_after_pause["idempotent_replay"], true);
    assert_eq!(replay_after_pause["record"]["id"], "payment-trademark-fee");

    // Undeclared commands and actors without permission are explicit 4xx.
    let mut undeclared = json!({
        "id": "action-undeclared",
        "command_name": "work_item.delete",
        "subject_ref": {"kind": "work_item", "id": "work-trademark-filing"},
        "requested_by": actor("human", "human-brand-owner"),
        "payload": {"definition_id": "page-trademark", "record": {"id": "work-trademark-filing"}},
        "required_permission": "company.records.write",
        "policy_ref": "page-trademark:approval.decide",
        "risk_tier": "r1",
        "requires_human_approval": false,
        "approval_refs": [],
        "status": "requested",
        "audit_event_refs": ["audit-undeclared"],
        "requested_at": NOW,
        "completed_at": null
    });
    let (status, body) = post_json(&serve, "/v1/company-os/actions/dispatch", &undeclared);
    assert_eq!(status, 403, "{body}");
    undeclared["command_name"] = json!("approval.decide");
    undeclared["required_permission"] = json!("permission-not-granted");
    let (status, body) = post_json(&serve, "/v1/company-os/actions/dispatch", &undeclared);
    assert_eq!(status, 403, "{body}");

    let (status, snapshot) = serve.get_json(&format!("/v1/company-os/snapshot{query}"));
    assert_eq!(status, 200, "{snapshot}");
    assert_eq!(snapshot["result"]["snapshot_contract"], "company-os-v1");
    assert_eq!(snapshot["result"]["source"]["kind"], "firm_store");
    assert_eq!(snapshot["result"]["source"]["authoritative"], true);
    assert_eq!(snapshot["result"]["source"]["project_id"], project_id);
    assert_eq!(snapshot["result"]["source"]["schema"], "company-os/v1");
    assert!(snapshot["result"]["source"]["revision"]
        .as_str()
        .is_some_and(|value| value.starts_with("fnv1a64:")));
    assert_eq!(
        snapshot["result"]["work_items"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        snapshot["result"]["milestones"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(snapshot["result"]["work"]["summary"]["total"], 2);
    assert_eq!(
        snapshot["result"]["work"]["work_types"]["legal"],
        json!(["work-trademark-filing"])
    );
    assert_eq!(
        snapshot["result"]["work"]["work_types"]["operations"],
        json!(["work-merchant-outreach"])
    );
    assert_eq!(
        snapshot["result"]["work"]["business_lines"]["module-trademark"],
        json!(["work-merchant-outreach", "work-trademark-filing"])
    );
    assert_eq!(
        snapshot["result"]["commitments"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        snapshot["result"]["payments"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        snapshot["result"]["action_policy_definitions"]
            .as_array()
            .map(Vec::len),
        Some(10)
    );
    assert_eq!(
        snapshot["result"]["action_commands"]
            .as_array()
            .map(Vec::len),
        Some(22)
    );
    assert_eq!(
        snapshot["result"]["audit_events"].as_array().map(Vec::len),
        Some(44)
    );

    let (status, detail) = serve.get_json("/v1/company-os/work-items/work-trademark-filing");
    assert_eq!(status, 200, "{detail}");
    assert_eq!(detail["result"]["title"], "Trademark filing for Brand A");
    assert_eq!(detail["result"]["status"], "completed");
    assert_eq!(
        detail["result"]["result_document_ref"],
        "document-trademark-cn-2026-018"
    );
    assert_eq!(detail["result"]["work_type"], "legal");
    assert_eq!(
        detail["result"]["milestone_ref"],
        "milestone-trademark-submitted"
    );
    assert_eq!(
        detail["result"]["result_record_refs"],
        json!(["trademark-application-cn-2026-018"])
    );
    let (status, returned_document) =
        serve.get_json("/v1/company-os/documents/document-trademark-cn-2026-018");
    assert_eq!(status, 200, "{returned_document}");
    assert_eq!(
        returned_document["result"]["block_ids"],
        json!(["block-trademark-filing-result"])
    );
    assert!(returned_document["result"]["reference_refs"]
        .as_array()
        .is_some_and(|refs| refs.iter().any(|reference| {
            reference["kind"] == "financial_record" && reference["id"] == "commitment-trademark-fee"
        })));
    let (status, returned_record) =
        serve.get_json("/v1/company-os/typed-records/trademark-application-cn-2026-018");
    assert_eq!(status, 200, "{returned_record}");
    assert_eq!(returned_record["result"]["lifecycle_status"], "filed");
    assert_eq!(
        returned_record["result"]["fields"]["filing_receipt_ref"],
        "evidence-filing-receipt"
    );
    let (status, list) = serve.get_json("/v1/company-os/actors");
    assert_eq!(status, 200, "{list}");
    assert_eq!(list["result"]["count"], 7);
    let (status, work_projection) = serve.get_json("/v1/company-os/work-projection");
    assert_eq!(status, 200, "{work_projection}");
    assert_eq!(work_projection["result"]["summary"]["total"], 2);
    assert_eq!(
        work_projection["result"]["milestones"][0]["progress_percent"],
        100
    );
    let (status, filtered_work) = post_json(
        &serve,
        "/v1/company-os/work-query",
        &json!({
            "statuses": ["completed"],
            "work_types": ["legal"],
            "business_module_refs": ["module-trademark"],
            "milestone_refs": ["milestone-trademark-submitted"],
            "accountable_owner": actor("human", "human-brand-owner"),
            "assignee": actor("agent", "agent-trademark")
        }),
    );
    assert_eq!(status, 200, "{filtered_work}");
    assert_eq!(filtered_work["result"]["summary"]["total"], 1);
    assert_eq!(
        filtered_work["result"]["work_items"][0]["id"],
        "work-trademark-filing"
    );

    let (status, dashboard) = serve.get_json(&format!("/v1/snapshot{query}"));
    assert_eq!(status, 200, "{dashboard}");
    assert_eq!(
        dashboard["company_os"]["snapshot_contract"],
        "company-os-v1"
    );
    assert_eq!(dashboard["company_os"]["source"]["authoritative"], true);
    assert_eq!(
        dashboard["company_os"]["financial_records"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
}

#[test]
fn archived_source_provenance_and_docs_health_reporting() {
    let home = TempHome::new("company-os-archived-source");
    let project_id = init_project(&home);
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("FIRM_COMPANY_OS_TOKEN", TEST_TOKEN)],
    );

    // An empty store reports a passing Docs health report.
    let (status, health) = serve.get_json("/v1/company-os/docs-health");
    assert_eq!(status, 200, "{health}");
    assert_eq!(health["result"]["status"], "pass");
    assert_eq!(health["result"]["counts"]["findings"], 0);

    post_ok(
        &serve,
        "/v1/company-os/actors",
        human("human-brand-owner", "Brand Owner"),
    );
    let mut maintainer = agent_record("agent-docs", "Docs Agent", "Maintain company memory");
    maintainer["actor"]["maintained_document_refs"] = json!(["document-operating-manual"]);
    post_ok(&serve, "/v1/company-os/actors", admin(maintainer.clone()));

    let document = json!({
        "id": "document-operating-manual",
        "space_id": "operations",
        "parent_document_id": null,
        "title": "Company operating manual",
        "kind": "page",
        "lifecycle_status": "active",
        "block_ids": [],
        "template_ref": null,
        "permission_policy_refs": ["company.records.write"],
        "reference_refs": [],
        "created_by": actor("human", "human-brand-owner"),
        "updated_by": actor("human", "human-brand-owner"),
        "created_at": NOW,
        "updated_at": NOW
    });
    post_ok(&serve, "/v1/company-os/documents", admin(document.clone()));

    let work_item = json!({
        "id": "work-refresh-operating-manual",
        "title": "Refresh operating manual",
        "objective": "Keep the operating manual current",
        "status": "in_progress",
        "source_document_ref": "document-operating-manual",
        "source_record_refs": [],
        "milestone_ref": null,
        "work_type": "governance",
        "business_module_ref": null,
        "result_document_ref": null,
        "result_record_refs": [],
        "submitted_by": actor("agent", "agent-docs"),
        "requested_by": actor("human", "human-brand-owner"),
        "accountable_owner": actor("human", "human-brand-owner"),
        "assignees": [actor("agent", "agent-docs")],
        "contributors": [],
        "reviewer": null,
        "approver": null,
        "execution_mode": "direct",
        "execution_refs": [],
        "approval_refs": [],
        "evidence_refs": [],
        "artifact_refs": [],
        "outcome_summary": null,
        "due_at": null,
        "priority": "medium",
        "risk_level": "governance",
        "created_at": NOW,
        "updated_at": NOW,
        "completed_at": null
    });
    post_ok(
        &serve,
        "/v1/company-os/work-items",
        admin(work_item.clone()),
    );

    // Active Work resolves to the active source Document with title+lifecycle.
    let (status, provenance) = serve.get_json("/v1/company-os/work-provenance");
    assert_eq!(status, 200, "{provenance}");
    let item = &provenance["result"]["work_items"][0];
    assert_eq!(item["work_item_id"], "work-refresh-operating-manual");
    assert_eq!(item["is_active"], true);
    assert_eq!(item["source"]["resolution"], "active");
    assert_eq!(item["source"]["title"], "Company operating manual");
    assert_eq!(item["source"]["lifecycle_status"], "active");
    assert_eq!(provenance["result"]["summary"]["active_source_active"], 1);
    let (status, health) = serve.get_json("/v1/company-os/docs-health");
    assert_eq!(status, 200, "{health}");
    assert_eq!(
        health["result"]["status"], "pass",
        "an active source must not produce findings: {health}"
    );

    // Archive the source Document through an ordinary governed append.
    let mut archived_document = document.clone();
    archived_document["lifecycle_status"] = json!("archived");
    archived_document["updated_at"] = json!("2026-07-21T10:00:00+08:00");
    post_ok(&serve, "/v1/company-os/documents", admin(archived_document));

    // The active WorkItem now resolves to explicit archived-source history
    // with title and lifecycle, never a bare id.
    let (status, provenance) = serve.get_json("/v1/company-os/work-provenance");
    assert_eq!(status, 200, "{provenance}");
    let item = &provenance["result"]["work_items"][0];
    assert_eq!(item["source"]["resolution"], "archived");
    assert_eq!(item["source"]["title"], "Company operating manual");
    assert_eq!(item["source"]["lifecycle_status"], "archived");
    assert_eq!(provenance["result"]["summary"]["active_source_archived"], 1);
    assert_eq!(provenance["result"]["summary"]["active_source_missing"], 0);

    let (status, health) = serve.get_json("/v1/company-os/docs-health");
    assert_eq!(status, 200, "{health}");
    assert_eq!(health["result"]["status"], "issues");
    let finding = health["result"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["kind"] == "work_item_source_document_archived")
        .unwrap_or_else(|| panic!("archived work source finding missing: {health}"));
    assert_eq!(finding["severity"], "warning");
    assert_eq!(finding["subject"]["id"], "work-refresh-operating-manual");
    assert_eq!(finding["related"]["id"], "document-operating-manual");
    assert_eq!(finding["related"]["title"], "Company operating manual");
    assert_eq!(finding["related"]["lifecycle_status"], "archived");

    // Archive the maintaining agent: Organization provenance keeps the member
    // and the maintained-document history navigable.
    maintainer["actor"]["status"] = json!("archived");
    maintainer["actor"]["updated_at"] = json!("2026-07-21T10:00:00+08:00");
    post_ok(&serve, "/v1/company-os/actors", admin(maintainer));

    let (status, organization) = serve.get_json("/v1/company-os/organization-provenance");
    assert_eq!(status, 200, "{organization}");
    let member = organization["result"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["actor_id"] == "agent-docs")
        .unwrap_or_else(|| panic!("archived member must stay listed: {organization}"));
    assert_eq!(member["resolution"], "archived");
    assert_eq!(member["member_status"], "archived");
    assert_eq!(member["maintained_documents"][0]["resolution"], "archived");
    assert_eq!(
        member["maintained_documents"][0]["title"],
        "Company operating manual"
    );
    assert_eq!(organization["result"]["summary"]["members_archived"], 1);
    assert_eq!(
        organization["result"]["summary"]["maintained_documents_archived"],
        1
    );

    let (status, provenance) = serve.get_json("/v1/company-os/work-provenance");
    assert_eq!(status, 200, "{provenance}");
    assert_eq!(
        provenance["result"]["work_items"][0]["assignees"][0]["resolution"],
        "archived"
    );
    assert_eq!(
        provenance["result"]["summary"]["archived_actor_references"],
        1
    );

    // A legacy row whose source Document no longer exists (only possible via
    // import, since appends validate references) resolves as missing and is
    // reported as a critical finding. The row is appended directly here only
    // to simulate that legacy state; production ledgers stay append-only.
    let store_root = home.projects_dir().join(&project_id);
    let work_ledger = store_root.join("company_os_work_items.jsonl");
    assert!(
        work_ledger.is_file(),
        "work ledger should exist at {work_ledger:?}"
    );
    let mut legacy_item = work_item.clone();
    legacy_item["id"] = json!("work-legacy-missing-source");
    legacy_item["title"] = json!("Legacy work with missing source");
    legacy_item["source_document_ref"] = json!("document-pruned-away");
    use std::io::Write as _;
    let mut ledger = std::fs::OpenOptions::new()
        .append(true)
        .open(&work_ledger)
        .unwrap();
    writeln!(ledger, "{}", serde_json::to_string(&legacy_item).unwrap()).unwrap();
    drop(ledger);

    let (status, provenance) = serve.get_json("/v1/company-os/work-provenance");
    assert_eq!(status, 200, "{provenance}");
    let legacy = provenance["result"]["work_items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["work_item_id"] == "work-legacy-missing-source")
        .unwrap_or_else(|| panic!("legacy work item must project: {provenance}"));
    assert_eq!(legacy["source"]["resolution"], "missing");
    assert_eq!(legacy["source"]["document_id"], "document-pruned-away");
    assert_eq!(provenance["result"]["summary"]["active_source_missing"], 1);

    let (status, health) = serve.get_json("/v1/company-os/docs-health");
    assert_eq!(status, 200, "{health}");
    let finding = health["result"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["kind"] == "work_item_source_document_missing")
        .unwrap_or_else(|| panic!("missing work source finding missing: {health}"));
    assert_eq!(finding["severity"], "critical");
    assert_eq!(finding["subject"]["id"], "work-legacy-missing-source");
    assert_eq!(health["result"]["counts"]["critical"], 1);
}

// --- work_item.update responsibility-ownership enforcement -----------------

const ROUTING_DEFINITION: &str = "page-work-routing";
const ROUTING_UPDATE_POLICY: &str = "page-work-routing:work_item.update";
const ROUTING_TRANSITION_POLICY: &str = "page-work-routing:work_item.transition";

/// A `work_item.update` dispatch against the routing fixture.
fn update_action(id: &str, requested_by: Value, record: Value) -> Value {
    json!({
        "id": id,
        "command_name": "work_item.update",
        "subject_ref": {"kind": "work_item", "id": "work-routing-target"},
        "requested_by": requested_by,
        "payload": {"definition_id": ROUTING_DEFINITION, "record": record},
        "required_permission": "company.records.write",
        "policy_ref": ROUTING_UPDATE_POLICY,
        "risk_tier": "r2",
        "requires_human_approval": false,
        "approval_refs": [],
        "status": "requested",
        "audit_event_refs": [format!("{id}:policy-authorized")],
        "requested_at": NOW,
        "completed_at": null
    })
}

/// A `work_item.transition` dispatch against the routing fixture, used to
/// prove what a self-granted seat would actually buy the requesting Actor.
fn transition_action(id: &str, requested_by: Value, record: Value) -> Value {
    json!({
        "id": id,
        "command_name": "work_item.transition",
        "subject_ref": {"kind": "work_item", "id": "work-routing-target"},
        "requested_by": requested_by,
        "payload": {"definition_id": ROUTING_DEFINITION, "record": record},
        "required_permission": "company.work.execute",
        "policy_ref": ROUTING_TRANSITION_POLICY,
        "risk_tier": "r2",
        "requires_human_approval": false,
        "approval_refs": [],
        "status": "requested",
        "audit_event_refs": [format!("{id}:policy-authorized")],
        "requested_at": NOW,
        "completed_at": null
    })
}

fn dispatch_update(serve: &ServeHandle, body: &Value) -> (u16, Value) {
    post_json(serve, "/v1/company-os/actions/dispatch", body)
}

/// Fail closed if a governed test could ever resolve the live Company Store.
///
/// The dogfood incident behind these tests came from assuming a store was
/// sandboxed when it was not, so isolation is asserted rather than trusted.
fn assert_isolated_company_store(home: &TempHome) {
    // Canonicalized: on macOS the temp base resolves through /private, so a
    // raw prefix comparison would report a false escape.
    let canonical = |path: &std::path::Path| {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_path_buf()
    };
    let firm_home = home.firm_home();
    let base = canonical(home.base());
    assert!(
        firm_home.starts_with(home.base()) || firm_home.starts_with(&base),
        "effective FIRM_HOME {firm_home:?} escapes the temp base {base:?}"
    );
    if let Some(real_home) = std::env::var_os("HOME") {
        let live = canonical(&std::path::Path::new(&real_home).join(".firm"));
        assert!(
            !canonical(firm_home).starts_with(&live),
            "effective FIRM_HOME {firm_home:?} resolves inside the live store {live:?}"
        );
    }
    assert!(
        !firm_home
            .join("companies")
            .join("agent-company")
            .exists(),
        "test store {firm_home:?} already carries the live agent-company ledger"
    );
}

/// One isolated Company with a routing module and a WorkItem whose executor
/// and closer seats are held by different Actors, so the executor/closer split
/// can be exercised. Returns the WorkItem record as posted.
fn work_routing_fixture(tag: &str) -> (TempHome, ServeHandle, Value) {
    let home = TempHome::new(tag);
    assert_isolated_company_store(&home);
    init_project(&home);
    let serve = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("FIRM_COMPANY_OS_TOKEN", TEST_TOKEN)],
    );

    post_ok(
        &serve,
        "/v1/company-os/actors",
        human("human-brand-owner", "Brand Owner"),
    );
    for (id, name) in [
        ("agent-work-owner", "Owner Agent"),
        ("agent-work-assignee", "Assignee Agent"),
        ("agent-work-reviewer", "Reviewer Agent"),
        ("agent-outsider", "Unrelated Agent"),
        ("agent-bystander", "Bystander Agent"),
    ] {
        post_ok(
            &serve,
            "/v1/company-os/actors",
            admin(agent_record(id, name, "Hold governed company work")),
        );
    }
    // The router is the only Actor holding explicit, policy-named update
    // authority. It still holds nothing more than the same
    // `company.records.write` every other agent has.
    let mut router = agent_record("agent-work-router", "Routing Agent", "Route company work");
    router["actor"]["permission_policy_refs"] = json!([
        "company.records.write",
        "company.work.execute",
        ROUTING_UPDATE_POLICY
    ]);
    post_ok(&serve, "/v1/company-os/actors", admin(router));

    let document = json!({
        "id": "document-work-routing",
        "space_id": "operations",
        "parent_document_id": null,
        "title": "Work routing",
        "kind": "page",
        "lifecycle_status": "active",
        "block_ids": [],
        "template_ref": null,
        "permission_policy_refs": ["company.records.write"],
        "reference_refs": [],
        "created_by": actor("human", "human-brand-owner"),
        "updated_by": actor("human", "human-brand-owner"),
        "created_at": NOW,
        "updated_at": NOW
    });
    post_ok(&serve, "/v1/company-os/documents", admin(document));
    post_ok(
        &serve,
        "/v1/company-os/business-modules",
        admin(json!({
            "id": "module-work-routing",
            "name": "Work routing",
            "purpose": "Govern how company work is routed",
            "root_document_ref": "document-work-routing",
            "record_types": [],
            "relation_rules": [],
            "default_view_refs": [],
            "policy_refs": [],
            "lifecycle_rules": [],
            "metric_definition_refs": [],
            "custom_page_definition_refs": [],
            "status": "draft",
            "owner": actor("human", "human-brand-owner"),
            "created_at": NOW,
            "updated_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/views",
        admin(json!({
            "id": "view-work-routing-standard",
            "module_id": "module-work-routing",
            "title": "Routed work",
            "mode": "table",
            "source_kinds": ["work_item"],
            "query": {"module_id": "module-work-routing"},
            "owner": actor("human", "human-brand-owner"),
            "policy_refs": [],
            "created_at": NOW,
            "updated_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/custom-page-packages",
        admin(json!({
            "id": "package-work-routing",
            "definition_id": ROUTING_DEFINITION,
            "version": "1.0.0",
            "kind": "react",
            "artifact_ref": "artifact://work-routing-page",
            "entrypoint": "WorkRoutingPage",
            "integrity_digest": "sha256:test",
            "built_at": NOW
        })),
    );
    post_ok(
        &serve,
        "/v1/company-os/custom-page-definitions",
        admin(json!({
            "id": ROUTING_DEFINITION,
            "module_id": "module-work-routing",
            "purpose": "Route governed company work",
            "allowed_data_queries": [{
                "id": "query-routed-work",
                "source_kind": "work_item",
                "source_scope": "module-work-routing",
                "permission_policy_ref": "company.records.write"
            }],
            "approved_ui_components": ["WorkItemCard"],
            "action_command_refs": ["work_item.update", "work_item.transition"],
            "standard_view_fallback_ref": "view-work-routing-standard",
            "owner": actor("human", "human-brand-owner"),
            "package_ref": "package-work-routing",
            "package_version": "1.0.0",
            "fixture_ref": "company-os-work-routing-v1",
            "visual_contract_ref": "visual-contract-v1",
            "policy_refs": [ROUTING_UPDATE_POLICY, ROUTING_TRANSITION_POLICY],
            "created_at": NOW,
            "updated_at": NOW
        })),
    );

    let work_item = json!({
        "id": "work-routing-target",
        "title": "Governed work with a named owner",
        "objective": "Prove responsibility rewrites are authorized",
        "description": null,
        "acceptance_criteria": [],
        "context_refs": [],
        "deliverable_refs": [],
        "status": "in_progress",
        "source_document_ref": "document-work-routing",
        "source_record_refs": [],
        "milestone_ref": null,
        "work_type": "development",
        "business_module_ref": "module-work-routing",
        "result_document_ref": null,
        "result_record_refs": [],
        "submitted_by": actor("human", "human-brand-owner"),
        "requested_by": actor("human", "human-brand-owner"),
        "accountable_owner": actor("agent", "agent-work-owner"),
        "assignees": [actor("agent", "agent-work-assignee")],
        "contributors": [],
        "reviewer": actor("agent", "agent-work-reviewer"),
        "approver": null,
        "execution_mode": "direct",
        "execution_refs": [],
        "approval_refs": [],
        "evidence_refs": [],
        "artifact_refs": [],
        "outcome_summary": null,
        "due_at": null,
        "priority": "high",
        "risk_level": "high",
        "created_at": NOW,
        "updated_at": NOW,
        "completed_at": null
    });
    post_ok(
        &serve,
        "/v1/company-os/work-items",
        admin(work_item.clone()),
    );
    (home, serve, work_item)
}

/// Every responsibility rewrite through `work_item.update` needs explicit
/// authority, and no path through it may hand the requesting Actor the
/// ownership that `work_item.transition` checks.
#[test]
fn work_item_update_requires_explicit_responsibility_authority() {
    let (_home, serve, work_item) = work_routing_fixture("company-os-work-update-ownership");

    // An unrelated Actor may still edit non-responsibility business context.
    // The gate must close authority, not ordinary authorship.
    let mut retitled = work_item.clone();
    retitled["title"] = json!("Retitled by an unrelated records author");
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-outsider-retitle",
            actor("agent", "agent-outsider"),
            retitled,
        ),
    );
    assert_eq!(status, 200, "{body}");

    // An unrelated Actor cannot rewrite responsibility, even holding the
    // blanket company.records.write that authorized the retitle above.
    let mut reassigned = work_item.clone();
    reassigned["title"] = json!("Retitled by an unrelated records author");
    reassigned["accountable_owner"] = actor("agent", "agent-bystander");
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-outsider-reassign",
            actor("agent", "agent-outsider"),
            reassigned,
        ),
    );
    assert_eq!(status, 403, "{body}");
    let detail = body["detail"].as_str().unwrap_or_default().to_string();
    assert!(
        detail.contains("agent-outsider")
            && detail.contains("accountable_owner")
            && detail.contains("work-routing-target")
            && detail.contains(ROUTING_UPDATE_POLICY),
        "denial must name actor, field, WorkItem, and missing authority: {detail}"
    );

    // Authority laundering: writing itself into executor standing is refused.
    let mut self_assigned = work_item.clone();
    self_assigned["title"] = json!("Retitled by an unrelated records author");
    self_assigned["assignees"] = json!([
        actor("agent", "agent-work-assignee"),
        actor("agent", "agent-outsider")
    ]);
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-outsider-self-grant",
            actor("agent", "agent-outsider"),
            self_assigned,
        ),
    );
    assert_eq!(status, 403, "{body}");
    let detail = body["detail"].as_str().unwrap_or_default().to_string();
    assert!(
        detail.contains("grant itself") && detail.contains("assignees"),
        "self-grant denial must name the laundering attempt: {detail}"
    );

    // The same refusal holds for the closer role.
    let mut self_reviewer = work_item.clone();
    self_reviewer["title"] = json!("Retitled by an unrelated records author");
    self_reviewer["reviewer"] = actor("agent", "agent-outsider");
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-outsider-self-reviewer",
            actor("agent", "agent-outsider"),
            self_reviewer,
        ),
    );
    assert_eq!(status, 403, "{body}");
    assert!(body["detail"]
        .as_str()
        .is_some_and(|detail| detail.contains("grant itself") && detail.contains("reviewer")));

    // Policy-named authority routes work to others but still cannot be used
    // to take the work. Laundering is refused independently of authority.
    let mut router_self_grant = work_item.clone();
    router_self_grant["title"] = json!("Retitled by an unrelated records author");
    router_self_grant["accountable_owner"] = actor("agent", "agent-work-router");
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-router-self-grant",
            actor("agent", "agent-work-router"),
            router_self_grant,
        ),
    );
    assert_eq!(status, 403, "{body}");
    assert!(body["detail"].as_str().is_some_and(
        |detail| detail.contains("grant itself") && detail.contains("accountable_owner")
    ));

    // Every denied attempt above left durable, reconstructable evidence.
    let (status, commands) = serve.get_json("/v1/company-os/action-commands");
    assert_eq!(status, 200, "{commands}");
    for denied_id in [
        "action-outsider-reassign",
        "action-outsider-self-grant",
        "action-outsider-self-reviewer",
        "action-router-self-grant",
    ] {
        let command = commands["result"]["items"]
            .as_array()
            .expect("action command items")
            .iter()
            .find(|row| row["id"] == denied_id)
            .unwrap_or_else(|| panic!("denied ActionCommand {denied_id} was not recorded"));
        assert_eq!(command["status"], "rejected", "{command}");
    }
    let (status, audits) = serve.get_json("/v1/company-os/audit-events");
    assert_eq!(status, 200, "{audits}");
    let laundering = audits["result"]["items"]
        .as_array()
        .expect("audit items")
        .iter()
        .find(|row| row["id"] == "action-outsider-self-grant:rejected")
        .expect("denial AuditEvent was not recorded");
    assert_eq!(laundering["event_kind"], "failed");
    assert_eq!(laundering["actor_ref"]["actor_id"], "agent-outsider");
    assert_eq!(laundering["subject_ref"]["id"], "work-routing-target");
    assert_eq!(laundering["detail"]["denial_kind"], "authority_laundering");
    assert_eq!(laundering["detail"]["rejected_fields"][0], "assignees");
    assert_eq!(
        laundering["detail"]["previous_responsibility"]["assignees"][0]["actor_id"],
        "agent-work-assignee"
    );
    assert_eq!(
        laundering["detail"]["requested_responsibility"]["assignees"][1]["actor_id"],
        "agent-outsider"
    );
    let missing_authority = audits["result"]["items"]
        .as_array()
        .expect("audit items")
        .iter()
        .find(|row| row["id"] == "action-outsider-reassign:rejected")
        .expect("denial AuditEvent was not recorded");
    assert_eq!(
        missing_authority["detail"]["denial_kind"],
        "missing_update_authority"
    );

    // Replaying a denied command repeats the refusal instead of degrading
    // into a generic conflict.
    let mut replay = work_item.clone();
    replay["title"] = json!("Retitled by an unrelated records author");
    replay["accountable_owner"] = actor("agent", "agent-bystander");
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-outsider-reassign",
            actor("agent", "agent-outsider"),
            replay,
        ),
    );
    assert_eq!(status, 403, "{body}");
    assert!(body["detail"]
        .as_str()
        .is_some_and(|detail| detail.contains("already denied")));

    // The accountable owner may re-route its own WorkItem.
    let mut owner_routed = work_item.clone();
    owner_routed["title"] = json!("Retitled by an unrelated records author");
    owner_routed["contributors"] = json!([actor("agent", "agent-bystander")]);
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-owner-adds-contributor",
            actor("agent", "agent-work-owner"),
            owner_routed.clone(),
        ),
    );
    assert_eq!(status, 200, "{body}");

    // So may the assignee and the reviewer.
    let mut assignee_routed = owner_routed.clone();
    assignee_routed["approver"] = actor("human", "human-brand-owner");
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-assignee-sets-approver",
            actor("agent", "agent-work-assignee"),
            assignee_routed.clone(),
        ),
    );
    assert_eq!(status, 200, "{body}");

    let mut reviewer_routed = assignee_routed.clone();
    reviewer_routed["contributors"] = json!([]);
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-reviewer-clears-contributor",
            actor("agent", "agent-work-reviewer"),
            reviewer_routed.clone(),
        ),
    );
    assert_eq!(status, 200, "{body}");

    // Governed routing by explicit policy-named authority stays open, as long
    // as the router hands the work to somebody other than itself.
    let mut router_routed = reviewer_routed.clone();
    router_routed["accountable_owner"] = actor("agent", "agent-bystander");
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-router-reassigns",
            actor("agent", "agent-work-router"),
            router_routed.clone(),
        ),
    );
    assert_eq!(status, 200, "{body}");

    let (status, detail) = serve.get_json("/v1/company-os/work-items/work-routing-target");
    assert_eq!(status, 200, "{detail}");
    assert_eq!(
        detail["result"]["accountable_owner"]["actor_id"],
        "agent-bystander"
    );
    // The displaced owner is no longer a controller and cannot take it back.
    let mut former_owner_reclaims = router_routed;
    former_owner_reclaims["accountable_owner"] = actor("agent", "agent-work-owner");
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-former-owner-reclaims",
            actor("agent", "agent-work-owner"),
            former_owner_reclaims,
        ),
    );
    assert_eq!(status, 403, "{body}");
    assert!(body["detail"]
        .as_str()
        .is_some_and(|detail| detail.contains("grant itself")));

    // Two-step escalation is closed too. The router may write itself in as a
    // contributor, because a contributor can neither execute nor close...
    let mut router_contributes = router_routed_after_reassign(&work_item);
    router_contributes["contributors"] = json!([actor("agent", "agent-work-router")]);
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-router-self-contributor",
            actor("agent", "agent-work-router"),
            router_contributes.clone(),
        ),
    );
    assert_eq!(status, 200, "{body}");

    // ...but that contributor seat is a dead end: it confers no control, so
    // the second step into executor standing is still refused.
    let mut router_escalates = router_contributes;
    router_escalates["assignees"] = json!([
        actor("agent", "agent-work-assignee"),
        actor("agent", "agent-work-router")
    ]);
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-router-escalates",
            actor("agent", "agent-work-router"),
            router_escalates,
        ),
    );
    assert_eq!(status, 403, "{body}");
    assert!(
        body["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("grant itself") && detail.contains("assignees")),
        "contributor standing must not escalate into executor standing: {body}"
    );
}

/// The fixture WorkItem as it stands after the governed reassignment above.
fn router_routed_after_reassign(work_item: &Value) -> Value {
    let mut record = work_item.clone();
    record["title"] = json!("Retitled by an unrelated records author");
    record["accountable_owner"] = actor("agent", "agent-bystander");
    record["approver"] = actor("human", "human-brand-owner");
    record["contributors"] = json!([]);
    record
}

/// Regression for the executor/closer split. An Actor that already holds one
/// transition role may not use `work_item.update` to take the other, so no
/// single Standing Agent can both execute and sign off the same work.
#[test]
fn work_item_update_cannot_cross_the_executor_closer_split() {
    let (_home, serve, work_item) = work_routing_fixture("company-os-work-update-role-split");
    let assignee = actor("agent", "agent-work-assignee");

    // The assignee is a legitimate executor: it may move the work to in_review.
    let mut in_review = work_item.clone();
    in_review["status"] = json!("in_review");
    in_review["result_document_ref"] = json!("document-work-routing");
    in_review["artifact_refs"] = json!(["artifact://work-routing/result-v1"]);
    in_review["outcome_summary"] = json!("Routed work is ready for accountable review.");
    let (status, body) = dispatch_update(
        &serve,
        &transition_action("action-split-submit", assignee.clone(), in_review.clone()),
    );
    assert_eq!(
        status, 200,
        "the assignee must still be able to execute: {body}"
    );

    // It is not a closer, so it cannot sign its own work off.
    let mut completed = in_review.clone();
    completed["status"] = json!("completed");
    completed["completed_at"] = json!(NOW);
    let (status, body) = dispatch_update(
        &serve,
        &transition_action(
            "action-split-close-before",
            assignee.clone(),
            completed.clone(),
        ),
    );
    assert_eq!(status, 403, "{body}");
    assert!(body["detail"]
        .as_str()
        .is_some_and(|detail| detail.contains("does not own")));

    // The escalation this test exists for: buying the closer seat through
    // work_item.update. It must be refused even though the assignee already
    // controls the WorkItem as an executor.
    let mut self_reviewer = in_review.clone();
    self_reviewer["reviewer"] = assignee.clone();
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-split-self-reviewer",
            assignee.clone(),
            self_reviewer,
        ),
    );
    assert_eq!(status, 403, "{body}");
    let detail = body["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("closer") && detail.contains("reviewer"),
        "denial must name the role class and the seat: {body}"
    );

    // The closer seat was never granted, so the close still fails.
    let (status, body) = dispatch_update(
        &serve,
        &transition_action("action-split-close-after", assignee, completed),
    );
    assert_eq!(status, 403, "{body}");

    // The mirror case: the reviewer is a closer and may not take the
    // executor seat.
    let mut self_assignee = in_review;
    self_assignee["assignees"] = json!([actor("agent", "agent-work-reviewer")]);
    let (status, body) = dispatch_update(
        &serve,
        &update_action(
            "action-split-self-assignee",
            actor("agent", "agent-work-reviewer"),
            self_assignee,
        ),
    );
    assert_eq!(status, 403, "{body}");
    assert!(
        body["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("executor")),
        "{body}"
    );
}

/// Denial evidence must be unforgeable. An Actor may not pre-bind another
/// command's terminal audit id and thereby suppress that command's denial
/// record.
#[test]
fn work_item_update_denial_survives_a_pre_bound_audit_id() {
    let (_home, serve, work_item) = work_routing_fixture("company-os-work-update-denial-evidence");
    let outsider = actor("agent", "agent-outsider");

    // Squatting on another command's :rejected id is refused up front, so the
    // cover command never lands and never reserves the victim id.
    let mut retitled = work_item.clone();
    retitled["title"] = json!("Cover traffic");
    let mut cover = update_action("action-cover", outsider.clone(), retitled);
    cover["audit_event_refs"] = json!(["action-evil:rejected"]);
    let (status, body) = dispatch_update(&serve, &cover);
    assert_eq!(status, 403, "{body}");
    assert!(
        body["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("namespace belongs to")),
        "{body}"
    );

    // The denial therefore still records its own evidence.
    let mut stolen = work_item;
    stolen["accountable_owner"] = outsider.clone();
    let (status, body) = dispatch_update(&serve, &update_action("action-evil", outsider, stolen));
    assert_eq!(status, 403, "{body}");
    assert!(body["detail"]
        .as_str()
        .is_some_and(|detail| detail.contains("grant itself")));

    let (status, audits) = serve.get_json("/v1/company-os/audit-events");
    assert_eq!(status, 200, "{audits}");
    let denial = audits["result"]["items"]
        .as_array()
        .expect("audit items")
        .iter()
        .find(|row| row["id"] == "action-evil:rejected")
        .expect("denial AuditEvent was suppressed");
    assert_eq!(denial["event_kind"], "failed", "{denial}");
    assert_eq!(denial["action_command_id"], "action-evil", "{denial}");
    assert_eq!(denial["detail"]["denial_kind"], "authority_laundering");
}

/// Source accountability is not editable business context: `work_item.update`
/// must not become the forgery path for who raised the work.
#[test]
fn work_item_update_cannot_forge_request_provenance() {
    let (_home, serve, work_item) = work_routing_fixture("company-os-work-update-provenance");
    let outsider = actor("agent", "agent-outsider");

    for field in ["requested_by", "submitted_by"] {
        let mut forged = work_item.clone();
        forged[field] = outsider.clone();
        let (status, body) = dispatch_update(
            &serve,
            &update_action(&format!("action-forge-{field}"), outsider.clone(), forged),
        );
        assert_eq!(status, 409, "{field} must be protected provenance: {body}");
        assert!(
            body["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("request provenance")),
            "{body}"
        );
    }

    // The stored record kept its original provenance.
    let (status, items) = serve.get_json("/v1/company-os/work-items");
    assert_eq!(status, 200, "{items}");
    let stored = items["result"]["items"]
        .as_array()
        .expect("work items")
        .iter()
        .find(|row| row["id"] == "work-routing-target")
        .expect("fixture WorkItem");
    assert_eq!(stored["requested_by"]["actor_id"], "human-brand-owner");
    assert_eq!(stored["submitted_by"]["actor_id"], "human-brand-owner");
}
