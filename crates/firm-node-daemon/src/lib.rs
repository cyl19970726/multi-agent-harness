//! Machine-scoped daemon authority and lifecycle, with CLI application composition injected.
pub mod daemon_application_port;
pub mod daemon_error;
pub mod daemon_protocol;
pub mod daemon_support;
pub mod lease_renewal_diagnostics;
pub mod scan_diagnostics;
pub mod start_failure_classification;
#[cfg(unix)]
mod supervisor_daemon;
#[cfg(unix)]
pub use supervisor_daemon::{
    node_daemon_socket_path, CONTROL_TRANSIENT_READ_BACKOFF, CONTROL_TRANSIENT_READ_RETRIES,
    NODE_DAEMON_STOP_DRAIN_BOUND,
};

/// Run the machine daemon in the foreground until its existing stop/drain path finishes.
#[cfg(unix)]
pub fn run(
    firm_home: std::path::PathBuf,
    node_id: String,
    max_concurrency: usize,
    input_acceptance_secs: u64,
    scan_interval_secs: u64,
    application: std::sync::Arc<dyn daemon_application_port::DaemonApplicationPort>,
) -> daemon_error::DaemonResult<()> {
    supervisor_daemon::MultiTeamDaemon::run(
        firm_home,
        node_id,
        max_concurrency,
        input_acceptance_secs,
        scan_interval_secs,
        application,
    )
}

#[cfg(unix)]
#[cfg(feature = "test-support")]
pub use supervisor_daemon::test_support;
