//! Live HTTP coverage for the workspace-bounded local evidence viewer.

mod firm_env;
use firm_env::{current_project_id, run_firm, ServeHandle, TempHome};

#[test]
fn source_viewer_serves_project_file_and_refuses_traversal() {
    let home = TempHome::new("source-viewer");
    let project_root = home.base().join("project");
    std::fs::create_dir_all(&project_root).expect("create project");
    std::fs::write(project_root.join("evidence.md"), "first\nselected\nthird\n")
        .expect("write evidence");
    let init = run_firm(&home, &project_root, &["init"]);
    assert!(init.status.success(), "init failed: {init:?}");
    let project_id = current_project_id(&home);
    let serve = ServeHandle::spawn(&home, &project_root, &[]);

    let (status, allowed) = serve.get_json(&format!(
        "/v1/projects/{project_id}/source?path=evidence.md&line=2"
    ));
    assert_eq!(status, 200, "allowed body: {allowed}");
    assert_eq!(allowed["kind"], "markdown");
    assert_eq!(allowed["line"], 2);
    assert_eq!(allowed["content"], "first\nselected\nthird\n");

    let (status, denied) = serve.get_json(&format!(
        "/v1/projects/{project_id}/source?path=../outside.txt"
    ));
    assert_eq!(status, 200, "denied body: {denied}");
    assert_eq!(denied["kind"], "outside_workspace");
    assert!(denied.get("content").is_none());
}
