use super::*;

#[test]
fn parallel_writable_spec_field_flows() {
    let seen = Mutex::new(Vec::new());
    let script =
        "\nparallel([{\"prompt\": \"a\", \"label\": \"x\"}, {\"prompt\": \"b\", \"label\": \"y\", \"writable\": True}])\n";
    {
        let driver = writable_recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");
    }
    let mut seen = seen.into_inner().unwrap();
    seen.sort();
    assert_eq!(
        seen,
        vec![("x".to_string(), false), ("y".to_string(), true)]
    );
}
