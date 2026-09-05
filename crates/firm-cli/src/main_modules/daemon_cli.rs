use super::*;
use std::io::{Seek as _, SeekFrom};

const STARTUP_LOG_TAIL_LINES: usize = 20;
const STARTUP_LOG_TAIL_MAX_BYTES: u64 = 64 * 1024;
const DAEMON_LOG_ROTATE_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn node_daemon_log_path(firm_home: &Path, node_id: &str) -> PathBuf {
    project::canonicalize_best_effort(firm_home)
        .join("nodes")
        .join(node_id)
        .join("node-daemon.log")
}

pub(crate) fn node_daemon_log_streams(
    firm_home: &Path,
    node_id: &str,
) -> CliResult<(PathBuf, Stdio, Stdio)> {
    let path = node_daemon_log_path(firm_home, node_id);
    let parent = path.parent().ok_or_else(|| {
        CliError::Usage(format!(
            "NodeDaemon log path has no parent: {}",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        CliError::Usage(format!(
            "cannot create NodeDaemon log directory {}: {error}",
            parent.display()
        ))
    })?;
    rotate_daemon_log_if_needed(&path).map_err(|error| {
        CliError::Usage(format!(
            "cannot rotate NodeDaemon log {}: {error}",
            path.display()
        ))
    })?;
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| {
            CliError::Usage(format!(
                "cannot open NodeDaemon log {}: {error}",
                path.display()
            ))
        })?;
    let stderr = stdout.try_clone().map_err(|error| {
        CliError::Usage(format!(
            "cannot duplicate NodeDaemon log {}: {error}",
            path.display()
        ))
    })?;
    Ok((path, Stdio::from(stdout), Stdio::from(stderr)))
}

fn rotated_daemon_log_path(path: &Path) -> PathBuf {
    let mut rotated = path.as_os_str().to_os_string();
    rotated.push(".1");
    rotated.into()
}

fn rotate_daemon_log_if_needed(path: &Path) -> std::io::Result<()> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.len() <= DAEMON_LOG_ROTATE_BYTES {
        return Ok(());
    }
    fs::rename(path, rotated_daemon_log_path(path))
}

fn read_log_tail(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(
        length.saturating_sub(STARTUP_LOG_TAIL_MAX_BYTES),
    ))?;
    let capacity = usize::try_from(length.min(STARTUP_LOG_TAIL_MAX_BYTES))
        .expect("64 KiB log tail window fits usize");
    let mut bytes = Vec::with_capacity(capacity);
    file.take(STARTUP_LOG_TAIL_MAX_BYTES)
        .read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text
        .lines()
        .rev()
        .take(STARTUP_LOG_TAIL_LINES)
        .collect::<Vec<_>>();
    lines.reverse();
    Ok(lines.join("\n"))
}

fn last_daemon_error_line(tail: &str) -> Option<&str> {
    tail.lines()
        .rev()
        .find(|line| {
            line.contains("NODE_DAEMON_")
                || line.contains("PREDECESSOR_SETTLEMENT_REQUIRED")
                || line.to_ascii_lowercase().contains("error")
        })
        .map(str::trim)
        .filter(|line| !line.is_empty())
}

pub(crate) fn daemon_status_with_log_path(response: &str, log_path: &Path) -> CliResult<String> {
    let mut status = serde_json::from_str::<serde_json::Value>(response)
        .map_err(|error| CliError::Usage(format!("invalid daemon status JSON: {error}")))?;
    let object = status
        .as_object_mut()
        .ok_or_else(|| CliError::Usage("invalid daemon status JSON: expected object".into()))?;
    object.insert(
        "log_path".into(),
        serde_json::Value::String(log_path.display().to_string()),
    );
    serde_json::to_string(&status).map_err(CliError::from)
}

pub(crate) fn daemon_start_failure(pid: u32, reason: &str, log_path: &Path) -> CliError {
    let tail =
        read_log_tail(log_path).unwrap_or_else(|error| format!("<could not read log: {error}>"));
    let tail = if tail.is_empty() {
        "<log is empty>"
    } else {
        &tail
    };
    let last_error = last_daemon_error_line(tail)
        .map(|line| format!("\nlast daemon error: {line}"))
        .unwrap_or_default();
    CliError::Usage(format!(
        "NodeDaemon pid {pid} {reason}{last_error}\nlog: {}\nlast {STARTUP_LOG_TAIL_LINES} log lines:\n{tail}",
        log_path.display()
    ))
}

fn daemon_absent_status(firm_home: &Path, node_id: &str, log_path: &Path) -> CliResult<String> {
    let mut predecessors = Vec::new();
    let mut unreadable_spaces = Vec::new();
    let now = current_unix_ms_u64();
    for space in execution_space::list_spaces(firm_home).map_err(execution_space_err)? {
        let store = HarnessStore::new(space.store_root);
        match store.latest_node_daemon_lease(node_id) {
            Ok(Some(lease)) if lease.status != NodeDaemonLeaseStatus::Released => {
                let status = match lease.status {
                    NodeDaemonLeaseStatus::Active => "active",
                    NodeDaemonLeaseStatus::Draining => "draining",
                    NodeDaemonLeaseStatus::Expired => "expired",
                    NodeDaemonLeaseStatus::Released => unreachable!(),
                };
                let expiry = if lease.expires_unix_ms <= now {
                    format!("expires unix-ms:{} (expired)", lease.expires_unix_ms)
                } else {
                    format!(
                        "expires unix-ms:{} (expires in {}s)",
                        lease.expires_unix_ms,
                        (lease.expires_unix_ms - now) / 1000
                    )
                };
                predecessors.push(format!(
                    "{}={status} generation {} (daemon {}, instance {}, {expiry})",
                    space.id, lease.generation, lease.daemon_id, lease.instance_id
                ));
            }
            Ok(_) => {}
            Err(error) => unreadable_spaces.push(format!("{} ({error})", space.id)),
        }
    }
    if predecessors.is_empty() && unreadable_spaces.is_empty() {
        Ok(format!(
            "absent (no NodeDaemon for Node {node_id}); log: {}",
            log_path.display()
        ))
    } else {
        let mut details = Vec::new();
        if !predecessors.is_empty() {
            details.push(format!(
                "unreleased predecessor NodeDaemonLease: {}",
                predecessors.join(", ")
            ));
        }
        if !unreadable_spaces.is_empty() {
            details.push(format!(
                "unreadable NodeDaemonLease stores: {}",
                unreadable_spaces.join(", ")
            ));
        }
        Ok(format!(
            "absent (no live NodeDaemon for Node {node_id}); {}; recovery action: \
             firm daemon recover-predecessor --confirm daemon-recover-predecessor; log: {}",
            details.join("; "),
            log_path.display()
        ))
    }
}

/// `daemon recover-predecessor`: settle one exact unreleased predecessor
/// NodeDaemonLease through the same validate+recover seam as the Operator
/// HTTP role action. Returns the recovery projection; the caller prints it.
fn daemon_recover_predecessor(
    firm_home: &Path,
    node_id: &str,
    args: &[String],
) -> CliResult<serde_json::Value> {
    let confirm = value(args, "--confirm").ok_or_else(|| {
        CliError::Usage(
            "daemon recover-predecessor refuses without --confirm daemon-recover-predecessor"
                .to_string(),
        )
    })?;
    if confirm != "daemon-recover-predecessor" {
        return Err(CliError::Usage(format!(
            "daemon recover-predecessor refuses --confirm {confirm:?}: the exact literal daemon-recover-predecessor is required"
        )));
    }
    let evidence_ref = value(args, "--evidence-ref")
        .unwrap_or_else(|| "cli:daemon-recover-predecessor".to_string());

    // Read this Node's leases exactly like `daemon status` does.
    let mut leases = Vec::new();
    for space in execution_space::list_spaces(firm_home).map_err(execution_space_err)? {
        let store = HarnessStore::new(space.store_root);
        if let Some(lease) = store.latest_node_daemon_lease(node_id)? {
            leases.push(lease);
        }
    }
    let Some(reference) = leases.iter().max_by_key(|lease| lease.generation).cloned() else {
        return Err(CliError::Usage(format!(
            "no predecessor NodeDaemonLease exists for Node {node_id}; nothing to recover"
        )));
    };
    if leases
        .iter()
        .all(|lease| lease.status == NodeDaemonLeaseStatus::Released)
    {
        return Ok(serde_json::json!({
            "node_id": node_id,
            "daemon_id": reference.daemon_id,
            "instance_id": reference.instance_id,
            "generation": reference.generation,
            "status": "released",
            "recovered_spaces": [],
            "space_settlements": [],
            "already_released": true,
            "evidence_ref": evidence_ref,
        }));
    }

    // Recovering inside the lease TTL is only ever a mistake or a live-daemon
    // race; name the expiry so the operator knows when recovery becomes
    // possible. The store's own refusal remains the backstop.
    let now = current_unix_ms_u64();
    if let Some(unreleased) = leases
        .iter()
        .filter(|lease| lease.status != NodeDaemonLeaseStatus::Released)
        .max_by_key(|lease| lease.generation)
    {
        if unreleased.expires_unix_ms > now {
            return Err(CliError::Usage(format!(
                "predecessor lease generation {} has not expired (expires unix-ms:{}, in {}s); retry after expiry or stop the live daemon",
                unreleased.generation,
                unreleased.expires_unix_ms,
                (unreleased.expires_unix_ms - now) / 1000
            )));
        }
    }

    let intent = validate_daemon_predecessor_recovery(firm_home, node_id, None)
        .map_err(|(code, detail)| CliError::Usage(format!("{code}: {detail}")))?;
    let actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: node_id.to_string(),
    };
    recover_daemon_predecessor_spaces(
        firm_home,
        node_id,
        &intent,
        &actor,
        true,
        &evidence_ref,
        &format!("cli-daemon-recover-predecessor:{node_id}"),
        None,
    )
    .map_err(|(code, detail)| CliError::Usage(format!("{code}: {detail}")))
}

/// Machine-scoped NodeDaemon lifecycle. Exactly one process serves the stable
/// local Node and discovers all registered Execution Spaces under FIRM_HOME.
pub(super) fn daemon_command(args: &[String]) -> CliResult<()> {
    require_subcommand(args, "daemon start|serve|status|stop|recover-predecessor")?;
    let firm_home = execution_space::firm_home().map_err(execution_space_err)?;
    let node_id = read_local_node_id()?;
    let log_path = node_daemon_log_path(&firm_home, &node_id);
    match args[0].as_str() {
        "serve" => {
            let max_concurrency = value(args, "--max-concurrency")
                .map(|raw| {
                    raw.parse::<usize>().map_err(|_| {
                        CliError::Usage("--max-concurrency must be an integer".to_string())
                    })
                })
                .transpose()?
                .unwrap_or(4);
            let idle_timeout_secs = value(args, "--idle-timeout-secs")
                .map(|raw| {
                    raw.parse::<u64>().map_err(|_| {
                        CliError::Usage("--idle-timeout-secs must be an integer".to_string())
                    })
                })
                .transpose()?
                .unwrap_or(300);
            let scan_interval_secs = value(args, "--scan-interval-secs")
                .map(|raw| {
                    raw.parse::<u64>().map_err(|_| {
                        CliError::Usage("--scan-interval-secs must be an integer".to_string())
                    })
                })
                .transpose()?
                .unwrap_or(5);
            if max_concurrency == 0 || idle_timeout_secs == 0 || scan_interval_secs == 0 {
                return Err(CliError::Usage(
                    "daemon serve concurrency and timeout/scan values must be greater than zero"
                        .to_string(),
                ));
            }
            supervisor_daemon::MultiTeamDaemon::run(
                firm_home,
                node_id,
                max_concurrency,
                idle_timeout_secs,
                scan_interval_secs,
            )?;
        }
        "start" => {
            if supervisor_daemon::daemon_status_via_socket(&firm_home, &node_id).is_some() {
                println!(
                    "NodeDaemon already running for Node {node_id}; log: {}",
                    log_path.display()
                );
                return Ok(());
            }
            let (log_path, stdout, stderr) = node_daemon_log_streams(&firm_home, &node_id)?;
            let executable = std::env::current_exe().map_err(|error| {
                CliError::Usage(format!(
                    "cannot resolve NodeDaemon executable (log: {}): {error}",
                    log_path.display()
                ))
            })?;
            let mut command = Command::new(executable);
            command
                .arg("daemon")
                .arg("serve")
                .arg("--max-concurrency")
                .arg(value(args, "--max-concurrency").unwrap_or_else(|| "4".to_string()))
                .arg("--idle-timeout-secs")
                .arg(value(args, "--idle-timeout-secs").unwrap_or_else(|| "300".to_string()))
                .arg("--scan-interval-secs")
                .arg(value(args, "--scan-interval-secs").unwrap_or_else(|| "5".to_string()))
                .stdin(Stdio::null())
                .stdout(stdout)
                .stderr(stderr);
            // `daemon start` is often invoked from a short-lived PTY (CLI,
            // Codex, CI). Put the long-lived child in its own process group so
            // closing the caller's terminal cannot SIGHUP a daemon that was
            // already reported ready.
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            let mut child = command.spawn().map_err(|error| {
                CliError::Usage(format!(
                    "cannot start NodeDaemon (log: {}): {error}",
                    log_path.display()
                ))
            })?;
            // Startup includes recovery/adoption of durable TeamRuns before
            // the socket can answer status. Ten seconds was too short for a
            // real provider resume and, worse, returned failure while leaving
            // the child to become authoritative later. A failed Start must
            // have no latent daemon effect.
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                if supervisor_daemon::daemon_status_via_socket(&firm_home, &node_id).is_some() {
                    println!(
                        "NodeDaemon started for Node {node_id} (pid {}); log: {}",
                        child.id(),
                        log_path.display()
                    );
                    break;
                }
                if let Some(status) = child.try_wait()? {
                    return Err(daemon_start_failure(
                        child.id(),
                        &format!("exited before becoming ready ({status})"),
                        &log_path,
                    ));
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(daemon_start_failure(
                        child.id(),
                        "did not become ready within 60s and was stopped",
                        &log_path,
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        "status" => match supervisor_daemon::daemon_status_via_socket(&firm_home, &node_id) {
            Some(response) => println!("{}", daemon_status_with_log_path(&response, &log_path)?),
            None => println!("{}", daemon_absent_status(&firm_home, &node_id, &log_path)?),
        },
        "recover-predecessor" => {
            let projection = daemon_recover_predecessor(&firm_home, &node_id, args)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&projection).map_err(CliError::from)?
            );
        }
        "stop" => {
            let (space_id, generation) = execution_space::list_spaces(&firm_home)
                .map_err(execution_space_err)?
                .into_iter()
                .find_map(|space| {
                    let store = HarnessStore::new(space.store_root);
                    store
                        .latest_node_daemon_lease(&node_id)
                        .ok()
                        .flatten()
                        .filter(|lease| {
                            lease.status == NodeDaemonLeaseStatus::Active
                                && lease.expires_unix_ms > current_unix_ms_u64()
                        })
                        .map(|lease| (space.id, lease.generation))
                })
                .ok_or_else(|| {
                    CliError::Usage(format!(
                        "no current NodeDaemon lease is available for Node {node_id}"
                    ))
                })?;
            let response = supervisor_daemon::daemon_stop_via_socket(
                &firm_home, &node_id, &space_id, generation,
            )
            .ok_or_else(|| {
                CliError::Usage(format!("no NodeDaemon is running for Node {node_id}"))
            })?;
            println!("{response}");
            // Stop now answers with its drain result. Printing that result and
            // exiting 0 would put the honest `NODE_DAEMON_DRAIN_INCOMPLETE`
            // behind a success exit code, which is how a still-spinning daemon
            // looked stopped in the first place (#584).
            let receipt = serde_json::from_str::<serde_json::Value>(&response)
                .map_err(|error| CliError::Usage(format!("invalid stop receipt: {error}")))?;
            if receipt["ok"] != true {
                // A refused stop (generation fence, malformed request) never
                // reached a drain: its receipt carries only {ok, error}, so
                // the partial-release wording would invent phases and Space
                // lists that do not exist. `drained` is the field that marks a
                // receipt as a drain result (DEV-149-REVIEW-04).
                if !receipt["drained"].is_boolean() {
                    return Err(CliError::Usage(format!(
                        "{}: NodeDaemon {node_id} retains machine authority; the stop had no effect",
                        receipt["error"]
                            .as_str()
                            .unwrap_or("NODE_DAEMON_STOP_REFUSED"),
                    )));
                }
                // Release continues past a per-Space failure, so a failed drain
                // does not mean nothing was released. Say "not wholly
                // released" and name the Spaces rather than asserting the
                // daemon still holds everything (DEV-149-REVIEW-03).
                let space_ids = |key: &str| {
                    receipt[key]
                        .as_array()
                        .map(|ids| {
                            ids.iter()
                                .filter_map(|id| id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .filter(|joined| !joined.is_empty())
                        .unwrap_or_else(|| "none".to_string())
                };
                return Err(CliError::Usage(format!(
                    "{}: NodeDaemon {node_id} machine authority is NOT wholly released \
                     (failed phase: {}; Execution Space leases already released: {}; \
                     release failed: {}). Read each NodeDaemonLease for certainty.",
                    receipt["error"]
                        .as_str()
                        .unwrap_or("NODE_DAEMON_DRAIN_INCOMPLETE"),
                    receipt["failed_phase"].as_str().unwrap_or("unknown"),
                    space_ids("released_execution_space_ids"),
                    space_ids("release_failed_execution_space_ids"),
                )));
            }
        }
        other => return Err(CliError::Usage(format!("unknown daemon command: {other}"))),
    }
    Ok(())
}

#[cfg(test)]
#[path = "daemon_recover_predecessor_tests.rs"]
mod daemon_recover_predecessor_tests;

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(tag: &str) -> Self {
            let unique = format!(
                "firm-daemon-cli-{tag}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("test clock")
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn log_tail_reads_a_short_file() {
        let root = TestDir::new("short-tail");
        let path = root.path().join("node-daemon.log");
        fs::write(&path, "first\nsecond\n").expect("write short log");

        assert_eq!(read_log_tail(&path).expect("read tail"), "first\nsecond");
    }

    #[test]
    fn log_tail_reads_only_the_bounded_window() {
        let root = TestDir::new("bounded-tail");
        let path = root.path().join("node-daemon.log");
        let mut contents = "outside-window"
            .repeat(usize::try_from(STARTUP_LOG_TAIL_MAX_BYTES).expect("window size") + 1);
        contents.push('\n');
        for line in 0..25 {
            contents.push_str(&format!("tail-line-{line}\n"));
        }
        fs::write(&path, contents).expect("write long log");

        let expected = (5..25)
            .map(|line| format!("tail-line-{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(read_log_tail(&path).expect("read tail"), expected);
    }

    #[test]
    fn log_tail_reads_an_empty_file() {
        let root = TestDir::new("empty-tail");
        let path = root.path().join("node-daemon.log");
        fs::write(&path, "").expect("write empty log");

        assert_eq!(read_log_tail(&path).expect("read tail"), "");
    }

    #[test]
    fn daemon_start_failure_surfaces_the_last_daemon_error_line() {
        let root = TestDir::new("start-last-error");
        let path = root.path().join("node-daemon.log");
        fs::write(
            &path,
            "[node-daemon] first diagnostic\n\
             [node-daemon] NODE_DAEMON_MACHINE_AUTHORITY_LOST: renewal failed\n\
             [node-daemon] shutdown complete\n",
        )
        .expect("write daemon failure log");

        let error = daemon_start_failure(
            4242,
            "did not become ready within 60s and was stopped",
            &path,
        );
        let rendered = error.to_string();
        assert!(rendered.contains(
            "last daemon error: [node-daemon] NODE_DAEMON_MACHINE_AUTHORITY_LOST: renewal failed"
        ));
        assert!(rendered.contains("did not become ready within 60s and was stopped"));
        assert!(rendered.contains(&format!("log: {}", path.display())));
    }

    #[test]
    fn daemon_log_streams_rotates_an_oversized_log_and_replaces_the_previous_backup() {
        let root = TestDir::new("rotation");
        let path = node_daemon_log_path(root.path(), "node-test");
        fs::create_dir_all(path.parent().expect("log parent")).expect("create log parent");
        let current = fs::File::create(&path).expect("create current log");
        current
            .set_len(DAEMON_LOG_ROTATE_BYTES + 1)
            .expect("extend current log");
        let rotated = rotated_daemon_log_path(&path);
        fs::write(&rotated, "stale backup").expect("write stale backup");

        let (_, stdout, stderr) =
            node_daemon_log_streams(root.path(), "node-test").expect("open log streams");
        drop((stdout, stderr));

        assert_eq!(fs::metadata(&path).expect("new current log").len(), 0);
        assert_eq!(
            fs::metadata(&rotated).expect("rotated log").len(),
            DAEMON_LOG_ROTATE_BYTES + 1
        );
    }
}
