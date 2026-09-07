//! Existing cross-boundary daemon scenarios using the real CLI application.
use crate::daemon_application::DaemonApplication;
use crate::daemon_protocol::{NativeSessionWakePostError, AT_CAPACITY_REFUSAL};
use crate::{current_unix_ms_u64, CliError, CliResult, HarnessStore, TeamRunDriveOutcome};
use harness_node_daemon::node_daemon_socket_path;
use harness_node_daemon::test_support::*;
use std::{
    io::{BufRead, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
mod adoption_tests;
mod drain_blocked_member_tests;
mod drain_inflight_work_tests;
mod drain_recovery_tests;
mod drive_outcome_tests;
mod lease_renewal_tests;
mod recover_blocked_lane_blocker_tests;
mod recover_lost_execution_tests;
mod self_stop_events_tests;
#[cfg(unix)]
mod shutdown_tests;
mod stop_drain_tests;
mod tests;
