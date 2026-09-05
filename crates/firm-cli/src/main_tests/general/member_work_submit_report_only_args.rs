use super::*;

/// DEV-214 (#830): `member work submit` takes exactly one revision shape —
/// `--candidate-revision <sha>` or `--report-only`; both or neither is a
/// usage error naming both flags.
#[test]
fn submit_revision_args_accepts_exactly_one_revision_shape() {
    let args = |tokens: &[&str]| {
        tokens
            .iter()
            .map(|token| token.to_string())
            .collect::<Vec<_>>()
    };

    let (candidate, report_only) = submit_revision_args(&args(&[
        "--candidate-revision",
        "0123456789abcdef0123456789abcdef01234567",
    ]))
    .expect("a named candidate is a valid submission shape");
    assert_eq!(
        candidate.as_deref(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );
    assert!(!report_only);

    let (candidate, report_only) =
        submit_revision_args(&args(&["--report-only"])).expect("report-only is a valid shape");
    assert_eq!(candidate, None);
    assert!(report_only);

    let both = submit_revision_args(&args(&[
        "--candidate-revision",
        "0123456789abcdef0123456789abcdef01234567",
        "--report-only",
    ]))
    .expect_err("both flags must be a usage error");
    let detail = both.to_string();
    assert!(
        detail.contains("--candidate-revision") && detail.contains("--report-only"),
        "the both-flags error must name both flags: {detail}"
    );

    let neither = submit_revision_args(&args(&["--result-summary", "done"]))
        .expect_err("neither flag must be a usage error");
    let detail = neither.to_string();
    assert!(
        detail.contains("--candidate-revision") && detail.contains("--report-only"),
        "the neither-flag error must name both flags: {detail}"
    );
}
