//! Read-only verifier integration. Isolated deterministic records are fixtures,
//! never evidence of a real coding dogfood or native tool execution.
#[test]
fn dogfood_v2_cli_resolves_trusted_space_and_canonical_host_metadata() {
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = std::process::Command::new("node")
        .current_dir(repository)
        .args([
            "scripts/check-agent-team-evidence-sources.mjs",
            env!("CARGO_BIN_EXE_firm"),
        ])
        .output()
        .expect("run v2 source integration");
    assert!(
        output.status.success(),
        "v2 source integration failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
