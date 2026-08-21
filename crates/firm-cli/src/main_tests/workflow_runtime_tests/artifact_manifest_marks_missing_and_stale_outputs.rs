use super::*;

#[test]
fn artifact_manifest_marks_missing_and_stale_outputs() {
    let store = temp_store("artifact-edges");
    let project_root = init_gc_git_project("artifact-edges", &store);
    std::fs::create_dir_all(project_root.join("out")).expect("mk out");
    std::fs::write(project_root.join("out/summary.md"), "artifact").expect("artifact");
    std::fs::write(project_root.join("missing.md"), "wrong root").expect("root fallback trap");

    let missing = append_artifact_manifest(
        &store,
        "wfrun-artifact-edges",
        None,
        Some("missing".into()),
        Some("out".into()),
        vec!["out".into()],
        vec!["missing.md".into()],
    )
    .expect("missing manifest");
    assert_eq!(missing.status, WorkflowArtifactManifestStatus::Missing);
    assert!(missing.files[0].path.ends_with("out/missing.md"));

    let prefixed = append_artifact_manifest(
        &store,
        "wfrun-artifact-edges",
        None,
        Some("prefixed".into()),
        Some("out".into()),
        vec!["out".into()],
        vec!["out/summary.md".into()],
    )
    .expect("prefixed manifest");
    assert_eq!(prefixed.status, WorkflowArtifactManifestStatus::Current);
    assert_eq!(prefixed.files[0].path, "out/summary.md");

    let stale = append_artifact_manifest(
        &store,
        "wfrun-artifact-edges",
        None,
        Some("stale".into()),
        Some("out".into()),
        vec!["reports".into()],
        vec!["summary.md".into()],
    )
    .expect("stale manifest");
    assert_eq!(stale.status, WorkflowArtifactManifestStatus::Stale);
    assert_eq!(stale.files[0].path, "out/summary.md");
    assert!(stale
        .reason
        .unwrap_or_default()
        .contains("outside write_roots"));

    let _ = std::fs::remove_dir_all(&project_root);
    let _ = std::fs::remove_dir_all(store.root());
}
