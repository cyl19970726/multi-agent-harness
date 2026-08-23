use super::*;

#[test]
fn claude_runner_resolves_from_installed_harness_tree() {
    let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("runner")));
    let project = root.join("unrelated-project");
    let install = root.join("star-harness/0.4.2");
    let executable = install.join("harness");
    let runner = install.join("apps/claude-member-runner/bin/claude-member-runner.mjs");
    std::fs::create_dir_all(&project).expect("create unrelated project");
    std::fs::create_dir_all(runner.parent().expect("runner parent"))
        .expect("create installed runner directory");
    std::fs::write(&runner, "#!/usr/bin/env node\n").expect("write installed runner");
    std::fs::write(&executable, b"binary").expect("write installed Harness path");

    assert_eq!(
        claude_agent_sdk_runner_path_from(&project, Some(&executable))
            .expect("resolve installed runner"),
        runner
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deepseek_runner_resolves_from_installed_harness_tree() {
    let root =
        std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("dsh-runner")));
    let project = root.join("unrelated-project");
    let install = root.join("star-harness/0.4.2");
    let executable = install.join("harness");
    let runner = install.join("apps/deepseek-member-runner/bin/deepseek-member-runner.mjs");
    std::fs::create_dir_all(&project).expect("create unrelated project");
    std::fs::create_dir_all(runner.parent().expect("runner parent"))
        .expect("create installed runner directory");
    std::fs::write(&runner, "#!/usr/bin/env node\n").expect("write installed runner");
    std::fs::write(&executable, b"binary").expect("write installed Harness path");

    assert_eq!(
        deepseek_harness_runner_path_from(&project, Some(&executable))
            .expect("resolve installed DSH runner"),
        runner
    );
    let _ = std::fs::remove_dir_all(root);
}
