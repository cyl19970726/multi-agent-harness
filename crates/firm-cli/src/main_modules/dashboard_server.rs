use super::*;

pub(super) fn dashboard_command(
    store: &HarnessStore,
    _resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    require_subcommand(
        args,
        "dashboard snapshot | dashboard doctor --team-run-id <id> --api <base-url>",
    )?;
    match args[0].as_str() {
        "doctor" => dashboard_doctor_command(store, &args[1..])?,
        "snapshot" => print_json(&dashboard_snapshot(store)?)?,
        other => {
            return Err(CliError::Usage(format!(
                "unknown dashboard command: {other}"
            )))
        }
    }
    Ok(())
}

/// `harness dashboard doctor --team-run-id <id> --api <base-url>` (issue #307,
/// item 3) — a read-only, operator-facing check that a dashboard pointed at
/// `--api` would show Store truth for the given TeamRun. It fetches the exact
/// two things the Workbench itself fetches (`GET /v1/meta` and
/// `GET /v1/team-runs/{id}/snapshot`) and compares them against THIS process's
/// own direct store reads (bypassing HTTP entirely) plus this CLI binary's own
/// build rev (embedded the same way the server's is — `build_git_rev`, or an
/// explicit `--expected-git-rev` override for CI that deploys a different
/// commit than it runs doctor from). It performs no writes. A count mismatch
/// or a git_rev disagreement both fail non-zero.
pub(super) fn dashboard_doctor_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let team_run_id = required(args, "--team-run-id")?;
    let api = required(args, "--api")?;
    // Owned (not `Option<&str>`) so defaulting to this binary's own compiled-in
    // rev is a plain value fallback, not a borrow that has to outlive `args`.
    let expected_git_rev =
        value(args, "--expected-git-rev").unwrap_or_else(|| build_git_rev().to_string());
    if expected_git_rev != "unknown" && !is_exact_git_rev(&expected_git_rev) {
        return Err(CliError::Usage(
            "--expected-git-rev must be a full 40-character hexadecimal SHA".to_string(),
        ));
    }

    let member_runs = latest_member_runs_in_append_order(store)?;
    let message_ids = canonical_team_messages_for_run(store, &team_run_id)?
        .into_iter()
        .map(|message| message.id)
        .collect::<BTreeSet<_>>();
    let store_counts = DoctorStoreCounts {
        works: store
            .latest_works()?
            .into_iter()
            .filter(|work| work.team_run_id == team_run_id)
            .count(),
        members: member_runs
            .iter()
            .filter(|member| member.team_run_id == team_run_id)
            .count(),
        messages: message_ids.len(),
    };

    let (meta_status, meta) = http_get_json(&api, "/v1/meta")?;
    if meta_status != 200 {
        return Err(CliError::Usage(format!(
            "GET {api}/v1/meta returned HTTP {meta_status}: {meta}"
        )));
    }
    // `--team-run-id` is an operator-supplied CLI flag, not untrusted web
    // input, and Harness's own generated ids are always plain
    // alphanumeric/hyphen — no percent-encoding needed for this path segment.
    let snapshot_path = format!("/v1/team-runs/{team_run_id}/snapshot");
    let (snapshot_status, snapshot) = http_get_json(&api, &snapshot_path)?;
    if snapshot_status != 200 {
        return Err(CliError::Usage(format!(
            "GET {api}{snapshot_path} returned HTTP {snapshot_status}: {snapshot}"
        )));
    }

    let report = doctor_report(&store_counts, &meta, &snapshot, &expected_git_rev);
    print_doctor_report(&team_run_id, &api, &report);
    if report.all_pass() {
        Ok(())
    } else {
        Err(CliError::Usage(
            "dashboard doctor: the API disagrees with direct store reads or the server build; see table above"
                .to_string(),
        ))
    }
}

/// Direct-store Work/member/message counts for one TeamRun — bypasses HTTP
/// entirely, so it is the ground truth `dashboard doctor` compares the API
/// against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DoctorStoreCounts {
    pub(super) works: usize,
    pub(super) members: usize,
    pub(super) messages: usize,
}

/// One row of the printed `dashboard doctor` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DoctorCheck {
    pub(super) label: &'static str,
    pub(super) expected: String,
    pub(super) observed: String,
    pub(super) pass: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct DoctorReport {
    pub(super) checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub(super) fn all_pass(&self) -> bool {
        self.checks.iter().all(|check| check.pass)
    }
}

/// Pure comparison, no I/O, so it is unit-testable without a live server.
/// `store_counts` and `expected_git_rev` are this process's own ground truth;
/// `meta` and `snapshot` are exactly the JSON bodies a dashboard client would
/// receive from `GET /v1/meta` and `GET /v1/team-runs/{id}/snapshot`.
pub(super) fn doctor_report(
    store_counts: &DoctorStoreCounts,
    meta: &serde_json::Value,
    snapshot: &serde_json::Value,
    expected_git_rev: &str,
) -> DoctorReport {
    let api_works = json_array_len(snapshot, "works");
    let api_members = json_array_len(snapshot, "member_runs");
    let api_messages = json_array_len(snapshot, "team_messages");
    let server_git_rev = meta
        .get("git_rev")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // Equality is meaningful only for two exact object ids. `unknown ==
    // unknown` cannot prove that the dashboard and CLI came from one revision.
    let rev_pass = is_exact_git_rev(expected_git_rev)
        && is_exact_git_rev(server_git_rev)
        && expected_git_rev.eq_ignore_ascii_case(server_git_rev);

    DoctorReport {
        checks: vec![
            DoctorCheck {
                label: "works count (store vs API)",
                expected: store_counts.works.to_string(),
                observed: api_works.to_string(),
                pass: store_counts.works == api_works,
            },
            DoctorCheck {
                label: "members count (store vs API)",
                expected: store_counts.members.to_string(),
                observed: api_members.to_string(),
                pass: store_counts.members == api_members,
            },
            DoctorCheck {
                label: "messages count (store vs API)",
                expected: store_counts.messages.to_string(),
                observed: api_messages.to_string(),
                pass: store_counts.messages == api_messages,
            },
            DoctorCheck {
                label: "git_rev (this build vs server)",
                expected: expected_git_rev.to_string(),
                observed: server_git_rev.to_string(),
                pass: rev_pass,
            },
        ],
    }
}

pub(super) fn is_exact_git_rev(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn json_array_len(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

pub(super) fn print_doctor_report(team_run_id: &str, api: &str, report: &DoctorReport) {
    println!("harness dashboard doctor — team-run {team_run_id} against {api}");
    for check in &report.checks {
        println!(
            "  {:<32} expected={:<24} observed={:<24} {}",
            check.label,
            check.expected,
            check.observed,
            if check.pass { "PASS" } else { "FAIL" },
        );
    }
    if report.all_pass() {
        println!("PASS — API and this store agree; server build matches.");
    } else {
        println!(
            "FAIL — the API (what a dashboard renders) disagrees with direct store reads or the server build. See rows above."
        );
    }
}

/// Strip an optional `http(s)://` scheme, returning the bare `host:port`
/// `TcpStream::connect` needs. `dashboard doctor` only ever talks to a
/// local/plain-HTTP `harness serve` (the same transport `serve` itself
/// speaks) — no TLS.
pub(super) fn http_authority(base_url: &str) -> CliResult<String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let authority = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    if authority.is_empty() {
        return Err(CliError::Usage("--api must not be empty".to_string()));
    }
    Ok(authority.to_string())
}

/// A minimal blocking HTTP/1.1 GET over a raw TCP socket — `dashboard doctor`'s
/// only network call. This workspace has no HTTP client crate (`serve` itself
/// is hand-rolled TCP/HTTP — see `handle_http_connection`), and adding one for
/// a single CLI diagnostic is not worth a new dependency. Returns
/// `(status_code, parsed_json_body)`.
pub(super) fn http_get_json(base_url: &str, path: &str) -> CliResult<(u16, serde_json::Value)> {
    let authority = http_authority(base_url)?;
    let mut stream = TcpStream::connect(&authority).map_err(|error| {
        CliError::Usage(format!(
            "cannot reach --api {base_url} ({authority}): {error}"
        ))
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
    )?;
    let mut raw = String::new();
    read_http_response_to_string(&mut stream, &mut raw)?;
    let (header_part, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| CliError::Usage(format!("malformed HTTP response from {base_url}{path}")))?;
    let status = header_part
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| CliError::Usage(format!("malformed HTTP status from {base_url}{path}")))?;
    let json = serde_json::from_str(body.trim()).map_err(|error| {
        CliError::Usage(format!("{base_url}{path} did not return JSON: {error}"))
    })?;
    Ok((status, json))
}

/// Linux may report `ECONNRESET` after the peer has already written a
/// complete `Connection: close` response (the same transport quirk the serve
/// integration test harness works around). Accept that ending only when the
/// declared Content-Length is fully present; any other read error propagates.
pub(super) fn read_http_response_to_string(
    stream: &mut TcpStream,
    raw: &mut String,
) -> CliResult<()> {
    match stream.read_to_string(raw) {
        Ok(_) => Ok(()),
        Err(error)
            if error.kind() == std::io::ErrorKind::ConnectionReset
                && http_response_looks_complete(raw) =>
        {
            Ok(())
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

pub(super) fn http_response_looks_complete(raw: &str) -> bool {
    let Some((headers, body)) = raw.split_once("\r\n\r\n") else {
        return false;
    };
    let Some(content_length) = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    }) else {
        return false;
    };
    body.len() >= content_length
}

#[cfg(test)]
mod dashboard_doctor_tests {
    use super::*;

    const REV_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const REV_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn counts(works: usize, members: usize, messages: usize) -> DoctorStoreCounts {
        DoctorStoreCounts {
            works,
            members,
            messages,
        }
    }

    fn snapshot(works: usize, members: usize, messages: usize) -> serde_json::Value {
        serde_json::json!({
            "works": vec![serde_json::json!({}); works],
            "member_runs": vec![serde_json::json!({}); members],
            "team_messages": vec![serde_json::json!({}); messages],
        })
    }

    fn meta(git_rev: &str) -> serde_json::Value {
        serde_json::json!({"git_rev": git_rev})
    }

    #[test]
    fn all_matching_counts_and_revs_pass() {
        let report = doctor_report(&counts(2, 1, 3), &meta(REV_A), &snapshot(2, 1, 3), REV_A);
        assert!(report.all_pass(), "{report:?}");
        assert!(report.checks.iter().all(|check| check.pass));
    }

    #[test]
    fn works_count_mismatch_fails_only_that_row() {
        let report = doctor_report(&counts(2, 1, 3), &meta(REV_A), &snapshot(1, 1, 3), REV_A);
        assert!(!report.all_pass());
        let works_row = &report.checks[0];
        assert_eq!(works_row.label, "works count (store vs API)");
        assert!(!works_row.pass);
        assert!(report.checks[1].pass, "members row should be unaffected");
        assert!(report.checks[2].pass, "messages row should be unaffected");
    }

    #[test]
    fn members_count_mismatch_fails() {
        let report = doctor_report(&counts(2, 1, 3), &meta(REV_A), &snapshot(2, 0, 3), REV_A);
        assert!(!report.all_pass());
        assert!(!report.checks[1].pass);
    }

    #[test]
    fn messages_count_mismatch_fails() {
        let report = doctor_report(&counts(2, 1, 3), &meta(REV_A), &snapshot(2, 1, 0), REV_A);
        assert!(!report.all_pass());
        assert!(!report.checks[2].pass);
    }

    #[test]
    fn git_rev_mismatch_fails_the_rev_row_even_when_counts_match() {
        let report = doctor_report(&counts(0, 0, 0), &meta(REV_B), &snapshot(0, 0, 0), REV_A);
        assert!(!report.all_pass());
        let rev_row = report.checks.last().expect("rev row");
        assert_eq!(rev_row.label, "git_rev (this build vs server)");
        assert!(!rev_row.pass);
        assert_eq!(rev_row.expected, REV_A);
        assert_eq!(rev_row.observed, REV_B);
    }

    #[test]
    fn both_sides_reporting_unknown_git_rev_fail_exact_revision_proof() {
        let report = doctor_report(
            &counts(0, 0, 0),
            &meta("unknown"),
            &snapshot(0, 0, 0),
            "unknown",
        );
        assert!(!report.all_pass(), "{report:?}");
        assert!(!report.checks.last().expect("rev row").pass);
    }

    #[test]
    fn missing_meta_git_rev_field_defaults_to_unknown_and_fails_against_a_known_expected_rev() {
        // A server that cannot even report SOME git_rev while this build
        // knows its own concrete rev is a real provenance gap, not something
        // to silently pass.
        let report = doctor_report(
            &counts(0, 0, 0),
            &serde_json::json!({}),
            &snapshot(0, 0, 0),
            REV_A,
        );
        let rev_row = report.checks.last().expect("rev row");
        assert_eq!(rev_row.observed, "unknown");
        assert!(
            !rev_row.pass,
            "a concrete expected rev vs an unreported server rev must fail"
        );
    }

    #[test]
    fn missing_snapshot_arrays_count_as_zero_not_a_panic() {
        let report = doctor_report(
            &counts(0, 0, 0),
            &meta(REV_A),
            &serde_json::json!({}),
            REV_A,
        );
        assert!(report.all_pass(), "{report:?}");
    }

    #[test]
    fn http_authority_strips_scheme_and_trailing_slash() {
        assert_eq!(
            http_authority("http://127.0.0.1:8787/").unwrap(),
            "127.0.0.1:8787"
        );
        assert_eq!(
            http_authority("https://example.com").unwrap(),
            "example.com"
        );
        assert_eq!(http_authority("127.0.0.1:8787").unwrap(), "127.0.0.1:8787");
        assert!(http_authority("   ").is_err());
    }
}

pub(super) fn broadcast_native_session_wake(
    manager: &sse::SseManager,
    execution_space_id: &str,
    project_binding_id: &str,
    owner_agent_member_id: &str,
    event: serde_json::Value,
) -> serde_json::Value {
    manager.broadcast_native_session_wake(
        execution_space_id,
        project_binding_id,
        owner_agent_member_id,
        event.clone(),
    );
    event
}

pub(super) struct SseSelection<'a> {
    pub project_binding_id: Option<&'a str>,
    pub company_scope_id: Option<&'a str>,
    pub team_id: Option<&'a str>,
    pub agent_member_id: Option<&'a str>,
}

pub(super) fn handle_sse_stream(
    store: &HarnessStore,
    execution_space_id: &str,
    selection: SseSelection<'_>,
    mut stream: HttpResponseWriter<TcpStream>,
    sse_manager: sse::SseManager,
) -> CliResult<()> {
    use std::time::Duration;

    let SseSelection {
        project_binding_id: selected_project_binding_id,
        company_scope_id,
        team_id: selected_team_id,
        agent_member_id: selected_agent_member_id,
    } = selection;

    // Subscribe before exposing the initial snapshot marker. The browser starts
    // its authoritative GET after that marker; registering first guarantees
    // that a write crossing the marker -> GET boundary is queued for this
    // stream instead of falling into a gap between the GET and subscription.
    let rx = sse_manager.subscribe_scoped_agent_session(
        execution_space_id,
        company_scope_id,
        selected_agent_member_id,
        selected_project_binding_id,
    );

    let session_scope = selected_project_binding_id
        .zip(selected_team_id)
        .zip(selected_agent_member_id);
    let mut persisted_read = None;

    // Send SSE header
    sse::write_sse_header(&mut stream)?;

    // Send initial snapshot
    // Initial snapshot sent to client for sync
    let _snapshot = sse::SseEventFrame::Snapshot {
        messages: Vec::new(),
        generated_at: now_string(),
    };

    // Convert snapshot to JSON for transmission
    let snapshot_json = serde_json::json!({
        "generated_at": now_string(),
        "execution_space_id": execution_space_id,
        "selected_project_binding_id": selected_project_binding_id,
        "company_scope_id": company_scope_id,
        "team_session_persisted_events": selected_agent_member_id.is_some(),
        "stream_epoch": sse_manager.stream_epoch(),
    });
    sse::write_sse_frame(&mut stream, "snapshot", &snapshot_json)?;

    if let Some(((project_binding_id, team_id), agent_member_id)) = session_scope {
        persisted_read = initialize_persisted_session_read(
            store,
            execution_space_id,
            project_binding_id,
            team_id,
            agent_member_id,
            &mut stream,
        )?;
    }

    // Deterministic integration-test hook for the marker -> GET crossing. The
    // subscription is intentionally already live while this pause is active.
    if let Some(pause) = sse_post_snapshot_test_pause() {
        std::thread::sleep(pause);
    }
    let mut last_keepalive = std::time::Instant::now();

    // Wait for events and stream them to the client
    loop {
        // Calculate timeout for the next keepalive
        let elapsed = last_keepalive.elapsed();
        let poll_interval = if session_scope.is_some() {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(15)
        };
        let timeout = if elapsed < poll_interval {
            poll_interval - elapsed
        } else {
            Duration::from_millis(100)
        };

        match rx.recv_timeout(timeout) {
            Ok(frame) => {
                match frame {
                    sse::SseEventFrame::Snapshot { .. } => {
                        // Don't re-send snapshots after initial
                    }
                    sse::SseEventFrame::RegistryMessage(msg) => {
                        if let Ok(json) = serde_json::to_value(&msg) {
                            if sse::write_sse_frame(&mut stream, "message", &json).is_err() {
                                break; // Client disconnected
                            }
                        }
                    }
                    // Agent Team v0: folded per-run events (team console merges
                    // these incrementally).
                    sse::SseEventFrame::TeamRunEvent(event) => {
                        if let Ok(json) = serde_json::to_value(&event) {
                            if sse::write_sse_frame(&mut stream, "team_run_event", &json).is_err() {
                                break; // Client disconnected
                            }
                        }
                    }
                    sse::SseEventFrame::Mission(mission) => {
                        if let Ok(json) = serde_json::to_value(&mission) {
                            if sse::write_sse_frame(&mut stream, "mission", &json).is_err() {
                                break;
                            }
                        }
                    }
                    sse::SseEventFrame::AgentTeamRun(run) => {
                        if let Ok(json) = serde_json::to_value(&run) {
                            if sse::write_sse_frame(&mut stream, "agent_team_run", &json).is_err() {
                                break;
                            }
                        }
                    }
                    sse::SseEventFrame::ProviderRuntimeProjection(member) => {
                        if let Ok(json) = serde_json::to_value(&member) {
                            if sse::write_sse_frame(&mut stream, "member_run", &json).is_err() {
                                break;
                            }
                        }
                    }
                    sse::SseEventFrame::TeamSupervisorLease(lease) => {
                        if let Ok(json) = serde_json::to_value(&lease) {
                            if sse::write_sse_frame(&mut stream, "team_supervisor_lease", &json)
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    sse::SseEventFrame::TeamMemberCloseRequest(request) => {
                        if let Ok(json) = serde_json::to_value(&request) {
                            if sse::write_sse_frame(&mut stream, "team_member_close_request", &json)
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    sse::SseEventFrame::MemberAction(action) => {
                        if let Ok(json) = serde_json::to_value(&action) {
                            if sse::write_sse_frame(&mut stream, "member_action", &json).is_err() {
                                break;
                            }
                        }
                    }
                    sse::SseEventFrame::ProjectionInvalidated(invalidation) => {
                        if let Ok(json) = serde_json::to_value(&invalidation) {
                            if sse::write_sse_frame(&mut stream, "projection_invalidated", &json)
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    sse::SseEventFrame::NativeSessionWake(_wake) => {
                        // Provider callbacks are payload-free wake hints only.
                        // The browser receives semantic records exclusively from
                        // the persisted Session reader below.
                        if persisted_read.is_none() {
                            if let Some(((project_binding_id, team_id), agent_member_id)) =
                                session_scope
                            {
                                persisted_read = initialize_persisted_session_read(
                                    store,
                                    execution_space_id,
                                    project_binding_id,
                                    team_id,
                                    agent_member_id,
                                    &mut stream,
                                )?;
                            }
                        }
                        if emit_persisted_session_advance(&mut stream, persisted_read.as_mut())
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                last_keepalive = std::time::Instant::now();
            }
            Err(crossbeam::channel::RecvTimeoutError::Timeout) => {
                if persisted_read.is_none() {
                    if let Some(((project_binding_id, team_id), agent_member_id)) = session_scope {
                        persisted_read = initialize_persisted_session_read(
                            store,
                            execution_space_id,
                            project_binding_id,
                            team_id,
                            agent_member_id,
                            &mut stream,
                        )?;
                    }
                }
                if emit_persisted_session_advance(&mut stream, persisted_read.as_mut()).is_err() {
                    break;
                }
                if last_keepalive.elapsed() >= Duration::from_secs(15) {
                    if sse::write_sse_keepalive(&mut stream).is_err() {
                        break; // Client disconnected
                    }
                    last_keepalive = std::time::Instant::now();
                }
            }
            Err(crossbeam::channel::RecvTimeoutError::Disconnected) => {
                break; // Channel closed, exit
            }
        }
    }

    Ok(())
}

fn initialize_persisted_session_read(
    store: &HarnessStore,
    execution_space_id: &str,
    project_binding_id: &str,
    team_id: &str,
    agent_member_id: &str,
    stream: &mut HttpResponseWriter<TcpStream>,
) -> CliResult<Option<provider_event_api::PersistedSessionReadRequest>> {
    let Ok(mut request) = provider_event_api::local_operator_session_read_request(
        store,
        execution_space_id,
        project_binding_id,
        team_id,
        agent_member_id,
        provider_event_api::DEFAULT_SESSION_PAGE_SIZE,
    ) else {
        return Ok(None);
    };
    let firm_home =
        crate::execution_space::firm_home().map_err(|error| CliError::Usage(error.to_string()))?;
    let Ok(response) = crate::supervisor_daemon::native_session_read_via_socket(
        &firm_home,
        &request.node_id,
        &request,
    ) else {
        return Ok(None);
    };
    sse::write_sse_frame(
        stream,
        "native_session_snapshot",
        &serde_json::to_value(&response)?,
    )?;
    if let Some(watermark) = response.snapshot_watermark {
        request.mode = provider_event_api::PersistedSessionReadMode::After;
        request.cursor = Some(provider_event_api::PersistedSessionCursor {
            source_generation: response.source_generation,
            ordering_key: watermark,
        });
    }
    Ok(Some(request))
}

fn emit_persisted_session_advance(
    stream: &mut HttpResponseWriter<TcpStream>,
    request: Option<&mut provider_event_api::PersistedSessionReadRequest>,
) -> CliResult<()> {
    let Some(request) = request else {
        return Ok(());
    };
    let firm_home =
        crate::execution_space::firm_home().map_err(|error| CliError::Usage(error.to_string()))?;
    if request.mode == provider_event_api::PersistedSessionReadMode::Snapshot {
        let response = crate::supervisor_daemon::native_session_read_via_socket(
            &firm_home,
            &request.node_id,
            request,
        )
        .map_err(CliError::Io)?;
        if !response.records.is_empty() {
            sse::write_sse_frame(
                stream,
                "native_session_snapshot",
                &serde_json::to_value(&response)?,
            )?;
        }
        if let Some(watermark) = response.snapshot_watermark {
            request.mode = provider_event_api::PersistedSessionReadMode::After;
            request.cursor = Some(provider_event_api::PersistedSessionCursor {
                source_generation: response.source_generation,
                ordering_key: watermark,
            });
        }
        return Ok(());
    }
    if request.cursor.is_none() {
        return Ok(());
    }
    let response = crate::supervisor_daemon::native_session_read_via_socket(
        &firm_home,
        &request.node_id,
        request,
    )
    .map_err(CliError::Io)?;
    if response.source_reset {
        sse::write_sse_frame(
            stream,
            "native_session_source_reset",
            &serde_json::to_value(&response)?,
        )?;
        request.mode = provider_event_api::PersistedSessionReadMode::Snapshot;
        request.cursor = None;
        let snapshot = crate::supervisor_daemon::native_session_read_via_socket(
            &firm_home,
            &request.node_id,
            request,
        )
        .map_err(CliError::Io)?;
        sse::write_sse_frame(
            stream,
            "native_session_snapshot",
            &serde_json::to_value(&snapshot)?,
        )?;
        if let Some(watermark) = snapshot.snapshot_watermark {
            request.mode = provider_event_api::PersistedSessionReadMode::After;
            request.cursor = Some(provider_event_api::PersistedSessionCursor {
                source_generation: snapshot.source_generation,
                ordering_key: watermark,
            });
        }
        return Ok(());
    }
    if !response.records.is_empty() {
        sse::write_sse_frame(
            stream,
            "native_session_append",
            &serde_json::to_value(&response)?,
        )?;
    }
    if let Some(watermark) = response.snapshot_watermark {
        request.cursor = Some(provider_event_api::PersistedSessionCursor {
            source_generation: response.source_generation,
            ordering_key: watermark,
        });
    }
    Ok(())
}

pub(super) fn sse_post_snapshot_test_pause() -> Option<std::time::Duration> {
    std::env::var("FIRM_TEST_SSE_POST_SNAPSHOT_PAUSE_MS")
        .or_else(|_| std::env::var("HARNESS_TEST_SSE_POST_SNAPSHOT_PAUSE_MS"))
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(std::time::Duration::from_millis)
}

pub(super) fn dashboard_snapshot_build_test_pause() -> Option<std::time::Duration> {
    std::env::var("FIRM_TEST_DASHBOARD_SNAPSHOT_BUILD_PAUSE_MS")
        .or_else(|_| std::env::var("HARNESS_TEST_DASHBOARD_SNAPSHOT_BUILD_PAUSE_MS"))
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(std::time::Duration::from_millis)
}

/// One serve-process fence around the synchronous full Dashboard Store build.
///
/// HTTP connections remain independently threaded so SSE and writes never
/// head-of-line block. Full snapshots are the exception: their Store scan is
/// synchronous and cannot be interrupted after a browser disconnect, so a
/// true scope handoff must wait outside the builder instead of starting a
/// second abandoned scan. The browser still owns first-success/coalesced-dirty
/// semantics; this fence owns only backend build concurrency.
#[derive(Default)]
pub(super) struct DashboardSnapshotBuildFence {
    pub(super) gate: Mutex<()>,
    pub(super) active: AtomicU64,
    pub(super) max_active: AtomicU64,
    pub(super) started: AtomicU64,
    pub(super) completed: AtomicU64,
}

pub(super) struct ActiveDashboardSnapshotBuild<'a> {
    pub(super) fence: &'a DashboardSnapshotBuildFence,
    pub(super) _gate: std::sync::MutexGuard<'a, ()>,
}

impl Drop for ActiveDashboardSnapshotBuild<'_> {
    fn drop(&mut self) {
        self.fence.active.fetch_sub(1, Ordering::SeqCst);
        self.fence.completed.fetch_add(1, Ordering::SeqCst);
    }
}

impl DashboardSnapshotBuildFence {
    pub(super) fn build<T>(&self, build: impl FnOnce() -> CliResult<T>) -> CliResult<T> {
        let gate = self
            .gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        self.started.fetch_add(1, Ordering::SeqCst);
        let _active = ActiveDashboardSnapshotBuild {
            fence: self,
            _gate: gate,
        };
        if let Some(pause) = dashboard_snapshot_build_test_pause() {
            std::thread::sleep(pause);
        }
        build()
    }

    pub(super) fn test_metrics(&self) -> serde_json::Value {
        serde_json::json!({
            "active": self.active.load(Ordering::SeqCst),
            "max_active": self.max_active.load(Ordering::SeqCst),
            "started": self.started.load(Ordering::SeqCst),
            "completed": self.completed.load(Ordering::SeqCst),
        })
    }
}

#[cfg(test)]
mod dashboard_snapshot_build_tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn full_snapshot_store_builds_are_serialized() {
        let fence = Arc::new(DashboardSnapshotBuildFence::default());
        let rendezvous = Arc::new(Barrier::new(5));
        let active_builds = Arc::new(AtomicU64::new(0));
        let max_active_builds = Arc::new(AtomicU64::new(0));
        let mut workers = Vec::new();

        for _ in 0..4 {
            let worker_fence = Arc::clone(&fence);
            let worker_rendezvous = Arc::clone(&rendezvous);
            let worker_active = Arc::clone(&active_builds);
            let worker_max = Arc::clone(&max_active_builds);
            workers.push(std::thread::spawn(move || {
                worker_rendezvous.wait();
                worker_fence
                    .build(|| {
                        let active = worker_active.fetch_add(1, Ordering::SeqCst) + 1;
                        worker_max.fetch_max(active, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(25));
                        worker_active.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .expect("snapshot build");
            }));
        }

        rendezvous.wait();
        for worker in workers {
            worker.join().expect("snapshot worker");
        }
        let metrics = fence.test_metrics();
        assert_eq!(max_active_builds.load(Ordering::SeqCst), 1);
        assert_eq!(metrics["max_active"], 1);
        assert_eq!(metrics["started"], 4);
        assert_eq!(metrics["completed"], 4);
        assert_eq!(metrics["active"], 0);
    }
}

/// Independent Execution Space and Project Binding routing for one live serve.
///
/// Native mode routes `?space` to coordination storage and `?project` to
/// provider execution context. Raw-store and project-derived compatibility
/// modes retain the historical single-store behavior.
#[derive(Clone)]
pub(super) struct NativeSessionWakeCallback {
    pub(super) authority: String,
    pub(super) token: String,
    pub(super) serve_instance_id: String,
}

#[derive(Clone)]
pub(super) struct ServeProjects {
    /// `~/.harness` — `None` only when serve was started with a raw
    /// `--store`/`FIRM_ROOT` override (no registry to consult).
    pub(super) firm_home: Option<PathBuf>,
    /// The id of the project `serve` started for (the active/`_global` project, or a
    /// synthetic id in raw-override mode). Used as the default when no `?project`.
    pub(super) default_id: String,
    /// The store resolved at startup — the default project's store.
    pub(super) default_store: HarnessStore,
    /// Native execution-space identity when serve was started through one.
    pub(super) default_space: Option<ExecutionSpace>,
    /// Preserve the exact startup context even when it came from an
    /// unregistered Git worktree path. Reconstructing it from the synthetic id
    /// would otherwise collapse project_root into store_root, and provider
    /// members would receive an unusable FIRM_PROJECT selector.
    pub(super) default_context: Option<ProjectContext>,
    /// Shared by every per-connection thread in this serve process. It fences
    /// the synchronous full-snapshot Store builder without serializing SSE,
    /// authenticated RoleView reads/writes, or other HTTP work.
    pub(super) dashboard_snapshot_builds: Arc<DashboardSnapshotBuildFence>,
    /// Process-memory-only callback capability. It is registered per exact
    /// authenticated AgentMember when that owner opens a private SSE stream;
    /// an ambient same-user process cannot install a global private sink.
    pub(super) native_session_wake_callback: Option<NativeSessionWakeCallback>,
}

impl ServeProjects {
    /// Build from the store resolved in `run()` plus its `ResolvedStore` record.
    pub(super) fn from_resolved(store: &HarnessStore, resolved: &ResolvedStore) -> Self {
        // A project identity only exists when resolution went through the registry /
        // global path (not a raw `--store`/`FIRM_ROOT` override).
        let registry_backed =
            resolved.context.is_some() || resolved.execution_space_context.is_some();
        let firm_home = registry_backed
            .then(project::firm_home)
            .and_then(Result::ok);
        let default_id = resolved
            .context
            .as_ref()
            .map(|context| context.id.clone())
            .unwrap_or_else(|| "_store".to_string());
        Self {
            firm_home,
            default_id: resolved
                .execution_space_context
                .as_ref()
                .map(|space| space.id.clone())
                .unwrap_or(default_id),
            default_store: store.clone(),
            default_space: resolved.execution_space_context.clone(),
            default_context: resolved.context.clone(),
            dashboard_snapshot_builds: Arc::new(DashboardSnapshotBuildFence::default()),
            native_session_wake_callback: None,
        }
    }

    /// Resolve an Execution Space selector to its coordination store. In
    /// compatibility mode (no native space selected at startup), project-derived
    /// stores remain readable through the old selector.
    pub(super) fn store_for(&self, selector: Option<&str>) -> CliResult<(String, HarnessStore)> {
        let Some(id) = selector.filter(|id| !id.is_empty()) else {
            return Ok((self.default_id.clone(), self.default_store.clone()));
        };
        if id == self.default_id {
            return Ok((self.default_id.clone(), self.default_store.clone()));
        }
        if let Some(home) = &self.firm_home {
            if let Some(space) =
                execution_space::context_for_id(home, id).map_err(execution_space_err)?
            {
                return Ok((space.id, HarnessStore::new(space.store_root)));
            }
            if self.default_space.is_none() {
                if let Some(ctx) = project::context_for_id(home, id).map_err(project_err)? {
                    return Ok((ctx.id, HarnessStore::new(ctx.store_root)));
                }
            }
        }
        Err(CliError::Usage(format!(
            "unknown {}: {id}",
            if self.default_space.is_some() {
                "execution space"
            } else {
                "coordination store"
            }
        )))
    }

    /// Resolve the independent Project Binding used for provider cwd.
    pub(super) fn context_for(
        &self,
        project_binding_id: Option<&str>,
        execution_space_id: Option<&str>,
        store: &HarnessStore,
    ) -> ProjectContext {
        if let (Some(id), Some(default)) = (project_binding_id, &self.default_context) {
            if default.id == id {
                return default.clone();
            }
        }
        if let (Some(home), Some(id)) = (&self.firm_home, project_binding_id) {
            if let Ok(Some(context)) = project::context_for_id(home, id) {
                return context;
            }
        }
        if project_binding_id.is_none() {
            if let Some(binding_id) = execution_space_id
                .and_then(|id| self.space_context_for(id))
                .and_then(|space| space.default_project_binding_id)
            {
                if let Some(context) = self
                    .firm_home
                    .as_ref()
                    .and_then(|home| project::context_for_id(home, &binding_id).ok().flatten())
                {
                    return context;
                }
            }
            if let Some(context) = &self.default_context {
                return context.clone();
            }
        }
        ProjectContext {
            id: project_binding_id.unwrap_or("_unbound").to_string(),
            project_root: store.root().to_path_buf(),
            store_root: store.root().to_path_buf(),
            kind: ProjectKind::Repo,
            is_git_repo: false,
        }
    }

    /// Resolve a Project Binding as an authority-bearing registry object.
    /// Unlike `context_for`, this never fabricates an `_unbound`/path-derived
    /// compatibility context for a remotely supplied selector.
    pub(super) fn exact_project_context_for(
        &self,
        project_binding_id: Option<&str>,
        execution_space_id: &str,
    ) -> CliResult<ProjectContext> {
        let selected = project_binding_id
            .filter(|id| !id.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                self.space_context_for(execution_space_id)
                    .and_then(|space| space.default_project_binding_id)
            })
            .or_else(|| self.default_context.as_ref().map(|context| context.id.clone()))
            .ok_or_else(|| {
                CliError::Usage(
                    "an exact registered Project Binding is required for AgentFirm RoleViews and actions"
                        .to_string(),
                )
            })?;
        if let Some(default) = &self.default_context {
            if default.id == selected {
                return Ok(default.clone());
            }
        }
        if let Some(home) = &self.firm_home {
            return project::context_for_id(home, &selected)
                .map_err(project_err)?
                .ok_or_else(|| CliError::Usage(format!("unknown project binding: {selected}")));
        }
        Err(CliError::Usage(format!(
            "unknown project binding: {selected}"
        )))
    }

    pub(super) fn scoped_store_for_project(
        &self,
        store: &HarnessStore,
        execution_space_id: &str,
        project_binding_id: Option<&str>,
    ) -> CliResult<HarnessStore> {
        let project = self.exact_project_context_for(project_binding_id, execution_space_id)?;
        let store_scope = if self.default_space.is_some() {
            format!("execution-space:{execution_space_id}")
        } else {
            format!("project-store:{}", project.id)
        };
        Ok(store
            .clone()
            .with_provider_compatibility_scope(project.id, store_scope))
    }

    pub(super) fn current_space_id(&self) -> String {
        if let Some(home) = &self.firm_home {
            if let Ok(Some(id)) = execution_space::active_space_id(home) {
                return id;
            }
        }
        self.default_id.clone()
    }

    pub(super) fn current_project_binding_id(&self) -> String {
        if let Some(home) = &self.firm_home {
            if let Ok(Some(id)) = project::active_project_id(home) {
                return id;
            }
        }
        self.default_context
            .as_ref()
            .map(|context| context.id.clone())
            .unwrap_or_else(|| "_unbound".to_string())
    }

    /// Enumerate known projects for `GET /v1/projects`. In raw-override mode there is
    /// no registry, so only the served store is reported (as the synthetic default).
    pub(super) fn list_project_bindings(&self) -> Vec<ProjectContext> {
        match &self.firm_home {
            Some(home) => {
                let mut contexts = project::list_projects(home).unwrap_or_default();
                if let Some(default) = &self.default_context {
                    if !contexts.iter().any(|context| context.id == default.id) {
                        contexts.push(default.clone());
                    }
                }
                contexts
            }
            None => vec![ProjectContext {
                id: self.default_id.clone(),
                project_root: self.default_store.root().to_path_buf(),
                store_root: self.default_store.root().to_path_buf(),
                kind: ProjectKind::Repo,
                is_git_repo: false,
            }],
        }
    }

    pub(super) fn list_spaces(&self) -> Vec<ExecutionSpace> {
        match &self.firm_home {
            Some(home) => {
                let mut spaces = execution_space::list_spaces(home).unwrap_or_default();
                if let Some(default) = &self.default_space {
                    if !spaces.iter().any(|space| space.id == default.id) {
                        spaces.push(default.clone());
                    }
                }
                spaces
            }
            None => self.default_space.clone().into_iter().collect(),
        }
    }

    pub(super) fn space_context_for(&self, id: &str) -> Option<ExecutionSpace> {
        if self
            .default_space
            .as_ref()
            .is_some_and(|space| space.id == id)
        {
            return self.default_space.clone();
        }
        self.firm_home
            .as_ref()
            .and_then(|home| execution_space::context_for_id(home, id).ok().flatten())
    }

    /// Map of Execution-Space id → coordination store for SSE multiplexing.
    /// Compatibility-mode servers retain the historical per-project map.
    pub(super) fn watch_map(&self) -> std::collections::HashMap<String, PathBuf> {
        let mut map = std::collections::HashMap::new();
        map.insert(
            self.default_id.clone(),
            self.default_store.root().to_path_buf(),
        );
        if self.default_space.is_some() {
            for space in self.list_spaces() {
                map.entry(space.id).or_insert(space.store_root);
            }
        } else {
            for ctx in self.list_project_bindings() {
                map.entry(ctx.id).or_insert(ctx.store_root);
            }
        }
        map
    }
}

pub(super) fn serve_command(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    let addr = value(args, "--addr").unwrap_or_else(|| "127.0.0.1:8787".into());
    let once = has_flag(args, "--once");
    // Tests can keep the transient live turn-event tee instead of truncating it on
    // startup (per-project truncation drops in-flight events for ALL projects at
    // once — see Risks). Production serve always truncates.
    let listener = TcpListener::bind(&addr)?;
    let bound_addr = listener.local_addr()?;
    println!("serving harness API on http://{bound_addr}");
    // Show WHICH store this serve reads — the #1 confusion in issue #89 item 3 was
    // serve and run-script silently using different `.harness` dirs. Print the
    // absolute path so it can be compared against run-script's at a glance.
    let store_display = std::fs::canonicalize(store.root())
        .unwrap_or_else(|_| store.root().to_path_buf())
        .display()
        .to_string();
    println!(
        "coordination store: {store_display}  (select with --space/FIRM_SPACE; raw --store remains deprecated)"
    );

    let mut projects = ServeProjects::from_resolved(store, resolved);
    let watch_map = projects.watch_map();
    println!(
        "default execution space: {} ({} coordination store(s) watched)",
        projects.default_id,
        watch_map.len()
    );

    let sse_manager = sse::SseManager::new();

    // Prepare an exact process-memory callback capability. Registration is
    // deferred until an authorized Team/local reader selects /v1/events. The
    // callback token authenticates this serve process to the daemon; it is not
    // a user-facing or per-Agent viewing credential.
    #[cfg(unix)]
    if projects.firm_home.is_some() && bound_addr.ip().is_loopback() {
        projects.native_session_wake_callback = Some(NativeSessionWakeCallback {
            authority: bound_addr.to_string(),
            token: NATIVE_SESSION_WAKE_TOKEN
                .get_or_init(new_native_session_wake_token)
                .clone(),
            serve_instance_id: new_native_session_wake_token(),
        });
    }

    // Start one Execution-Space-multiplexed SSE watcher. The watcher re-scans the
    // registry so spaces registered after serve starts become live without a
    // restart. Each stream stays scoped to its coordination store.
    let watcher_projects = projects.clone();
    // DOC-108: no Company Store watch map remains; the second watcher map is
    // permanently empty (the SSE subscription field stays wire-compatible).
    sse::start_scoped_sse_watcher(
        move || watcher_projects.watch_map(),
        std::collections::HashMap::new,
        sse_manager.clone(),
    )
    .map_err(CliError::Io)?;

    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                // A failed accept (e.g. a client that hung up before the
                // handshake) must not take the whole server down.
                eprintln!("serve: accept failed: {error}");
                continue;
            }
        };

        if once {
            // Single-shot mode (tests): handle inline for deterministic ordering.
            if let Err(error) = handle_http_connection(&projects, stream, sse_manager.clone()) {
                eprintln!("serve: connection error: {error}");
            }
            break;
        }

        // Handle each connection on its own thread so a long-lived SSE stream
        // (/v1/events blocks for the life of the client) cannot starve other
        // requests — POST actions, snapshot polling, and additional clients
        // must still be served while a stream is open. Per-connection errors
        // (most commonly a broken pipe when a client disconnects mid-write) are
        // logged and contained to that thread instead of aborting the loop.
        let conn_projects = projects.clone();
        let conn_manager = sse_manager.clone();
        std::thread::spawn(move || {
            if let Err(error) = handle_http_connection(&conn_projects, stream, conn_manager) {
                eprintln!("serve: connection error: {error}");
            }
        });
    }
    Ok(())
}
