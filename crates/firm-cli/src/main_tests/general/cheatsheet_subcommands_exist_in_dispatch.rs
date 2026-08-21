use super::*;

#[test]
fn cheatsheet_subcommands_exist_in_dispatch() {
    let source = cli_command_source();
    let team_run_body = function_body(&source, "team_run_command");
    for leaf in [
        "create",
        "start",
        "add-member",
        "status",
        "wait",
        "send",
        "host-inbox",
        "ack",
        "events",
        "board-summary",
        "recover",
    ] {
        assert!(
            subcommand_is_real(team_run_body, leaf),
            "team-run {leaf} is documented in the cheatsheet but is not a \
                 real match arm in team_run_command"
        );
    }
    let work_body = function_body(&source, "team_run_work_command");
    for leaf in [
        "create",
        "list",
        "show",
        "assign",
        "accept",
        "request-changes",
    ] {
        assert!(
            subcommand_is_real(work_body, leaf),
            "work {leaf} is documented in the cheatsheet but is not a \
                 real match arm in team_run_work_command"
        );
    }
    let mission_body = function_body(&source, "mission_command");
    // Mission writers retired with the legacy CompanyOS cutover
    // (DOC-108); only the read-only legacy reads remain documented.
    for leaf in ["list", "show", "log"] {
        assert!(
            subcommand_is_real(mission_body, leaf),
            "mission {leaf} is documented in the cheatsheet but is not a \
                 real match arm in mission_command"
        );
    }
    // `mission log` is its own nested dispatcher (ADR 0051 Mission Log).
    let mission_log_body = function_body(&source, "mission_log_command");
    assert!(
        subcommand_is_real(mission_log_body, "show"),
        "mission log show is documented in the cheatsheet but is not a \
             real match arm in mission_log_command"
    );
    // Wave is absent from current workflow cheatsheets. Historical reads
    // live under the explicit `legacy wave` namespace only.
    assert!(!CHEATSHEET_MISSION.contains("wave "));
    assert!(!CHEATSHEET_ALL.contains("wave "));
    for retired in ["create", "update", "advance", "gate"] {
        assert!(
            !CHEATSHEET_MISSION.contains(&format!("wave {retired}")),
            "CHEATSHEET_MISSION documents retired `wave {retired}`; ADR 0051 retired \
                 Wave write commands"
        );
        assert!(
            !CHEATSHEET_ALL.contains(&format!("wave {retired}")),
            "CHEATSHEET_ALL documents retired `wave {retired}`; ADR 0051 retired \
                 Wave write commands"
        );
    }
    // Mission writers are likewise absent after DOC-108: the cheatsheets
    // document only the read-only legacy Mission reads.
    for retired in [
        "mission create",
        "update-context",
        "mission close",
        "log append",
    ] {
        assert!(
            !CHEATSHEET_MISSION.contains(retired),
            "CHEATSHEET_MISSION documents retired `{retired}`; DOC-108 retired Mission \
                 write commands, historical rows stay read-only"
        );
        assert!(
            !CHEATSHEET_ALL.contains(retired),
            "CHEATSHEET_ALL documents retired `{retired}`; DOC-108 retired Mission \
                 write commands, historical rows stay read-only"
        );
    }
}
