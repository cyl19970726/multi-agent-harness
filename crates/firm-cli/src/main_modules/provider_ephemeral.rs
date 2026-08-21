use super::*;

/// Spawn a one-shot `codex exec` with an EDITABLE (`--sandbox workspace-write`)
/// sandbox, JSON event stream, running in `cwd`. Non-interactive (stdin closed)
/// with a per-node timeout. When `schema_json` is set, `--output-schema <file>`
/// constrains codex's final answer to that JSON Schema. Flags verified via
/// `codex exec --help`: `--json`, `--sandbox workspace-write`, `--cd <dir>`,
/// `-m <model>`, `--skip-git-repo-check`, `--output-last-message <file>`,
/// `--output-schema <file>`.
#[allow(clippy::too_many_arguments)] // the spawn surface (session/spec/schema/cwd/model/effort/tier/timeout)
pub(super) fn spawn_codex_ephemeral(
    session_dir: &Path,
    session_id: &str,
    run_id: &str,
    spec: &workflow::AgentStepSpec,
    schema_json: Option<&serde_json::Value>,
    prompt: &str,
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    service_tier: Option<&str>,
    timeout_ms: u64,
    wall_clock_ms: Option<u64>,
) -> CliResult<EphemeralSpawn> {
    let last_message_ref = session_dir.join("last-message.md");
    // Read-only by default; a `writable` node gets FULL access (the codex analogue of
    // claude's `--permission-mode bypassPermissions`). NOT `workspace-write`: that
    // mode blocks writes to `.git/`, so a worker could edit files but `git add`/
    // `git commit` failed ("sandbox denied .git") and network was off. The caller has
    // already isolated the worker into a throwaway worktree, so the worktree (not the
    // codex sandbox) is the boundary — give it full access to actually do the work.
    let sandbox = if spec.writable {
        "danger-full-access"
    } else {
        "read-only"
    };
    let mut cmd = Command::new("codex");
    apply_workflow_child_store_guard(&mut cmd, session_dir, workflow_store_mutation_allowed());
    cmd.arg("exec")
        .arg("--cd")
        .arg(cwd)
        .arg("--sandbox")
        .arg(sandbox)
        .arg("--skip-git-repo-check")
        .arg("--json")
        .arg("--output-last-message")
        .arg(&last_message_ref);
    // Native schema enforcement: write the JSON Schema to a file and constrain the
    // final answer to it. The reply text then IS the validated JSON object.
    if let Some(schema) = schema_json {
        let schema_path = session_dir.join("output-schema.json");
        if fs::write(&schema_path, schema.to_string()).is_ok() {
            cmd.arg("--output-schema").arg(&schema_path);
        }
    }
    apply_codex_ephemeral_model_effort_service_tier_args(&mut cmd, model, effort, service_tier);
    // codex has no fallback-model flag; only providers with a native flag use it.
    for path in &spec.image {
        cmd.arg("-i").arg(path);
    }
    for path in &spec.add_dir {
        cmd.arg("--add-dir").arg(path);
    }
    // `-i/--image <FILE>...` is VARIADIC: a positional prompt placed after it is
    // swallowed as another image path, so codex finds no PROMPT positional, reads
    // an empty stdin, and dies with "No prompt provided via stdin." Terminate
    // option parsing with `--` so the prompt is unambiguously the PROMPT positional.
    if !spec.image.is_empty() {
        cmd.arg("--");
    }
    cmd.arg(prompt);

    let run = run_ndjson_child(
        cmd,
        session_dir,
        session_id,
        "codex.stream-json.ndjson",
        timeout_ms,
        wall_clock_ms,
        Some(OrphanRegistration {
            dir: session_dir
                .parent()
                .and_then(|delivery_dir| delivery_dir.parent())
                .unwrap_or(session_dir)
                .join("worker_pids"),
            run_id: run_id.to_string(),
            cmd_marker: "codex".to_string(),
        }),
        "ephemeral worker",
    )?;
    let codex_events: Vec<CodexExecEvent> = run
        .events
        .iter()
        .filter_map(|v| serde_json::to_string(v).ok())
        .filter_map(|line| CodexExecEvent::parse_line(&line))
        .collect();
    let ok = matches!(
        infer_provider_execution_status(&codex_events, run.process_success),
        ProviderExecutionStatus::Succeeded
    );
    // Prefer the parsed agent message; fall back to the last-message file codex
    // wrote (the terminal assistant text).
    let reply = extract_codex_reply_text(&codex_events)
        .or_else(|| fs::read_to_string(&last_message_ref).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let tokens = parse_codex_usage(&run.events);
    // With `--output-schema`, the constrained answer is the turn's FINAL message.
    // Parse structured output from that final message — the `--output-last-message`
    // file first, then the last `agent_message` — NOT the joined narration, so a
    // streamed preamble ("I'll start by inspecting…") can't be captured as the
    // result (issue #139 item 2). Fall back to the joined reply only as a last resort.
    let structured = schema_json.and_then(|_| {
        fs::read_to_string(&last_message_ref)
            .ok()
            .as_deref()
            .and_then(extract_json_object)
            .or_else(|| {
                extract_codex_final_message(&codex_events)
                    .as_deref()
                    .and_then(extract_json_object)
            })
            .or_else(|| reply.as_deref().and_then(extract_json_object))
    });

    Ok(EphemeralSpawn {
        ok,
        reply,
        native_session: extract_thread_id_from_exec_events(&codex_events)
            .map(|id| provider_native_session_ref("codex", id)),
        stderr: run.stderr,
        exit_code: run.exit_code,
        timed_out: run.timed_out,
        wall_timed_out: run.wall_timed_out,
        tokens,
        // codex exec --json carries no model; only spec.model is known.
        model: None,
        structured,
        // codex emits token usage but no dollar cost.
        cost_usd: None,
        warnings: run.warnings,
    })
}

/// Spawn a one-shot `claude -p` with EDITING allowed: `--output-format
/// stream-json --verbose`, an allowedTools set incl. Read/Edit/Write/Bash, and a
/// non-blocking `--permission-mode bypassPermissions` so it never blocks on an
/// approval prompt. When `schema_json` is set, `--json-schema <inline>` makes
/// claude emit a schema-validated `result.structured_output`. Runs with cwd =
/// `cwd` (the harness owns isolation; we do NOT use claude's -w). Flags verified
/// via `claude --help`.
#[allow(clippy::too_many_arguments)] // the spawn surface (session/spec/schema/cwd/timeout/budget)
pub(super) fn spawn_claude_ephemeral(
    session_dir: &Path,
    session_id: &str,
    run_id: &str,
    spec: &workflow::AgentStepSpec,
    schema_json: Option<&serde_json::Value>,
    prompt: &str,
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    timeout_ms: u64,
    wall_clock_ms: Option<u64>,
    max_budget_usd: Option<f64>,
) -> CliResult<EphemeralSpawn> {
    let prompt_with_images;
    let prompt = if spec.image.is_empty() {
        prompt
    } else {
        prompt_with_images = format!(
            "Attached image files (read them with the Read tool): {}\n\n{}",
            spec.image.join(", "),
            prompt
        );
        &prompt_with_images
    };
    // Read-only by default (no Edit/Write/Bash); a `writable` node gets the editing
    // tools (and the caller has isolated it into a throwaway worktree). The tool
    // allowlist is the gate; bypassPermissions only keeps -p non-interactive.
    let tools = if spec.writable {
        "Read,Edit,Write,Bash,Grep,Glob"
    } else {
        "Read,Grep,Glob"
    };
    let mut cmd = Command::new("claude");
    apply_workflow_child_store_guard(&mut cmd, session_dir, workflow_store_mutation_allowed());
    cmd.arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--permission-mode")
        .arg("bypassPermissions")
        .arg("--allowedTools")
        .arg(tools)
        .current_dir(cwd);
    // Per-worker spend backstop: bound a single worker to the run's ceiling so it
    // can't blow the budget between the program's barrier-granular tally checks.
    // (Soft: claude's --max-budget-usd is a post-turn cap that can overshoot a
    // little, but it bounds the runaway-single-worker case the tally can miss.)
    if let Some(budget) = max_budget_usd {
        if budget > 0.0 {
            cmd.arg("--max-budget-usd").arg(format!("{budget}"));
        }
    }
    // Native schema enforcement via constrained decoding: the validated object is
    // emitted on the terminal `result` event as `structured_output`.
    if let Some(schema) = schema_json {
        cmd.arg("--json-schema").arg(schema.to_string());
    }
    if let Some(model) = model {
        cmd.arg("--model").arg(model);
    }
    // Reasoning effort: claude has a native session flag.
    if let Some(effort) = effort {
        cmd.arg("--effort").arg(effort);
    }
    if let Some(model) = &spec.fallback_model {
        cmd.arg("--fallback-model").arg(model);
    }
    for path in &spec.add_dir {
        cmd.arg("--add-dir").arg(path);
    }

    let run = run_ndjson_child(
        cmd,
        session_dir,
        session_id,
        "claude.stream-json.ndjson",
        timeout_ms,
        wall_clock_ms,
        Some(OrphanRegistration {
            dir: session_dir
                .parent()
                .and_then(|delivery_dir| delivery_dir.parent())
                .unwrap_or(session_dir)
                .join("worker_pids"),
            run_id: run_id.to_string(),
            cmd_marker: "claude".to_string(),
        }),
        "ephemeral worker",
    )?;
    let claude_events: Vec<ClaudeStreamEvent> = run
        .events
        .iter()
        .filter_map(|v| serde_json::to_string(v).ok())
        .filter_map(|line| ClaudeStreamEvent::parse_line(&line))
        .collect();
    let ok = matches!(
        infer_claude_session_status(&claude_events, run.process_success),
        ProviderExecutionStatus::Succeeded
    );
    let reply = extract_claude_reply_text(&claude_events);
    let tokens = parse_claude_usage(&run.events);
    let model = parse_worker_model(&run.events);
    // `structured_output` (when `--json-schema` ran) + the billed turn cost, both
    // off the terminal `result` frame.
    let (structured, cost_usd) = parse_claude_result_extras(&run.events);

    Ok(EphemeralSpawn {
        ok,
        reply,
        native_session: extract_session_id_from_claude_events(&claude_events)
            .map(|id| provider_native_session_ref("claude", id)),
        stderr: run.stderr,
        exit_code: run.exit_code,
        timed_out: run.timed_out,
        wall_timed_out: run.wall_timed_out,
        tokens,
        model,
        structured,
        cost_usd,
        warnings: run.warnings,
    })
}

/// The terminal state of one NDJSON child process: whether it exited 0, its raw
/// exit code (None when killed on timeout / signalled), whether the per-node
/// timeout fired, the parsed event payloads, and any stderr.
pub(super) struct NdjsonRun {
    pub(super) process_success: bool,
    /// Process exit code when the child exited on its own; `None` when it was
    /// killed on timeout or terminated by a signal (no code available).
    pub(super) exit_code: Option<i32>,
    /// True when the per-node timeout fired and we killed the child.
    pub(super) timed_out: bool,
    /// True when the per-leaf wall-clock timeout fired.
    pub(super) wall_timed_out: bool,
    pub(super) events: Vec<serde_json::Value>,
    pub(super) stderr: String,
    pub(super) warnings: Vec<String>,
}

/// Spawn a child that emits NDJSON on stdout, non-interactively (stdin closed).
/// Events are reduced in memory only; the provider-owned native session remains
/// the sole transcript/tool stream. Enforces a per-node timeout: on
/// timeout the child is killed and `process_success=false` (the run tolerates
/// failed nodes). Returns the terminal [`NdjsonRun`].
/// SIGKILL the worker's whole process GROUP (the child is the group leader, so
/// its pid is the pgid; `kill -9 -<pgid>`). codex/claude spawn child binaries
/// that inherit our stdout pipe — killing only the immediate child would leave a
/// grandchild holding the pipe open and the reader thread (and its join) blocked
/// forever. Falls back to killing the immediate child.
pub(super) fn kill_worker_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    {
        // SIGKILL the whole process GROUP (negative pid == the group). The child is
        // its own group leader (`process_group(0)`), so its pid IS the pgid; a
        // grandchild (codex/claude spawn a child binary; or a test's `sleep`)
        // inherits the group, so this reaps the tree and closes the inherited
        // stdout pipe — which is what lets the reader thread's join return.
        //
        // We call `kill(2)` directly rather than shelling out to `kill -9 -<pgid>`:
        // the external `kill` parses a leading-dash pgid INCONSISTENTLY across
        // platforms (BSD/macOS accept it; util-linux on CI swallowed it as options),
        // which left the grandchild alive and hung the reader for the full 600s.
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Clone, Debug)]
pub(super) struct OrphanRegistration {
    pub(super) dir: PathBuf,
    pub(super) run_id: String,
    pub(super) cmd_marker: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(super) struct OrphanPidfile {
    pub(super) run_id: String,
    pub(super) pid: u32,
    pub(super) pgid: u32,
    pub(super) cmd_marker: String,
    pub(super) started_ms: u128,
}

pub(super) struct OrphanPidfileGuard {
    pub(super) path: PathBuf,
}

impl OrphanPidfileGuard {
    pub(super) fn create(reg: OrphanRegistration, pid: u32) -> CliResult<Self> {
        fs::create_dir_all(&reg.dir)?;
        let path = reg.dir.join(format!("{}__{}.json", reg.run_id, pid));
        let entry = OrphanPidfile {
            run_id: reg.run_id,
            pid,
            // `process_group(0)` makes the child its own group leader, so pid == pgid.
            pgid: pid,
            cmd_marker: reg.cmd_marker,
            started_ms: current_unix_ms(),
        };
        fs::write(&path, serde_json::to_vec(&entry)?)?;
        Ok(Self { path })
    }
}

impl Drop for OrphanPidfileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[allow(clippy::too_many_arguments)] // shared process runner surface plus optional orphan registration
pub(super) fn run_ndjson_child(
    mut cmd: Command,
    session_dir: &Path,
    session_id: &str,
    live_file_name: &str,
    timeout_ms: u64,
    wall_clock_ms: Option<u64>,
    orphan_reg: Option<OrphanRegistration>,
    // Human label for this worker in spawn/timeout error + warning strings
    // (e.g. "ephemeral worker", "codex exec", "claude -p"). The persistent member
    // path passes its provider-specific label so failure summaries read the same
    // as before this runner was shared.
    context: &str,
) -> CliResult<NdjsonRun> {
    // Put the worker in its OWN process group so a timeout can kill the whole
    // tree (see kill_worker_tree), not just the immediate child.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CliError::Usage(format!("failed to spawn {context}: {error}")))?;
    let _orphan_guard = if let Some(reg) = orphan_reg {
        match OrphanPidfileGuard::create(reg, child.id()) {
            Ok(guard) => Some(guard),
            Err(error) => {
                kill_worker_tree(&mut child);
                return Err(error);
            }
        }
    } else {
        None
    };

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::Usage(format!("{context} stdout not available")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CliError::Usage(format!("{context} stderr not available")))?;

    let _ = (session_dir, session_id, live_file_name);

    // IDLE-timeout clock. A productive worker keeps emitting events, each resetting
    // this to "now"; the main thread kills only a worker that has gone SILENT for
    // `timeout_ms` (a wedged provider / auth or network stall) — never a slow but
    // still-streaming turn. Stored as millis-since-`start`.
    let start = Instant::now();
    let last_activity_ms = Arc::new(AtomicU64::new(0));
    let activity_ms = Arc::clone(&last_activity_ms);
    let activity_start = start;

    // Read stdout in a DEDICATED THREAD so the main thread can enforce the idle
    // timeout by KILLING a worker that stops emitting events but never closes stdout
    // (an auth/network stall, a wedged provider). The old code read stdout on the
    // main thread and only checked the deadline AFTER the read loop returned, so a
    // hung worker (stdout still open) blocked forever and froze the whole run. The
    // thread tees each event live + collects them; killing the child closes stdout,
    // which ends this loop.
    let stdout_handle = std::thread::spawn(move || {
        let mut warnings = Vec::new();
        let mut events = Vec::new();
        let mut dropped_lines = 0usize;
        for line in BufReader::new(stdout).lines() {
            let Ok(line_str) = line else { continue };
            let trimmed = line_str.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Any non-empty output proves the worker is alive — reset the idle clock.
            activity_ms.store(
                activity_start.elapsed().as_millis() as u64,
                Ordering::Relaxed,
            );
            let Ok(payload) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                dropped_lines += 1;
                continue;
            };
            events.push(payload);
        }
        if dropped_lines > 0 {
            warnings.push(format!(
                "{dropped_lines} stdout line(s) were not valid JSON and were dropped"
            ));
        }
        (events, warnings)
    });

    // Drain stderr in its own thread so a chatty worker cannot fill the pipe and
    // block (which would also stall the kill path).
    let stderr_handle = std::thread::spawn(move || {
        let mut log = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut log);
        log
    });

    // Main thread: enforce the IDLE timeout. While the worker keeps streaming events
    // the idle clock resets, so a slow-but-productive turn runs to completion however
    // long it takes; only a worker SILENT for `timeout_ms` (a wedged provider, an
    // auth/network stall) is killed. Killing closes stdout/stderr so the reader
    // threads finish and join cleanly.
    let idle_limit = Duration::from_millis(timeout_ms.max(1));
    let wall_clock_limit = wall_clock_ms.map(|ms| Duration::from_millis(ms.max(1)));
    let mut timed_out = false;
    let mut wall_timed_out = false;
    let mut exit_code: Option<i32> = None;
    let process_success = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_code = status.code();
                break status.success();
            }
            Ok(None) => {
                if let Some(wall) = wall_clock_limit {
                    if start.elapsed() > wall {
                        kill_worker_tree(&mut child);
                        wall_timed_out = true;
                        break false;
                    }
                }
                let last = Duration::from_millis(last_activity_ms.load(Ordering::Relaxed));
                if start.elapsed().saturating_sub(last) > idle_limit {
                    kill_worker_tree(&mut child);
                    timed_out = true;
                    break false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break false,
        }
    };

    let (events, mut warnings) = stdout_handle.join().unwrap_or_default();
    let mut stderr_log = stderr_handle.join().unwrap_or_default();
    if timed_out && stderr_log.is_empty() {
        stderr_log = format!("timeout waiting for {context}");
    }
    if wall_timed_out && stderr_log.is_empty() {
        let wall_s = wall_clock_ms.unwrap_or(0).div_ceil(1_000);
        stderr_log = format!("{context} exceeded per-leaf wall-clock timeout of {wall_s}s");
    }
    if timed_out {
        warnings.push(format!("{context} timed out"));
    }
    if wall_timed_out {
        let wall_s = wall_clock_ms.unwrap_or(0).div_ceil(1_000);
        warnings.push(format!(
            "{context} exceeded per-leaf wall-clock timeout of {wall_s}s"
        ));
    }

    Ok(NdjsonRun {
        process_success,
        exit_code,
        timed_out: timed_out || wall_timed_out,
        wall_timed_out,
        events,
        stderr: stderr_log,
        warnings,
    })
}

/// `git -C <wt> diff --binary` — the node's collected evidence for the isolation
/// path. Returns None when git is unavailable; an empty string means a clean tree.
///
/// We first `git add -A --intent-to-add` so brand-new UNTRACKED files a worker
/// creates show up in the diff as additions (plain `git diff` omits untracked
/// content). The worktree is throwaway, so touching its index is harmless.
///
/// D5 (binary-safe capture): `--binary` embeds a `GIT binary patch` block for any
/// changed binary file instead of collapsing it to a "Binary files differ" stub.
/// The throwaway worktree is deleted right after capture, so a stub would lose the
/// content irrecoverably AND poison the whole patch at `git apply --check`; the
/// binary block is git-encoded ASCII, so it round-trips through the stored diff.
pub(super) fn ephemeral_worktree_diff(worktree: &Path) -> Option<String> {
    let wt = worktree.display().to_string();
    // Best-effort intent-to-add so untracked files are included; ignore failure.
    let _ = Command::new("git")
        .args(["-C", &wt, "add", "-A", "--intent-to-add"])
        .output();
    let output = Command::new("git")
        .args(["-C", &wt, "diff", "--binary"])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Enumerate the paths a worktree's uncommitted work touches, robustly (D4a).
/// Uses `git diff --name-status -z -M`: the `-z` NUL-delimited form emits raw
/// (un-c-quoted) UTF-8 path bytes, so a CJK / spaced / crafted filename can't
/// desync a whitespace split (the old `diff --git` header parse's failure mode).
/// A rename record (`R<score>\0old\0new`) contributes BOTH sides; adds / mods /
/// deletes (`A|M|D\0path`) contribute their single path. Returns None only when
/// git is unavailable. Assumes the caller already staged intent-to-add (as
/// [`ephemeral_worktree_diff`] does) so untracked files are enumerated too.
pub(super) fn ephemeral_worktree_changed_paths(worktree: &Path) -> Option<Vec<String>> {
    let wt = worktree.display().to_string();
    let output = Command::new("git")
        .args(["-C", &wt, "diff", "--name-status", "-z", "-M", "HEAD"])
        .output()
        .ok()?;
    Some(parse_name_status_z(&output.stdout))
}

/// Parse `git diff --name-status -z` output into the set of changed paths. Each
/// record is a status field followed by 1 path (`A`/`M`/`D`/`T`/...) or 2 paths
/// (`R`/`C` renames/copies — both `old` and `new` are recorded), all NUL-
/// separated. Paths are raw UTF-8 (the `-z` form never c-quotes), decoded lossily.
pub(super) fn parse_name_status_z(bytes: &[u8]) -> Vec<String> {
    let mut fields = bytes
        .split(|b| *b == 0)
        .filter(|f| !f.is_empty())
        .map(|f| String::from_utf8_lossy(f).to_string());
    let mut paths = BTreeSet::new();
    while let Some(status) = fields.next() {
        // A rename/copy status is `R<score>` / `C<score>` and carries two path
        // fields (old, new); every other status carries exactly one.
        let takes_two = status.starts_with('R') || status.starts_with('C');
        let Some(first) = fields.next() else { break };
        if takes_two {
            let Some(second) = fields.next() else {
                if !first.is_empty() && first != "/dev/null" {
                    paths.insert(first);
                }
                break;
            };
            for p in [first, second] {
                if !p.is_empty() && p != "/dev/null" {
                    paths.insert(p);
                }
            }
        } else if !first.is_empty() && first != "/dev/null" {
            paths.insert(first);
        }
    }
    paths.into_iter().collect()
}

/// Parse `git apply --numstat -z <patch>` output into the set of paths git would
/// actually touch when applying the patch (D4b). This parses the patch EXACTLY as
/// git will apply it, closing the crafted-`diff --git`-header bypass (a header can
/// name a path the hunk never touches) and the c-quoted-CJK false Conflict (the
/// `-z` form emits raw UTF-8, so no `"\346..."` to mis-decode). Each record is
/// `added\tdeleted` followed by one path (adds/mods/deletes, and — since git apply
/// resolves renames to the destination — detected renames) OR two paths
/// (`old\0new`) for an unresolved rename; all NUL-separated. Errors carry git's
/// stderr so an unparsable patch fails closed at the call site.
pub(super) fn git_apply_numstat_paths(repo_root: &Path, patch: &[u8]) -> CliResult<Vec<String>> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["apply", "--numstat", "-z", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(patch)?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(CliError::Usage(format!(
            "git apply --numstat failed (patch is not applyable as written): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(parse_numstat_z(&out.stdout))
}

/// Parse `git apply --numstat -z` output into its changed paths. Each record is
/// `added\tdeleted` (two tab-separated count fields, `-` for binary) then one path
/// field, except an unresolved rename which appends a second path field. Paths are
/// raw UTF-8 (the `-z` form never c-quotes), decoded lossily.
pub(super) fn parse_numstat_z(bytes: &[u8]) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for record in bytes.split(|b| *b == 0).filter(|f| !f.is_empty()) {
        let text = String::from_utf8_lossy(record);
        // A record is `<added>\t<deleted>\t<path>`; a leading count block means
        // this field carries the numstat header + path. A bare field (no tab) is
        // the SECOND path of an unresolved rename emitted as its own NUL record.
        if let Some((_counts, path)) = text.rsplit_once('\t') {
            let path = path.trim();
            if !path.is_empty() && path != "/dev/null" {
                paths.insert(path.to_string());
            }
        } else {
            let path = text.trim();
            if !path.is_empty() && path != "/dev/null" {
                paths.insert(path.to_string());
            }
        }
    }
    paths.into_iter().collect()
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ExpectedArtifactOutcome {
    pub(super) copied: Vec<String>,
    pub(super) failures: Vec<String>,
}

pub(super) fn collect_expected_artifacts(
    worker_cwd: &Path,
    repo_root: &Path,
    expected_artifacts: &[String],
) -> ExpectedArtifactOutcome {
    let mut outcome = ExpectedArtifactOutcome::default();
    for artifact in expected_artifacts {
        let artifact = artifact.trim();
        if artifact.is_empty() {
            outcome.failures.push("empty artifact path".to_string());
            continue;
        }
        let rel = Path::new(artifact);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            outcome.failures.push(format!(
                "{artifact}: expected_artifacts entries must be repo-relative paths"
            ));
            continue;
        }
        let src = worker_cwd.join(rel);
        let metadata = match fs::metadata(&src) {
            Ok(metadata) => metadata,
            Err(_) => {
                outcome.push_missing(artifact);
                continue;
            }
        };
        if !metadata.is_file() {
            outcome
                .failures
                .push(format!("{artifact}: exists but is not a file"));
            continue;
        }
        if metadata.len() == 0 {
            outcome.push_missing(artifact);
            continue;
        }
        let dest = repo_root.join(rel);
        if let Some(parent) = dest.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                outcome
                    .failures
                    .push(format!("{artifact}: could not create destination: {err}"));
                continue;
            }
        }
        let same_path = fs::canonicalize(&src)
            .ok()
            .zip(fs::canonicalize(&dest).ok())
            .is_some_and(|(src, dest)| src == dest);
        if !same_path {
            if let Err(err) = fs::copy(&src, &dest) {
                outcome
                    .failures
                    .push(format!("{artifact}: could not copy to live repo: {err}"));
                continue;
            }
        }
        outcome.copied.push(artifact.to_string());
    }
    outcome
}

impl ExpectedArtifactOutcome {
    pub(super) fn push_missing(&mut self, artifact: &str) {
        self.failures.push(format!(
            "{artifact}: missing or empty; declare only artifacts the step writes, or write a non-empty file before the step exits"
        ));
    }
}

pub(super) fn step_ok_after_gates(
    provider_ok: bool,
    schema_failed: bool,
    artifact_outcome: &ExpectedArtifactOutcome,
) -> bool {
    provider_ok && !schema_failed && artifact_outcome.failures.is_empty()
}

pub(super) fn count_unique_worktree_diff_files(diff: &str) -> usize {
    diff.lines()
        .filter_map(|line| line.strip_prefix("diff --git "))
        .filter(|header| !header.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .len()
}

/// `workflow get-output <run_id> [--step <label>]` — retrieve a run's leaf
/// durable WorkflowStep outcomes in authored order. The generic workflow
/// command does not join private provider-native Session history.
pub(super) fn workflow_get_output_value(
    store: &HarnessStore,
    args: &[String],
) -> CliResult<serde_json::Value> {
    let run_id = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .ok_or_else(|| CliError::Usage("workflow get-output requires a <run_id>".into()))?;
    let step_filter = value(args, "--step");

    let run = store
        .workflow_runs()?
        .into_iter()
        .rfind(|r| r.id == run_id)
        .ok_or_else(|| CliError::Usage(format!("workflow run not found: {run_id}")))?;

    // Latest-wins projection of this run's steps, then order by run.step_ids so the
    // output reads in workflow order (fall back to journal order if step_ids empty).
    let mut by_id: std::collections::HashMap<String, WorkflowStep> =
        std::collections::HashMap::new();
    let mut journal_order: Vec<String> = Vec::new();
    for step in store.workflow_steps()? {
        if step.run_id == run_id {
            if !by_id.contains_key(&step.id) {
                journal_order.push(step.id.clone());
            }
            by_id.insert(step.id.clone(), step);
        }
    }
    let order: Vec<String> = if run.step_ids.is_empty() {
        journal_order
    } else {
        run.step_ids.clone()
    };

    let mut out_steps = Vec::new();
    for id in order {
        let Some(step) = by_id.get(&id) else { continue };
        if let Some(filter) = &step_filter {
            if &step.label != filter {
                continue;
            }
        }
        let output = step.output_summary.clone().unwrap_or_default();
        out_steps.push(serde_json::json!({
            "label": step.label,
            "status": serde_json::to_value(step.status)?,
            "native_session": step.native_session,
            "source": "workflow_step",
            "result": step.result,
            "provider_native_history": "not_exposed_by_workflow_output",
            "output": output,
        }));
    }

    if let Some(filter) = &step_filter {
        if out_steps.is_empty() {
            return Err(CliError::Usage(format!(
                "no step labeled '{filter}' in run {run_id}"
            )));
        }
    }

    Ok(serde_json::json!({
        "run_id": run_id,
        "workflow_name": run.workflow_name,
        "steps": out_steps,
    }))
}

pub(super) fn workflow_gc_worktrees(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
) -> CliResult<serde_json::Value> {
    // Worktrees live under the PROJECT ROOT (not the centralized store, not the
    // harness process cwd), so GC them there too (goal-multi-project P4). The git
    // commands tolerate a missing/moved project_root by failing soft (empty output).
    let repo_root = workflow_repo_root(
        &project_context
            .cloned()
            .unwrap_or_else(|| workflow_project_context(store)),
    );
    let repo = repo_root.display().to_string();

    // Prune dangling administrative entries first.
    let _ = Command::new("git")
        .args(["-C", &repo, "worktree", "prune"])
        .output();

    let runs_by_id: BTreeMap<String, WorkflowRunStatus> =
        latest_workflow_runs_in_append_order(store)?
            .into_iter()
            .map(|run| (run.id, run.status))
            .collect();
    let mut run_ids_by_len: Vec<&str> = runs_by_id.keys().map(String::as_str).collect();
    run_ids_by_len.sort_by_key(|id| std::cmp::Reverse(id.len()));

    // Registered worktree paths. A registered path is preserved only while its
    // owning WorkflowRun is still Running; terminal or missing owners are stale
    // after the serve reaper has finalized abandoned runs.
    let listed = Command::new("git")
        .args(["-C", &repo, "worktree", "list", "--porcelain"])
        .output()?;
    let listed_text = String::from_utf8_lossy(&listed.stdout);
    let registered: BTreeSet<PathBuf> = listed_text
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|p| PathBuf::from(p.trim()))
        .collect();

    let worktrees_dir = repo_root.join(".harness").join("worktrees");
    let mut removed = Vec::new();
    if let Ok(entries) = fs::read_dir(&worktrees_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // Compare against the canonicalized registered set when possible.
            let is_registered = registered
                .iter()
                .any(|reg| reg == &path || reg.canonicalize().ok() == path.canonicalize().ok());
            let owner_status = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| {
                    run_ids_by_len
                        .iter()
                        .find(|run_id| name == **run_id || name.starts_with(&format!("{run_id}-")))
                        .and_then(|run_id| runs_by_id.get(*run_id).copied())
                });
            if is_registered && owner_status == Some(WorkflowRunStatus::Running) {
                continue;
            }
            let _ = Command::new("git")
                .args(["-C", &repo, "worktree", "remove", "--force"])
                .arg(&path)
                .output();
            let _ = fs::remove_dir_all(&path);
            removed.push(path.display().to_string());
        }
    }
    let _ = Command::new("git")
        .args(["-C", &repo, "worktree", "prune"])
        .output();

    // Touch the store so the GC arm has a uniform signature with the rest.
    let _ = store.root();

    Ok(serde_json::json!({
        "ok": true,
        "removed": removed,
        "worktrees_dir": worktrees_dir.display().to_string(),
    }))
}

/// Parse a `unix-ms:<millis>` timestamp string into millis; 0 if unparseable.
pub(super) fn created_ms(created_at: &str) -> u128 {
    created_at
        .strip_prefix("unix-ms:")
        .and_then(|n| n.parse::<u128>().ok())
        .unwrap_or(0)
}
