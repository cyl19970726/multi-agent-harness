use super::*;

#[test]
fn agent_writable_kwarg_flows_onto_the_spec_default_false() {
    let seen = Mutex::new(Vec::new());
    let script =
        "\nagent(\"read it\", label = \"reader\")\nagent(\"fix it\", label = \"fixer\", writable = True)\n";
    {
        let driver = writable_recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");
    }
    let seen = seen.into_inner().unwrap();
    assert_eq!(
        seen,
        vec![("reader".to_string(), false), ("fixer".to_string(), true)]
    );
}
