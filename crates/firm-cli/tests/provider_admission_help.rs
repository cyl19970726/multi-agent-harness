use std::process::Command;

#[test]
fn root_and_provider_help_publish_provider_admit_contract() {
    let root = Command::new(env!("CARGO_BIN_EXE_firm"))
        .arg("--help")
        .output()
        .expect("run root help");
    assert!(root.status.success());
    let root_stdout = String::from_utf8_lossy(&root.stdout);
    assert!(root_stdout.contains("provider admit --provider <name>"));
    assert!(root_stdout.contains("--adapter-contract-version <version>"));
    assert!(root_stdout.contains("Execution Space requires the global --project flag"));

    let provider = Command::new(env!("CARGO_BIN_EXE_firm"))
        .args(["provider", "--help"])
        .output()
        .expect("run provider help");
    assert!(provider.status.success());
    let provider_stdout = String::from_utf8_lossy(&provider.stdout);
    assert!(provider_stdout.contains("harness [--project <id|path>] provider admit"));
    assert!(provider_stdout.contains("--policy strict|advisory"));
    assert!(provider_stdout.contains("--evidence <ref>"));
    assert!(provider_stdout.contains("FIRM_PROJECT and ambient defaults are not explicit"));
}
