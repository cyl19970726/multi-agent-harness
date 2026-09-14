use super::*;

fn link(kind: GitHubLinkKind, owner: &str, repo: &str, number: u64, url: &str) -> GitHubLink {
    GitHubLink {
        kind,
        owner: owner.into(),
        repo: repo.into(),
        number,
        url: url.into(),
        status: None,
        ci_status: None,
        ci_url: None,
    }
}

/// A submitted GitHub link stands in for the candidate revision, so its
/// structured fields must describe the url it points at. The check used to
/// compare the url byte-for-byte against a hard-coded `https://github.com`
/// origin, which refused two links that name exactly the right object: one
/// whose owner/repo differ only in case (GitHub is case-insensitive there)
/// and one served by a GitHub Enterprise host. It stays fail-closed for a
/// different object.
#[test]
fn github_link_url_describes_its_own_object() {
    // Accepted: the canonical github.com spelling.
    assert_eq!(
        link(
            GitHubLinkKind::PullRequest,
            "cyl19970726",
            "multi-agent-harness",
            951,
            "https://github.com/cyl19970726/multi-agent-harness/pull/951",
        )
        .require_structural_consistency(),
        Ok(())
    );

    // Accepted: owner/repo differing only in case name the same object.
    assert_eq!(
        link(
            GitHubLinkKind::PullRequest,
            "cyl19970726",
            "multi-agent-harness",
            951,
            "https://github.com/CYL19970726/Multi-Agent-Harness/pull/951",
        )
        .require_structural_consistency(),
        Ok(())
    );

    // Accepted: a GitHub Enterprise host serving the same object path.
    assert_eq!(
        link(
            GitHubLinkKind::Issue,
            "octocat",
            "hello-world",
            42,
            "https://github.example.com/octocat/hello-world/issues/42",
        )
        .require_structural_consistency(),
        Ok(())
    );

    // Accepted: a trailing slash is not part of the object identity.
    assert_eq!(
        link(
            GitHubLinkKind::Issue,
            "octocat",
            "hello-world",
            42,
            "https://github.com/octocat/hello-world/issues/42/",
        )
        .require_structural_consistency(),
        Ok(())
    );

    // Refused: a different number is a different object.
    assert!(link(
        GitHubLinkKind::PullRequest,
        "octocat",
        "hello-world",
        42,
        "https://github.com/octocat/hello-world/pull/43",
    )
    .require_structural_consistency()
    .is_err());

    // Refused: a different owner or repo is a different object.
    for url in [
        "https://github.com/someone-else/hello-world/pull/42",
        "https://github.com/octocat/other-repo/pull/42",
    ] {
        assert!(
            link(
                GitHubLinkKind::PullRequest,
                "octocat",
                "hello-world",
                42,
                url,
            )
            .require_structural_consistency()
            .is_err(),
            "{url} must not describe octocat/hello-world#42"
        );
    }

    // Refused: an issue url may not stand in for a pull request.
    assert!(link(
        GitHubLinkKind::PullRequest,
        "octocat",
        "hello-world",
        42,
        "https://github.com/octocat/hello-world/issues/42",
    )
    .require_structural_consistency()
    .is_err());

    // Refused: a look-alike url that merely contains the object path, carries
    // extra path segments, or is not an absolute http(s) url.
    for url in [
        "https://evil.example.com/redirect/octocat/hello-world/pull/42",
        "https://github.com/octocat/hello-world/pull/42/files",
        "https://github.com/octocat/hello-world/pull/42?utm=1",
        "https://github.com/octocat/hello-world/pull/42#diff",
        "github.com/octocat/hello-world/pull/42",
        "ftp://github.com/octocat/hello-world/pull/42",
        "https:///octocat/hello-world/pull/42",
    ] {
        assert!(
            link(
                GitHubLinkKind::PullRequest,
                "octocat",
                "hello-world",
                42,
                url,
            )
            .require_structural_consistency()
            .is_err(),
            "{url} must fail closed"
        );
    }

    // Refused: an empty owner/repo or a zero number never names an object.
    assert!(link(
        GitHubLinkKind::Issue,
        "",
        "hello-world",
        42,
        "https://github.com//hello-world/issues/42",
    )
    .require_structural_consistency()
    .is_err());
    assert!(link(
        GitHubLinkKind::Issue,
        "octocat",
        "hello-world",
        0,
        "https://github.com/octocat/hello-world/issues/0",
    )
    .require_structural_consistency()
    .is_err());

    // The minted canonical url is still the github.com spelling, and it
    // describes its own object.
    let minted = link(
        GitHubLinkKind::Issue,
        "octocat",
        "hello-world",
        42,
        "https://github.com/octocat/hello-world/issues/42",
    );
    assert_eq!(
        minted.canonical_url(),
        "https://github.com/octocat/hello-world/issues/42"
    );
    assert_eq!(minted.canonical_path(), "/octocat/hello-world/issues/42");
}
