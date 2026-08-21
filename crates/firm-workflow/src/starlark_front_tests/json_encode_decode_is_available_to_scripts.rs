use super::*;

#[test]
fn json_encode_decode_is_available_to_scripts() {
    // The `Json` library extension exposes json.encode/json.decode so a
    // program can serialize a structured value and inject it verbatim into a
    // downstream prompt (the forward-injection mechanism).
    let seen = Mutex::new(Vec::new());
    let script = r#"
data = {"verdict": "pass", "score": 100}
encoded = json.encode(data)
roundtrip = json.decode(encoded)
agent("use this: " + encoded + " score=" + str(roundtrip["score"]))
"#;
    let outcome = {
        let driver = recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 1);
    let prompt = &seen[0].1;
    assert!(
        prompt.contains("\"verdict\":\"pass\""),
        "encoded JSON injected into the prompt: {prompt}"
    );
    assert!(
        prompt.contains("score=100"),
        "decoded value usable in the script: {prompt}"
    );
    assert_eq!(outcome.steps.len(), 1);
}
