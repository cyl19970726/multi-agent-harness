//! Retirement contract for the legacy Company OS HTTP writers (DOC-108
//! Stage B): every `POST /v1/company-os/*` mutation fails with an explicit
//! `retired_write_authority` 410, while the read-shaped `work-query`
//! projection remains a legacy read. The Global Work RoleView and
//! `harness work` are the successor aggregate; historical Company data is
//! export/verify-only through `harness legacy-company-os export|verify`.

mod firm_env;
use firm_env::{run_firm, ServeHandle, TempHome};
use serde_json::{json, Value};

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

#[test]
fn company_os_writers_are_retired_with_explicit_410() {
    let (_home, serve) = serve("company-os-retired-writers");

    for (path, body) in [
        ("/v1/company-os/actors", json!({"actor_type": "human", "actor": {"id": "human-root"}})),
        (
            "/v1/company-os/milestones",
            json!({"mode": "administrative", "record": {"id": "milestone-1"}}),
        ),
        (
            "/v1/company-os/actions/dispatch",
            json!({"action": {"id": "action-1"}}),
        ),
        (
            "/v1/company-os/typed-records",
            json!({"mode": "administrative", "record": {"id": "record-1"}}),
        ),
    ] {
        let (status, body) = post(&serve, path, &body);
        assert_eq!(status, 410, "POST {path}: {body}");
        assert_eq!(
            body["error"].as_str(),
            Some("retired_write_authority"),
            "POST {path}: {body}"
        );
        assert!(
            body["detail"]
                .as_str()
                .unwrap_or_default()
                .contains("DOC-108"),
            "POST {path} detail must name the retirement: {body}"
        );
    }
}

#[test]
fn company_os_reads_are_retired_with_the_module() {
    let (_home, serve) = serve("company-os-retired-reads");

    // The read-shaped work-query and the read projections retired with the
    // module (DOC-108 Stage B step 4): the read-only successor aggregate is
    // the Global Work RoleView (`/v1/views/global-work`, `harness work list`).
    for (method, path) in [
        ("POST", "/v1/company-os/work-query"),
        ("GET", "/v1/company-os/work-projection"),
        ("GET", "/v1/company-os/works"),
        ("GET", "/v1/company-os/snapshot"),
    ] {
        let (status, body) = match method {
            "POST" => post(&serve, path, &json!({"phases": ["active"]})),
            _ => serve.get_json(path),
        };
        assert_eq!(status, 410, "{method} {path}: {body}");
        assert!(
            body["detail"]
                .as_str()
                .unwrap_or_default()
                .contains("DOC-108"),
            "{method} {path} detail must name the retirement: {body}"
        );
    }
}
