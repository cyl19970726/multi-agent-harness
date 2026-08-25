//! Machine-scoped NodeDaemon lease ownership.
//!
//! This module is the only NodeDaemon composition module allowed to acquire,
//! renew, drain, or release the machine authority recorded in each registered
//! Execution Space. Discovery and control code may ask for an exact authority
//! check, but they do not write the lease themselves.

use super::*;

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
    /// A dead socket is not sufficient evidence that the previous daemon
    /// generation lost authority. Every registered Execution Space must
    /// either have no active lease for this Node or have an expired one before
    /// the filesystem rendezvous may be reclaimed. An unreadable Store is
    /// fail-closed because it may contain the live lease we are trying not to
    /// steal.
    pub(super) fn ensure_stale_socket_reclaimable(
        firm_home: &Path,
        node_id: &str,
    ) -> CliResult<()> {
        let spaces = crate::execution_space::list_spaces(firm_home).map_err(|error| {
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
        let spaces = crate::execution_space::list_spaces(&self.firm_home).map_err(|error| {
            CliError::Usage(format!(
                "cannot list Execution Spaces for NodeDaemon: {error}"
            ))
        })?;
        Ok(spaces
            .into_iter()
            .map(|space| {
                let store = HarnessStore::new(space.store_root.clone());
                (space, store)
            })
            .collect())
    }

    fn node_lease_ttl_ms(&self) -> u64 {
        #[cfg(test)]
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

    /// Renew only authority already owned by this exact daemon instance.
    /// Discovery remains responsible for first acquisition; this heartbeat is
    /// deliberately unable to steal or create authority in an unscanned Space.
    pub(super) fn refresh_held_node_authorities(&self) -> CliResult<()> {
        let now_ms = current_unix_ms_u64();
        let ttl_ms = self.node_lease_ttl_ms();
        for (space, store) in self.registered_spaces()? {
            let lease = match store.latest_node_daemon_lease(&self.node_id) {
                Ok(Some(lease)) => lease,
                Ok(None) => continue,
                Err(error) => {
                    eprintln!(
                        "[node-daemon] cannot refresh Node authority in {}: {error}",
                        space.id
                    );
                    continue;
                }
            };
            if lease.daemon_id != self.daemon_id
                || lease.instance_id != self.instance_id
                || lease.status != harness_core::NodeDaemonLeaseStatus::Active
            {
                continue;
            }
            if let Err(error) = store.renew_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                now_ms,
                ttl_ms,
            ) {
                eprintln!(
                    "[node-daemon] cannot refresh Node authority in {}: {error}",
                    space.id
                );
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
        let lease = store
            .acquire_node_daemon_lease(
                &self.node_id,
                &self.daemon_id,
                &self.instance_id,
                now_ms,
                ttl_ms,
            )
            .map_err(CliError::Store)?;
        store
            .renew_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                now_ms,
                ttl_ms,
            )
            .map_err(CliError::Store)
    }

    pub(super) fn release_node_authorities(&self) -> CliResult<()> {
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
            if let Err(error) = store.release_node_daemon_lease(
                &self.node_id,
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                current_unix_ms_u64(),
            ) {
                failures.push(format!("{}: {error}", space.id));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(CliError::Usage(format!(
                "NODE_DAEMON_RELEASE_INCOMPLETE: {}",
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
