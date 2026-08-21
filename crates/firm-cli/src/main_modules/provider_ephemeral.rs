use super::*;

pub(super) use harness_runtime_host::{kill_process_tree as kill_worker_tree, NdjsonRun};

pub(super) fn run_ndjson_child(
    command: Command,
    timeout_ms: u64,
    wall_clock_ms: Option<u64>,
    context: &str,
) -> CliResult<NdjsonRun> {
    harness_runtime_host::run_ndjson_child(
        command,
        Duration::from_millis(timeout_ms),
        wall_clock_ms.map(Duration::from_millis),
        context,
    )
    .map_err(|error| CliError::Usage(error.to_string()))
}
