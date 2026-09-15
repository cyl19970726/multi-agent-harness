//! Process-identity proof for one NodeDaemon predecessor instance.
//!
//! A NodeDaemon instance id is `<pid>:<minted unix-ms>:<daemon label>`. Both
//! the `firm daemon recover-predecessor` CLI and the successor daemon's own
//! first scan must answer the same question before any predecessor lease may
//! be settled: *is the process behind that pid really gone?* This module is
//! the single owner of that answer, so the two callers can never disagree.
//!
//! Absence is proven two ways, and a pid that merely *exists* is never enough
//! to prove life:
//!
//! - `kill(pid, 0) == ESRCH` — the pid does not exist at all. `EPERM` still
//!   proves a process exists, so it counts as alive.
//! - the pid exists, but the process behind it provably started *after* the
//!   predecessor's last evidence of life (its lease renewal, or the instance
//!   id's own mint time). Pids are recycled; such a process is a different
//!   one, and the predecessor is absent.
//!
//! Everything else — an unreadable probe, an unparsable elapsed time, a start
//! time that cannot be separated from the anchor — fails closed as `Alive`,
//! which keeps recovery human-gated exactly as it is today.

use std::process::Command;

/// Start-time comparisons are only as sharp as `ps` resolution (one second)
/// plus ordinary clock jitter. A process must look at least this much younger
/// than the anchor before it is called a recycled pid.
pub const REUSED_PID_START_TOLERANCE_MS: u64 = 2_000;

/// The decomposed `<pid>:<minted unix-ms>:<daemon label>` instance id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredecessorInstanceId {
    pub pid: i32,
    /// Present only when the middle segment is a unix-ms stamp. Older/foreign
    /// instance ids stay readable without it.
    pub minted_unix_ms: Option<u64>,
}

/// What the probe proved about the pid behind a predecessor instance id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredecessorProcessState {
    /// The pid does not exist.
    Absent,
    /// The pid exists, but its process started after the predecessor's last
    /// evidence of life, so it is a recycled pid, not the predecessor.
    ReusedPid,
    /// The pid exists and nothing proves it is a different process.
    Alive,
}

impl PredecessorProcessState {
    /// The stable reason string recorded in receipts, journals and
    /// `daemon status` diagnostics.
    pub fn reason(self) -> &'static str {
        match self {
            PredecessorProcessState::Absent => "process_absent",
            PredecessorProcessState::ReusedPid => "process_absent_reused_pid",
            PredecessorProcessState::Alive => "process_alive",
        }
    }
}

/// One complete, reportable process-death proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredecessorProcessProof {
    pub pid: i32,
    pub state: PredecessorProcessState,
    /// The unix-ms stamp carried by the instance id, when it has one.
    pub instance_minted_unix_ms: Option<u64>,
    /// The latest moment the predecessor was known to be alive.
    pub anchor_unix_ms: u64,
    /// Lower bound of the live process' start time, when it was measurable.
    pub started_unix_ms_lower_bound: Option<u64>,
    /// Human-readable evidence for the journal: the raw `ps` row, or why the
    /// start time could not be measured.
    pub evidence: String,
}

impl PredecessorProcessProof {
    /// Absent for recovery purposes: either no such pid, or a recycled one.
    pub fn counts_as_absent(&self) -> bool {
        matches!(
            self.state,
            PredecessorProcessState::Absent | PredecessorProcessState::ReusedPid
        )
    }

    pub fn reason(&self) -> &'static str {
        self.state.reason()
    }
}

/// Parse `<pid>:<minted unix-ms>:<daemon label>`. Only the pid is required;
/// a missing or unparsable mint stamp simply leaves the extra anchor absent.
pub fn parse_predecessor_instance_id(instance_id: &str) -> Result<PredecessorInstanceId, String> {
    let mut segments = instance_id.split(':');
    let pid = segments
        .next()
        .ok_or_else(|| "predecessor instance id has no process id".to_string())?
        .parse::<i32>()
        .map_err(|_| "predecessor instance id does not begin with a process id".to_string())?;
    if pid <= 0 {
        return Err("predecessor process id must be positive".into());
    }
    Ok(PredecessorInstanceId {
        pid,
        minted_unix_ms: segments.next().and_then(|raw| raw.parse::<u64>().ok()),
    })
}

/// Standard process-existence probe. `EPERM` still proves that a process
/// exists; only `ESRCH` is accepted as absence, and any other errno is an
/// unverified probe rather than a verdict.
pub fn process_exists(pid: i32) -> Result<bool, String> {
    #[cfg(unix)]
    {
        // SAFETY: kill(pid, 0) sends no signal and is the standard process
        // existence probe.
        let result = unsafe { libc::kill(pid, 0) };
        if result == 0 {
            return Ok(true);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Ok(false),
            Some(libc::EPERM) => Ok(true),
            Some(code) => Err(format!(
                "cannot verify predecessor process {pid}: errno {code}"
            )),
            None => Err(format!("cannot verify predecessor process {pid}")),
        }
    }
    #[cfg(not(unix))]
    {
        Err(format!(
            "cannot verify predecessor process {pid} on this platform"
        ))
    }
}

/// `[[dd-]hh:]mm:ss` as printed by both BSD and procps `ps -o etime=`.
pub fn parse_elapsed_seconds(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (days, clock) = match raw.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, clock),
        None => (0, raw),
    };
    let mut fields = clock.split(':').rev();
    let seconds = fields.next()?.parse::<u64>().ok()?;
    let minutes = fields.next()?.parse::<u64>().ok()?;
    let hours = match fields.next() {
        Some(hours) => hours.parse::<u64>().ok()?,
        None => 0,
    };
    if fields.next().is_some() {
        return None;
    }
    Some(((days * 24 + hours) * 60 + minutes) * 60 + seconds)
}

/// Lower bound of a live process' start time, measured through `ps`.
///
/// `ps -o etime=` truncates elapsed time to whole seconds, so the real
/// elapsed time is at least the printed value: the start time is therefore
/// strictly later than `now - (etime + 1s)`. `now_unix_ms` must be read
/// *before* the probe runs, which keeps that bound sound even if `ps` is slow.
///
/// `Ok(None)` means the row could not be measured (the process disappeared, or
/// the output was unparsable); that is deliberately not an error, because the
/// classifier fails closed on it.
pub fn process_start_lower_bound_unix_ms(
    pid: i32,
    now_unix_ms: u64,
) -> Result<Option<(u64, String)>, String> {
    let output = Command::new("ps")
        .args(["-o", "etime=,lstart=", "-p", &pid.to_string()])
        .output()
        .map_err(|error| format!("cannot run ps for predecessor process {pid}: {error}"))?;
    let row = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if row.is_empty() {
        return Ok(None);
    }
    let Some(elapsed_seconds) = row
        .split_whitespace()
        .next()
        .and_then(parse_elapsed_seconds)
    else {
        return Ok(None);
    };
    let lower_bound = now_unix_ms.saturating_sub(elapsed_seconds.saturating_add(1) * 1_000);
    Ok(Some((lower_bound, row)))
}

/// Pure classifier: everything above feeds this, and the reused-pid case is
/// unit-testable with a synthetic start time.
pub fn classify_predecessor_process(
    instance: &PredecessorInstanceId,
    exists: bool,
    start_lower_bound: Option<(u64, String)>,
    anchor_unix_ms: u64,
) -> PredecessorProcessProof {
    // The predecessor existed before it minted its instance id and before it
    // last renewed its lease, so the latest of the two is the moment it was
    // certainly alive.
    let anchor = anchor_unix_ms.max(instance.minted_unix_ms.unwrap_or(0));
    if !exists {
        return PredecessorProcessProof {
            pid: instance.pid,
            state: PredecessorProcessState::Absent,
            instance_minted_unix_ms: instance.minted_unix_ms,
            anchor_unix_ms: anchor,
            started_unix_ms_lower_bound: None,
            evidence: format!("kill({}, 0) reported ESRCH", instance.pid),
        };
    }
    let Some((lower_bound, row)) = start_lower_bound else {
        return PredecessorProcessProof {
            pid: instance.pid,
            state: PredecessorProcessState::Alive,
            instance_minted_unix_ms: instance.minted_unix_ms,
            anchor_unix_ms: anchor,
            started_unix_ms_lower_bound: None,
            evidence: format!(
                "process {} exists and its start time could not be measured",
                instance.pid
            ),
        };
    };
    let reused = lower_bound > anchor.saturating_add(REUSED_PID_START_TOLERANCE_MS);
    PredecessorProcessProof {
        pid: instance.pid,
        state: if reused {
            PredecessorProcessState::ReusedPid
        } else {
            PredecessorProcessState::Alive
        },
        instance_minted_unix_ms: instance.minted_unix_ms,
        anchor_unix_ms: anchor,
        started_unix_ms_lower_bound: Some(lower_bound),
        evidence: format!("ps: {row}"),
    }
}

/// Probe one predecessor instance id against the last moment it was known to
/// be alive (normally its lease's `renewed_unix_ms`).
pub fn probe_predecessor_process(
    instance_id: &str,
    anchor_unix_ms: u64,
    now_unix_ms: u64,
) -> Result<PredecessorProcessProof, String> {
    let instance = parse_predecessor_instance_id(instance_id)?;
    let exists = process_exists(instance.pid)?;
    let start_lower_bound = if exists {
        process_start_lower_bound_unix_ms(instance.pid, now_unix_ms)?
    } else {
        None
    };
    Ok(classify_predecessor_process(
        &instance,
        exists,
        start_lower_bound,
        anchor_unix_ms,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_unix_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock")
            .as_millis() as u64
    }

    #[test]
    fn instance_ids_expose_the_pid_and_the_optional_mint_stamp() {
        let parsed = parse_predecessor_instance_id("4242:1700000000000:node-daemon:n1")
            .expect("well-formed instance id");
        assert_eq!(parsed.pid, 4242);
        assert_eq!(parsed.minted_unix_ms, Some(1_700_000_000_000));

        let legacy = parse_predecessor_instance_id("4242:not-a-stamp:node-daemon")
            .expect("legacy instance id stays readable");
        assert_eq!(legacy.minted_unix_ms, None);

        assert!(parse_predecessor_instance_id("node-daemon:n1").is_err());
        assert!(parse_predecessor_instance_id("0:1:node-daemon").is_err());
        assert!(parse_predecessor_instance_id("-3:1:node-daemon").is_err());
    }

    #[test]
    fn elapsed_times_parse_in_every_ps_shape() {
        assert_eq!(parse_elapsed_seconds("00:00"), Some(0));
        assert_eq!(parse_elapsed_seconds("01:05"), Some(65));
        assert_eq!(parse_elapsed_seconds("02:01:05"), Some(7265));
        assert_eq!(parse_elapsed_seconds("02-17:37:22"), Some(236_242));
        assert_eq!(parse_elapsed_seconds(""), None);
        assert_eq!(parse_elapsed_seconds("nonsense"), None);
        assert_eq!(parse_elapsed_seconds("1:2:3:4"), None);
    }

    #[test]
    fn a_missing_pid_is_absent_and_a_live_pid_is_alive() {
        let instance = PredecessorInstanceId {
            pid: 4242,
            minted_unix_ms: Some(1_000),
        };
        let absent = classify_predecessor_process(&instance, false, None, 2_000);
        assert_eq!(absent.state, PredecessorProcessState::Absent);
        assert!(absent.counts_as_absent());
        assert_eq!(absent.reason(), "process_absent");

        // Started before the anchor: this really is the predecessor.
        let alive =
            classify_predecessor_process(&instance, true, Some((500, "ps row".into())), 2_000);
        assert_eq!(alive.state, PredecessorProcessState::Alive);
        assert!(!alive.counts_as_absent());
    }

    #[test]
    fn a_pid_that_started_after_the_anchor_is_a_reused_pid() {
        let instance = PredecessorInstanceId {
            pid: 4242,
            minted_unix_ms: Some(1_000),
        };
        let reused = classify_predecessor_process(
            &instance,
            true,
            Some((2_000 + REUSED_PID_START_TOLERANCE_MS + 1, "ps row".into())),
            2_000,
        );
        assert_eq!(reused.state, PredecessorProcessState::ReusedPid);
        assert!(reused.counts_as_absent());
        assert_eq!(reused.reason(), "process_absent_reused_pid");

        // Inside the tolerance the proof is refused, not guessed.
        let ambiguous = classify_predecessor_process(
            &instance,
            true,
            Some((2_000 + REUSED_PID_START_TOLERANCE_MS, "ps row".into())),
            2_000,
        );
        assert_eq!(ambiguous.state, PredecessorProcessState::Alive);
    }

    #[test]
    fn the_instance_mint_stamp_raises_a_stale_anchor() {
        let instance = PredecessorInstanceId {
            pid: 4242,
            minted_unix_ms: Some(50_000),
        };
        // Anchor 1 would call this a reused pid; the mint stamp proves the
        // predecessor was still alive at 50_000.
        let proof =
            classify_predecessor_process(&instance, true, Some((10_000, "ps row".into())), 1);
        assert_eq!(proof.state, PredecessorProcessState::Alive);
        assert_eq!(proof.anchor_unix_ms, 50_000);
    }

    #[test]
    fn an_unmeasurable_start_time_fails_closed() {
        let instance = PredecessorInstanceId {
            pid: 4242,
            minted_unix_ms: None,
        };
        let proof = classify_predecessor_process(&instance, true, None, 1);
        assert_eq!(proof.state, PredecessorProcessState::Alive);
        assert!(proof.evidence.contains("could not be measured"));
    }

    #[test]
    fn this_process_is_alive_against_its_own_start_and_reused_against_a_stale_anchor() {
        let now = now_unix_ms();
        let pid = std::process::id() as i32;
        assert!(process_exists(pid).expect("probe own pid"));
        let measured = process_start_lower_bound_unix_ms(pid, now).expect("measure own start time");
        let Some((lower_bound, _)) = measured.clone() else {
            // `ps` is unavailable in this environment; the classifier's
            // fail-closed branch is covered by its own test.
            return;
        };
        assert!(lower_bound <= now);

        let instance = PredecessorInstanceId {
            pid,
            minted_unix_ms: None,
        };
        // Anchored at the epoch, a process running right now must be younger.
        let reused = classify_predecessor_process(&instance, true, measured.clone(), 1);
        assert_eq!(reused.state, PredecessorProcessState::ReusedPid);
        // Anchored at "now", nothing proves it is a different process.
        let alive = classify_predecessor_process(&instance, true, measured, now);
        assert_eq!(alive.state, PredecessorProcessState::Alive);
    }

    #[test]
    fn a_probe_of_an_absent_pid_reports_absence_end_to_end() {
        // 2147483647 is above every supported pid_max, so it is never live.
        let proof = probe_predecessor_process("2147483647:1:dead-daemon", 1, now_unix_ms())
            .expect("probe an absent pid");
        assert_eq!(proof.state, PredecessorProcessState::Absent);
    }
}
