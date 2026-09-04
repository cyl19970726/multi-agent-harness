use super::*;

const STARTUP_LOG_TAIL_LINES: usize = 20;

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
    let tail = fs::read(log_path)
        .map(|bytes| {
            let text = String::from_utf8_lossy(&bytes);
            let mut lines = text
                .lines()
                .rev()
                .take(STARTUP_LOG_TAIL_LINES)
                .collect::<Vec<_>>();
            lines.reverse();
            lines.join("\n")
        })
        .unwrap_or_else(|error| format!("<could not read log: {error}>"));
    let tail = if tail.is_empty() {
        "<log is empty>"
    } else {
        &tail
    };
    CliError::Usage(format!(
        "NodeDaemon pid {pid} {reason}\nlog: {}\nlast {STARTUP_LOG_TAIL_LINES} log lines:\n{tail}",
        log_path.display()
    ))
}

fn daemon_absent_status(firm_home: &Path, node_id: &str, log_path: &Path) -> CliResult<String> {
    let mut predecessors = Vec::new();
    for space in execution_space::list_spaces(firm_home).map_err(execution_space_err)? {
        let store = HarnessStore::new(space.store_root);
        if let Some(lease) = store.latest_node_daemon_lease(node_id)? {
            if lease.status != NodeDaemonLeaseStatus::Released {
                let status = match lease.status {
                    NodeDaemonLeaseStatus::Active => "active",
                    NodeDaemonLeaseStatus::Draining => "draining",
                    NodeDaemonLeaseStatus::Expired => "expired",
                    NodeDaemonLeaseStatus::Released => unreachable!(),
                };
                predecessors.push(format!(
                    "{}={status} generation {} (daemon {}, instance {})",
                    space.id, lease.generation, lease.daemon_id, lease.instance_id
                ));
            }
        }
    }
    if predecessors.is_empty() {
        Ok(format!(
            "absent (no NodeDaemon for Node {node_id}); log: {}",
            log_path.display()
        ))
    } else {
        Ok(format!(
            "absent (no live NodeDaemon for Node {node_id}); unreleased predecessor \
             NodeDaemonLease: {}; recovery action: daemon-recover-predecessor; log: {}",
            predecessors.join(", "),
            log_path.display()
        ))
    }
}

/// Machine-scoped NodeDaemon lifecycle. Exactly one process serves the stable
/// local Node and discovers all registered Execution Spaces under FIRM_HOME.
pub(super) fn daemon_command(args: &[String]) -> CliResult<()> {
    require_subcommand(args, "daemon start|serve|status|stop")?;
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
