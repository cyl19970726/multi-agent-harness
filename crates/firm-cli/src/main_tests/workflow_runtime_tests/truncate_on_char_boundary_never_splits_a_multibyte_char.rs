use super::*;

#[test]
fn truncate_on_char_boundary_never_splits_a_multibyte_char() {
    // ASCII shorter than the cap is returned unchanged.
    assert_eq!(truncate_on_char_boundary("hello", 160), "hello");

    // issue #89 P0: a CJK string whose byte cap (240) lands INSIDE a 3-byte
    // char must back off to a char boundary instead of panicking on `&s[..240]`.
    let cjk = "保留中文输出不要崩溃".repeat(40); // 10 chars * 3 bytes * 40
    let out = truncate_on_char_boundary(&cjk, 240);
    assert!(out.len() <= 240, "respects the byte cap");
    assert!(cjk.starts_with(out), "is a valid prefix");
    assert!(
        cjk.is_char_boundary(out.len()),
        "ends on a char boundary (no split)"
    );

    // The summary path that crashed (main.rs:summarize_json_value) must no
    // longer panic on CJK that overflows the cap.
    let value = serde_json::Value::String("留".repeat(200));
    let summary = summarize_json_value(&value); // pre-fix: byte-slice panic
    assert!(
        summary.ends_with("..."),
        "long value is truncated with an ellipsis"
    );
}
