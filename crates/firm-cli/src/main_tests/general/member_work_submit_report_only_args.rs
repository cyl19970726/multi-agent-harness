use super::*;

/// DEV-214 (#830): `--candidate-revision <sha>` and `--report-only` are
/// mutually exclusive, so naming both is a usage error naming both flags.
/// Naming neither parses cleanly — the submission service decides whether it
/// can derive the candidate from a structured GitHub link (#369) or must
/// refuse the submission.
#[test]
fn submit_revision_args_refuses_only_both_revision_shapes_together() {
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

    let (candidate, report_only) = submit_revision_args(&args(&["--result-summary", "done"]))
        .expect("naming neither flag parses; the submission service decides");
    assert_eq!(candidate, None);
    assert!(!report_only);
}
