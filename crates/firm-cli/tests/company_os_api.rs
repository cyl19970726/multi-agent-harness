//! End-to-end HTTP acceptance for the Unified Company Work boundary.

mod firm_env;
use firm_env::{run_firm, ServeHandle, TempHome};
use serde_json::{json, Value};

const NOW: &str = "2026-08-09T10:00:00+08:00";
const TEST_TOKEN: &str = "company-os-api-test-capability";

fn init_project(home: &TempHome) {
    let root = home.base().join("company");
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init failed: {out:?}");
}

fn serve(tag: &str) -> (TempHome, ServeHandle) {
    let home = TempHome::new(tag);
    init_project(&home);
    let server = ServeHandle::spawn_with_env(
        &home,
        home.base(),
        &[],
        &[("FIRM_COMPANY_OS_TOKEN", TEST_TOKEN)],
    );
    (home, server)
}

fn post(serve: &ServeHandle, path: &str, body: &Value) -> (u16, Value) {
    serve.post_json_with_token(path, body, TEST_TOKEN)
}

fn human_root() -> Value {
    json!({
        "actor_type": "human",
        "actor": {
            "id": "human-root",
            "display_name": "Company Root",
            "title": "Operator",
            "status": "active",
            "availability": "available",
            "membership_refs": [],
            "responsibility_summary": "Company authority",
            "permission_policy_refs": ["company_os.admin"],
            "authority_policy_refs": ["company_os.admin"],
            "created_at": NOW,
            "updated_at": NOW
        }
    })
}

fn administrative(record: Value) -> Value {
    json!({
        "mode": "administrative",
        "authority": {"actor_type": "human", "actor_id": "human-root"},
        "record": record
    })
}

#[test]
fn company_work_is_a_read_only_team_work_projection() {
    let (_home, serve) = serve("company-work-projection");
    let (status, actor) = post(&serve, "/v1/company-os/actors", &human_root());
    assert_eq!(status, 200, "{actor}");

    let (status, projection) = serve.get_json("/v1/company-os/work-projection");
    assert_eq!(status, 200, "{projection}");
    assert_eq!(projection["result"]["authority"], "team_work");
    assert_eq!(projection["result"]["read_only"], true);
    assert_eq!(projection["result"]["works"], json!([]));

    let (status, query) = post(
        &serve,
        "/v1/company-os/work-query",
        &json!({
            "phases": ["active"],
            "conditions": ["normal"]
        }),
    );
    assert_eq!(status, 200, "{query}");
    assert_eq!(query["result"]["query"]["phases"], json!(["active"]));

    for resource in ["work-items", "assignments"] {
        let (status, body) = serve.get_json(&format!("/v1/company-os/{resource}"));
        assert_eq!(status, 404, "GET {resource}: {body}");
        let (status, body) = post(
            &serve,
            &format!("/v1/company-os/{resource}"),
            &administrative(json!({})),
        );
        assert_eq!(status, 404, "POST {resource}: {body}");
    }

    let (status, snapshot) = serve.get_json("/v1/company-os/snapshot");
    assert_eq!(status, 200, "{snapshot}");
    assert!(snapshot["result"].get("work_items").is_none());
    assert!(snapshot["result"].get("assignments").is_none());
    assert!(snapshot["result"].get("work_execution_chains").is_none());
    assert_eq!(snapshot["result"]["work"]["authority"], "team_work");
}

#[test]
fn milestone_references_authoritative_work_ids_without_copying_work() {
    let (_home, serve) = serve("company-work-milestone");

    let (status, actor) = post(&serve, "/v1/company-os/actors", &human_root());
    assert_eq!(status, 200, "{actor}");

    let milestone = json!({
        "id": "milestone-release",
        "title": "Release ready",
        "outcome": "The accepted TeamWork is released",
        "status": "active",
        "accountable_owner": {"actor_type": "human", "actor_id": "human-root"},
        "source_document_ref": null,
        "business_module_ref": null,
        "target_at": null,
        "acceptance_criteria": ["The authoritative Work is accepted"],
        "work_refs": ["work-native-1"],
        "created_at": NOW,
        "updated_at": NOW,
        "achieved_at": null
    });
    let (status, result) = post(
        &serve,
        "/v1/company-os/milestones",
        &administrative(milestone.clone()),
    );
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["result"], milestone);

    let (status, stored) = serve.get_json("/v1/company-os/milestones/milestone-release");
    assert_eq!(status, 200, "{stored}");
    assert_eq!(stored["result"]["work_refs"], json!(["work-native-1"]));
    assert!(stored["result"].get("work_item_refs").is_none());
}
