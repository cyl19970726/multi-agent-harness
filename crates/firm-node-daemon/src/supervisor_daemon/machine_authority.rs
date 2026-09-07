//! Machine-scoped NodeDaemon lease ownership.
//!
//! This module is the only NodeDaemon composition module allowed to acquire,
//! renew, drain, or release the machine authority recorded in each registered
//! Execution Space. Discovery and control code may ask for an exact authority
//! check, but they do not write the lease themselves.

use super::*;

/// Which Execution Space leases this generation actually released, and which
/// it could not. Partial release is a real outcome, so the stop receipt
/// reports both lists rather than one boolean.
#[derive(Debug, Default, Clone)]
pub(super) struct AuthorityReleaseReport {
    pub(super) released_space_ids: Vec<String>,
    pub(super) failed_space_ids: Vec<String>,
}

pub(super) fn daemon_control_generation_authorized(
    lease: Option<&harness_core::NodeDaemonLease>,
    daemon_id: &str,
    instance_id: &str,
    expected_generation: u64,
    now_ms: u64,
) -> bool {
    lease.is_some_and(|lease| {
        lease.daemon_id == daemon_id
            && lease.instance_id == instance_id
            && lease.generation == expected_generation
            && lease.status == harness_core::NodeDaemonLeaseStatus::Active
            && lease.expires_unix_ms > now_ms
    })
}

pub(super) fn node_authority_refresh_interval(scan_interval: Duration) -> Duration {
    scan_interval
        .min(Duration::from_secs(5))
        .max(Duration::from_secs(1))
}

impl MultiTeamDaemon {
    fn require_machine_authority_open(&self) -> CliResult<()> {
        if self.authority_lost.load(Ordering::SeqCst) {
            return Err(CliError::Usage(
                "NODE_DAEMON_MACHINE_AUTHORITY_LOST: provider-effect admission is permanently closed for this daemon instance"
                    .into(),
            ));
        }
        Ok(())
    }

    fn latch_machine_authority_loss(&self, failures: &[String]) -> CliError {
        // Close the shared process admission gate before any durable Store IO.
        // Other already-running Team supervisors may be in different
        // Execution Spaces, but they all use this exact daemon instance and
        // must stop preparing new provider effects immediately.
        harness_store::close_process_node_daemon_admission(&self.daemon_id, &self.instance_id);
        self.authority_lost.store(true, Ordering::SeqCst);
        // Snapshot the served runs before publishing Stop. The serve loop
        // drains `contexts` as soon as it observes that flag, so reversing
        // these two operations could lose the Host-visible event targets.
        let first_loss = self.capture_machine_authority_loss(failures);
        self.stop_requested.store(true, Ordering::SeqCst);
        let mut failures = failures.to_vec();
        if let Err(error) = self.drain_node_authorities() {
            failures.push(format!("machine-wide admission drain: {error}"));
        }
        if first_loss {
            self.journal_machine_authority_loss_phase("renewal_failed", &[]);
            self.journal_machine_authority_loss_phase("lease_lost", &[]);
        }
        CliError::Usage(format!(
            "NODE_DAEMON_MACHINE_AUTHORITY_LOST: {}",
            failures.join("; ")
        ))
    }

    /// A dead socket is not sufficient evidence that the previous daemon
    /// generation lost authority. Every registered Execution Space must
    /// either have no active lease for this Node or have an expired one before
    /// the filesystem rendezvous may be reclaimed. An unreadable Store is
    /// fail-closed because it may contain the live lease we are trying not to
    /// steal.
    pub(super) fn ensure_stale_socket_reclaimable(
        firm_home: &Path,
        node_id: &str,
        application: &dyn DaemonApplicationPort,
    ) -> CliResult<()> {
        let spaces = application
            .list_execution_spaces(firm_home)
            .map_err(|error| {
                CliError::Usage(format!(
                    "NODE_DAEMON_SOCKET_RECLAIM_UNSAFE: cannot list Execution Spaces: {error}"
                ))
            })?;
        let now_ms = current_unix_ms_u64();
        for space in spaces {
            let store = HarnessStore::new(space.store_root.clone());
            let lease = store.latest_node_daemon_lease(node_id).map_err(|error| {
                CliError::Usage(format!(
                    "NODE_DAEMON_SOCKET_RECLAIM_UNSAFE: cannot verify Node {node_id} authority in Execution Space {}: {error}",
                    space.id
                ))
            })?;
            if let Some(lease) = lease {
                if lease.status == harness_core::NodeDaemonLeaseStatus::Active
                    && lease.expires_unix_ms > now_ms
                {
                    return Err(CliError::Usage(format!(
                        "NODE_DAEMON_LEASE_HELD: refusing to remove stale socket for Node {node_id}; Execution Space {} is held by {} generation {} until {}",
                        space.id,
                        lease.daemon_id,
                        lease.generation,
                        lease.expires_unix_ms
                    )));
                }
            }
        }
        Ok(())
    }

    pub(super) fn registered_spaces(
        &self,
    ) -> CliResult<Vec<(harness_core::ExecutionSpace, HarnessStore)>> {
        let spaces = self
            .application
            .list_execution_spaces(&self.firm_home)
            .map_err(|error| {
                CliError::Usage(format!(
                    "cannot list Execution Spaces for NodeDaemon: {error}"
                ))
            })?;
        Ok(spaces
            .into_iter()
            .map(|space| {
                // The held lease already owns a long-lived Store handle.
                // Reuse only its disposable read cache, never its lease as an
                // authorization decision. Root changes still get a fresh handle.
                let store = self
                    .confirmed_node_leases
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .get(&space.id)
                    .filter(|(store, _)| store.root() == space.store_root.as_path())
                    .map(|(store, _)| store.clone())
                    .unwrap_or_else(|| HarnessStore::new(space.store_root.clone()));
                (space, store)
            })
            .collect())
    }

    /// Acquire the process-local Node authority bundle before any TeamRun may
    /// admit a provider effect. Durable leases remain per Execution Space,
    /// but this daemon treats them as one all-or-nothing machine authority.
    /// Newly acquired leases are released on partial failure because no
    /// provider effect can have crossed admission before this method returns.
    pub(super) fn ensure_node_authority_bundle(&self) -> CliResult<HashSet<String>> {
        self.require_machine_authority_open()?;
        let spaces = self.registered_spaces()?;
        let mut required = Vec::new();
        for (space, store) in spaces {
            let node = store
                .latest_execution_nodes()
                .map_err(CliError::Store)?
                .into_iter()
                .find(|node| node.id == self.node_id);
            let Some(node) = node else {
                continue;
            };
            if node.status == harness_core::ExecutionNodeStatus::Retired {
                return Err(self.latch_machine_authority_loss(&[format!(
                    "{}: NODE_NOT_ACTIVE: Node {} is retired",
                    space.id, self.node_id
                )]));
            }
            let registered = store
                .latest_node_project_registrations()
                .map_err(CliError::Store)?
                .into_iter()
                .any(|registration| {
                    registration.node_id == self.node_id
                        && registration.execution_space_id == space.id
                        && registration.status
                            == harness_core::NodeProjectRegistrationStatus::Active
                });
            if registered {
                let previous = store
                    .latest_node_daemon_lease(&self.node_id)
                    .map_err(CliError::Store)?;
                let newly_acquired = previous.as_ref().is_none_or(|lease| {
                    lease.status == harness_core::NodeDaemonLeaseStatus::Released
                });
                required.push((space, store, newly_acquired));
            }
        }

        let mut acquired = Vec::new();
        for (space, store, newly_acquired) in &required {
            match self.ensure_node_authority(space, store) {
                Ok(lease) => acquired.push((space.id.clone(), store, lease, *newly_acquired)),
                Err(error) => {
                    let mut failures = vec![format!("{}: {error}", space.id)];
                    self.rollback_unused_bundle_leases(&acquired, &mut failures);
                    if matches!(
                        &error,
                        CliError::Store(
                            harness_store::StoreError::Io(_)
                                | harness_store::StoreError::LockTimeout(_)
                                | harness_store::StoreError::Json(_)
                        )
                    ) {
                        return Err(error);
                    }
                    return Err(self.latch_machine_authority_loss(&failures));
                }
            }
        }

        let now_ms = current_unix_ms_u64();
        let mut failures = Vec::new();
        for (space_id, store, expected, _) in &acquired {
            match store.latest_node_daemon_lease(&self.node_id) {
                Ok(Some(current))
                    if daemon_control_generation_authorized(
                        Some(&current),
                        &self.daemon_id,
                        &self.instance_id,
                        expected.generation,
                        now_ms,
                    ) => {}
                Ok(Some(current)) => failures.push(format!(
                    "{space_id}: final bundle revalidation observed daemon {} instance {} generation {} ({:?})",
                    current.daemon_id, current.instance_id, current.generation, current.status
                )),
                Ok(None) => failures.push(format!("{space_id}: final bundle lease is missing")),
                Err(error) => {
                    self.rollback_unused_bundle_leases(&acquired, &mut failures);
                    return Err(CliError::Store(error));
                }
            }
        }
        if !failures.is_empty() {
            self.rollback_unused_bundle_leases(&acquired, &mut failures);
            return Err(self.latch_machine_authority_loss(&failures));
        }
        Ok(acquired
            .into_iter()
            .map(|(space_id, _, _, _)| space_id)
            .collect())
    }

    fn rollback_unused_bundle_leases(
        &self,
        acquired: &[(String, &HarnessStore, harness_core::NodeDaemonLease, bool)],
        failures: &mut Vec<String>,
    ) {
        for (space_id, store, lease, newly_acquired) in acquired {
            if !newly_acquired {
                continue;
            }
            if let Err(error) = store.release_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                current_unix_ms_u64(),
            ) {
                failures.push(format!("{space_id}: bundle rollback failed: {error}"));
            }
        }
    }

    fn node_lease_ttl_ms(&self) -> u64 {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(ttl_ms) = self.lease_ttl_override_ms {
            return ttl_ms;
        }
        self.scan_interval
            .as_millis()
            .min(u64::MAX as u128)
            .try_into()
            .unwrap_or(u64::MAX)
            .saturating_mul(4)
            .max(15_000)
    }

    /// Never add a full scan interval after a slow round when a confirmed
    /// lease has less time left. This is a retry schedule, not drive authority.
    pub(super) fn next_node_authority_refresh_delay(&self) -> Duration {
        let now = current_unix_ms_u64();
        self.confirmed_node_leases
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .map(|(_, lease)| {
                Duration::from_millis((lease.expires_unix_ms.saturating_sub(now) / 4).max(1))
            })
            .min()
            .unwrap_or_else(|| node_authority_refresh_interval(self.scan_interval))
            .min(node_authority_refresh_interval(self.scan_interval))
    }

    /// Renew only authority already owned by this exact daemon instance.
    /// Discovery remains responsible for first acquisition; this heartbeat is
    /// deliberately unable to steal or create authority in an unscanned Space.
    pub(super) fn refresh_held_node_authorities(&self) -> CliResult<()> {
        self.require_machine_authority_open()?;
        // Inventory and ordinary business reads cannot delay the heartbeat.
        // Discovery records only successfully acquired exact leases here.
        let held = self
            .confirmed_node_leases
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let failures = std::thread::scope(|scope| {
            let workers = held
                .into_iter()
                .map(|(space, (store, lease))| {
                    scope.spawn(move || self.renew_held_node_lease(&space, &store, lease))
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .filter_map(|worker| match worker.join() {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error),
                    Err(_) => Some("authority refresh worker panicked".into()),
                })
                .collect::<Vec<_>>()
        });
        if failures.is_empty() || self.authority_shutdown.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(self.latch_machine_authority_loss(&failures))
        }
    }

    pub(super) fn remember_node_lease(
        &self,
        space: &str,
        store: &HarnessStore,
        lease: &harness_core::NodeDaemonLease,
    ) {
        let mut held = self
            .confirmed_node_leases
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if held.get(space).is_none_or(|(_, old)| {
            old.generation < lease.generation
                || (old.generation == lease.generation
                    && old.expires_unix_ms <= lease.expires_unix_ms)
        }) {
            held.insert(space.to_owned(), (store.clone(), lease.clone()));
        }
    }

    fn renew_held_node_lease(
        &self,
        space: &str,
        store: &HarnessStore,
        confirmed: harness_core::NodeDaemonLease,
    ) -> Result<(), String> {
        if self.authority_shutdown.load(Ordering::SeqCst) {
            return Ok(());
        }
        let expires = confirmed.expires_unix_ms;
        // Exactly one bounded attempt per Space per round. Retrying here for
        // an entire TTL would make the join starve every healthy Space.
        let started = Instant::now();
        let result = store
                .latest_node_daemon_lease(&self.node_id)
                .and_then(|current| {
                    if let Some(current) = current.as_ref() {
                        if current.daemon_id != confirmed.daemon_id
                            || current.instance_id != confirmed.instance_id
                            || current.generation != confirmed.generation {
                            return Err(harness_store::StoreError::Conflict(format!(
                                "NODE_DAEMON_GENERATION_FENCED: exact Node authority moved to daemon {} instance {} generation {} ({:?})",
                                current.daemon_id, current.instance_id, current.generation, current.status
                            )));
                        }
                    }
                    if current.as_ref().is_some_and(|lease| {
                        lease.daemon_id == confirmed.daemon_id
                            && lease.instance_id == confirmed.instance_id
                            && lease.generation == confirmed.generation
                            && matches!(
                                lease.status,
                                harness_core::NodeDaemonLeaseStatus::Draining
                                    | harness_core::NodeDaemonLeaseStatus::Released
                            )
                    }) {
                        return Ok(None);
                    }
                    store
                        .renew_node_daemon_lease(
                            &self.node_id,
                            &confirmed.daemon_id,
                            confirmed.generation,
                            &confirmed.instance_id,
                            current_unix_ms_u64(),
                            self.node_lease_ttl_ms(),
                        )
                        .map(Some)
                });
        match result {
            Ok(Some(lease)) => {
                crate::lease_renewal_diagnostics::record(
                    space,
                    "node_daemon",
                    lease.expires_unix_ms,
                    started.elapsed(),
                    None,
                );
                self.remember_node_lease(space, store, &lease);
                return Ok(());
            }
            Ok(None) => {
                let mut held = self
                    .confirmed_node_leases
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if held.get(space).is_some_and(|(_, lease)| {
                    lease.generation == confirmed.generation
                        && lease.instance_id == confirmed.instance_id
                }) {
                    held.remove(space);
                }
                return Ok(());
            }
            Err(error) => {
                if self.authority_shutdown.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let reason = error.to_string();
                crate::lease_renewal_diagnostics::record(
                    space,
                    "node_daemon",
                    expires,
                    started.elapsed(),
                    Some(&reason),
                );
                let fenced = matches!(&error, harness_store::StoreError::Conflict(message)
                        if message.starts_with("NODE_DAEMON_GENERATION_FENCED:"));
                let remaining = expires.saturating_sub(current_unix_ms_u64());
                if fenced || remaining == 0 {
                    return Err(format!(
                        "{space}: {reason}; confirmed_expires_unix_ms={expires}"
                    ));
                }
                eprintln!("[node-daemon] {space}: renewal retry within {remaining}ms: {reason}");
                // Keep the confirmed deadline unchanged. The scheduler
                // retries next round, using the shortest remaining TTL.
            }
        }
        Ok(())
    }

    /// Acquire or renew this process' parent authority in one registered
    /// Execution Space. A malformed/broken Space is isolated by the caller.
    pub(super) fn ensure_node_authority(
        &self,
        space: &harness_core::ExecutionSpace,
        store: &HarnessStore,
    ) -> CliResult<harness_core::NodeDaemonLease> {
        self.require_machine_authority_open()?;
        let node = store
            .latest_execution_nodes()?
            .into_iter()
            .find(|node| node.id == self.node_id)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "NODE_NOT_ENROLLED: Node {} is absent from Execution Space {}",
                    self.node_id, space.id
                ))
            })?;
        if node.status == harness_core::ExecutionNodeStatus::Retired {
            return Err(CliError::Usage(format!(
                "NODE_NOT_ACTIVE: Node {} is retired in Execution Space {}",
                self.node_id, space.id
            )));
        }
        let registered =
            store
                .latest_node_project_registrations()?
                .into_iter()
                .any(|registration| {
                    registration.node_id == self.node_id
                        && registration.execution_space_id == space.id
                        && registration.status
                            == harness_core::NodeProjectRegistrationStatus::Active
                });
        if !registered {
            return Err(CliError::Usage(format!(
                "NODE_HAS_NO_REGISTERED_PROJECT: Node {} has no active project in Execution Space {}",
                self.node_id, space.id
            )));
        }
        let now_ms = current_unix_ms_u64();
        let ttl_ms = self.node_lease_ttl_ms();
        if let Some(lease) = store.latest_node_daemon_lease(&self.node_id)? {
            if daemon_control_generation_authorized(
                Some(&lease),
                &self.daemon_id,
                &self.instance_id,
                lease.generation,
                now_ms,
            ) {
                self.remember_node_lease(&space.id, store, &lease);
                return Ok(lease);
            }
        }
        let lease = store
            .acquire_node_daemon_lease(
                &self.node_id,
                &self.daemon_id,
                &self.instance_id,
                now_ms,
                ttl_ms,
            )
            .map_err(CliError::Store)?;
        self.remember_node_lease(&space.id, store, &lease);
        Ok(lease)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn supersede_node_authority_for_test(&self, store: &HarnessStore) -> CliResult<()> {
        let lease = store
            .latest_node_daemon_lease(&self.node_id)?
            .ok_or_else(|| CliError::Usage("test NodeDaemon lease is missing".into()))?;
        store.release_node_daemon_lease(
            &self.node_id,
            &self.daemon_id,
            lease.generation,
            &self.instance_id,
            current_unix_ms_u64(),
        )?;
        store.acquire_node_daemon_lease(
            &self.node_id,
            "node-daemon:successor",
            "successor-instance",
            current_unix_ms_u64(),
            60_000,
        )?;
        Ok(())
    }

    /// Release this generation's lease in every registered Execution Space.
    ///
    /// This deliberately continues past a per-Space failure, so a failure is
    /// *partial*: some Space leases may already be Released while others are
    /// not. The report says which, because `authority_released: false` on a
    /// stop receipt therefore means "not wholly released", never "nothing was
    /// released" (DEV-149-REVIEW-03).
    pub(super) fn release_node_authorities(&self) -> (CliResult<()>, AuthorityReleaseReport) {
        let mut report = AuthorityReleaseReport::default();
        let mut failures = Vec::new();
        let spaces = match self.registered_spaces() {
            Ok(spaces) => spaces,
            Err(error) => return (Err(error), report),
        };
        for (space, store) in spaces {
            let lease = match store.latest_node_daemon_lease(&self.node_id) {
                Ok(Some(lease)) => lease,
                Ok(None) => continue,
                Err(error) => {
                    report.failed_space_ids.push(space.id.clone());
                    failures.push(format!("{}: {error}", space.id));
                    continue;
                }
            };
            if lease.daemon_id != self.daemon_id || lease.instance_id != self.instance_id {
                continue;
            }
            match store.release_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                current_unix_ms_u64(),
            ) {
                Ok(_) => report.released_space_ids.push(space.id.clone()),
                Err(error) => {
                    report.failed_space_ids.push(space.id.clone());
                    failures.push(format!("{}: {error}", space.id));
                }
            }
        }
        let result = if failures.is_empty() {
            Ok(())
        } else {
            Err(CliError::Usage(format!(
                "NODE_DAEMON_RELEASE_INCOMPLETE: released {} Execution Space lease(s) before failing: {}",
                report.released_space_ids.len(),
                failures.join("; ")
            )))
        };
        (result, report)
    }

    pub(super) fn settle_node_authorities_for_shutdown(&self) -> CliResult<()> {
        let mut failures = Vec::new();
        let updated_at = format!("unix-ms:{}", current_unix_ms_u64());
        for (space, store) in self.registered_spaces()? {
            let lease = match store.latest_node_daemon_lease(&self.node_id) {
                Ok(Some(lease)) => lease,
                Ok(None) => continue,
                Err(error) => {
                    failures.push(format!("{}: {error}", space.id));
                    continue;
                }
            };
            if lease.daemon_id != self.daemon_id || lease.instance_id != self.instance_id {
                continue;
            }
            let context = harness_core::agentfirm_api::MutationContext {
                execution_space_id: space.id.clone(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: self.daemon_id.clone(),
                },
                authority_actor: None,
                command_name: "node_daemon.shutdown.settle_sessions".into(),
                idempotency_key: format!(
                    "node-daemon-shutdown:{}:{}:{}",
                    self.node_id, self.daemon_id, lease.generation
                ),
                expected_version: lease.generation,
                request_fingerprint: None,
            };
            if let Err(error) = store.settle_node_daemon_shutdown_sessions(
                &context,
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                true,
                &updated_at,
            ) {
                failures.push(format!("{}: {error}", space.id));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(CliError::Usage(format!(
                "NODE_DAEMON_SHUTDOWN_SETTLEMENT_INCOMPLETE: {}",
                failures.join("; ")
            )))
        }
    }

    pub(super) fn drain_node_authorities(&self) -> CliResult<()> {
        const DRAIN_TTL_MS: u64 = 60_000;
        let mut failures = Vec::new();
        for (space, store) in self.registered_spaces()? {
            let lease = match store.latest_node_daemon_lease(&self.node_id) {
                Ok(Some(lease)) => lease,
                Ok(None) => continue,
                Err(error) => {
                    failures.push(format!("{}: {error}", space.id));
                    continue;
                }
            };
            if lease.daemon_id != self.daemon_id || lease.instance_id != self.instance_id {
                continue;
            }
            if let Err(error) = store.drain_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                current_unix_ms_u64(),
                DRAIN_TTL_MS,
            ) {
                failures.push(format!("{}: {error}", space.id));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(CliError::Usage(format!(
                "NODE_DAEMON_DRAIN_INCOMPLETE: {}",
                failures.join("; ")
            )))
        }
    }
}
