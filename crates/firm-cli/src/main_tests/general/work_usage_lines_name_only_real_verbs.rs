use super::*;

/// Every `team-run work a|b|c` list the CLI prints — the `require_subcommand`
/// usage, the unknown-command usage, and `print_help` — must name only verbs
/// that dispatch. `review` was advertised in two of them and in
/// `docs/current/product/agent-team-works.md` with no match arm anywhere, and
/// `delegate`/`delegation` stayed advertised after an early return made their
/// arms unreachable.
#[test]
fn work_usage_lines_name_only_real_verbs() {
    let source = cli_command_source();
    let work_body = function_body(&source, "team_run_work_command");

    let mut lists = Vec::new();
    for (offset, _) in source.match_indices("team-run work ") {
        let rest = &source[offset + "team-run work ".len()..];
        let end = rest
            .find(|character: char| {
                !(character.is_ascii_lowercase() || character == '-' || character == '|')
            })
            .unwrap_or(rest.len());
        let list = &rest[..end];
        if list.contains('|') {
            lists.push(list);
        }
    }
    assert!(
        lists.len() >= 3,
        "expected the usage, unknown-command and help verb lists, found {lists:?}"
    );

    for list in &lists {
        for verb in list.split('|') {
            assert!(
                subcommand_is_real(work_body, verb),
                "`team-run work {verb}` is advertised in `{list}` but has no match arm in \
                 team_run_work_command"
            );
        }
    }

    for phantom in ["review", "delegate", "delegation"] {
        assert!(
            !lists
                .iter()
                .any(|list| list.split('|').any(|verb| verb == phantom)),
            "`team-run work {phantom}` is still advertised but does not dispatch"
        );
    }
}
