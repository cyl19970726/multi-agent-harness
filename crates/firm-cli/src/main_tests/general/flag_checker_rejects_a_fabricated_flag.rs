use super::*;

#[test]
fn flag_checker_rejects_a_fabricated_flag() {
    let source = cli_command_source();
    // Proves flag_is_wired()/subcommand_is_real() actually discriminate
    // real CLI surface from made-up surface. The version this replaces
    // synthesized `["--flag", "placeholder"]` and fed it straight back
    // into value(), which trivially returns Some("placeholder") for ANY
    // string -- it could never fail no matter what the cheatsheet
    // claimed.
    assert!(
        !flag_is_wired(&source, "--frobnicate-widget"),
        "flag_is_wired must reject a flag that is not real"
    );
    let team_run_body = function_body(&source, "team_run_command");
    assert!(
        !subcommand_is_real(team_run_body, "levitate"),
        "subcommand_is_real must reject a leaf that is not a real match arm"
    );
    // And sanity-check the positive case on the same inputs, so a
    // trivial "always return false" implementation cannot pass this test.
    assert!(flag_is_wired(&source, "--objective"));
    assert!(subcommand_is_real(team_run_body, "create"));
}
