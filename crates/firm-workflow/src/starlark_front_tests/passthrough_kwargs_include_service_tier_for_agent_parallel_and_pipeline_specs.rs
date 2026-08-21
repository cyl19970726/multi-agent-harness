use super::*;

#[test]
fn passthrough_kwargs_include_service_tier_for_agent_parallel_and_pipeline_specs() {
    let seen = Mutex::new(Vec::<(
        String,
        Vec<String>,
        Vec<String>,
        Vec<String>,
        Option<String>,
        Option<String>,
        Option<u64>,
    )>::new());
    let script = r#"
agent("inspect", label = "single", image = ["a.png"], add_dir = ["src"], expected_artifacts = ["out/single.png"], service_tier = "priority", fallback_model = "claude-sonnet", timeout_s = 11)
parallel([{"prompt": "compare", "label": "fanout", "image": ["b.png", "c.jpg"], "add_dir": ["crates"], "expected_artifacts": ["out/fanout.json"], "service_tier": "flex", "fallback_model": "claude-haiku", "timeout_s": 12}])
pipeline(
["item"],
[{"prompt": "stage {input}", "label": "pipe", "image": ["d.webp"], "add_dir": ["skills"], "expected_artifacts": ["out/pipe.txt"], "service_tier": "default", "fallback_model": "claude-opus", "timeout_s": 13}],
)
"#;
    let outcome = {
        let driver = |spec: &AgentStepSpec| {
            seen.lock().unwrap().push((
                spec.label.clone(),
                spec.image.clone(),
                spec.add_dir.clone(),
                spec.expected_artifacts.clone(),
                spec.service_tier.clone(),
                spec.fallback_model.clone(),
                spec.timeout_s,
            ));
            StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                output_summary: "ok".to_string(),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
                ordinal: None,
            }
        };
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };

    #[allow(clippy::type_complexity)]
    let seen: std::collections::HashMap<
        String,
        (
            Vec<String>,
            Vec<String>,
            Vec<String>,
            Option<String>,
            Option<String>,
            Option<u64>,
        ),
    > = seen
        .into_inner()
        .unwrap()
        .into_iter()
        .map(
            |(
                label,
                image,
                add_dir,
                expected_artifacts,
                service_tier,
                fallback_model,
                timeout_s,
            )| {
                (
                    label,
                    (
                        image,
                        add_dir,
                        expected_artifacts,
                        service_tier,
                        fallback_model,
                        timeout_s,
                    ),
                )
            },
        )
        .collect();
    assert_eq!(seen["single"].0, vec!["a.png".to_string()]);
    assert_eq!(seen["single"].1, vec!["src".to_string()]);
    assert_eq!(seen["single"].2, vec!["out/single.png".to_string()]);
    assert_eq!(seen["single"].3.as_deref(), Some("priority"));
    assert_eq!(seen["single"].4.as_deref(), Some("claude-sonnet"));
    assert_eq!(seen["single"].5, Some(11));
    assert_eq!(
        seen["fanout"].0,
        vec!["b.png".to_string(), "c.jpg".to_string()]
    );
    assert_eq!(seen["fanout"].1, vec!["crates".to_string()]);
    assert_eq!(seen["fanout"].2, vec!["out/fanout.json".to_string()]);
    assert_eq!(seen["fanout"].3.as_deref(), Some("flex"));
    assert_eq!(seen["fanout"].4.as_deref(), Some("claude-haiku"));
    assert_eq!(seen["fanout"].5, Some(12));
    assert_eq!(seen["pipe"].0, vec!["d.webp".to_string()]);
    assert_eq!(seen["pipe"].1, vec!["skills".to_string()]);
    assert_eq!(seen["pipe"].2, vec!["out/pipe.txt".to_string()]);
    assert_eq!(seen["pipe"].3.as_deref(), Some("default"));
    assert_eq!(seen["pipe"].4.as_deref(), Some("claude-opus"));
    assert_eq!(seen["pipe"].5, Some(13));
    assert_eq!(outcome.steps.len(), 3);
}
