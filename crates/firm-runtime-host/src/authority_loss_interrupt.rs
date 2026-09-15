//! One cooperative interrupt per live provider turn when this process loses
//! the authority that admitted it.
//!
//! Losing machine authority or a TeamRun Supervisor lease permanently closes
//! provider-effect admission, so the interrupt that stops a running turn
//! CANNOT be a durable `RuntimeCommand`: nothing may be admitted under lost
//! authority, and that refusal is correct. This registry is the documented
//! exception (ADR 0074): a process-local fan-out that hands each live turn one
//! cooperative interrupt through the adapter's own `interrupt_current_cycle`
//! path, before the drain's bounded cooperative wait and its SIGKILL backstop.
//!
//! Two properties keep the fan-out honest:
//!
//! * **At most one request per turn.** A second latch (for example the machine
//!   latch after a Supervisor latch) never re-requests a turn that already
//!   carries a request, so a provider sees one interrupt, not a storm.
//! * **The wait is bounded, the request is not.** The caller waits only a short
//!   window for turns to observe the request so the drain is never blocked by a
//!   hung provider. The request itself stays latched, so a turn that polls
//!   later still gets its interrupt; the report simply records it as
//!   `not_observed` inside that window.

use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

/// Which live turns one authority-loss latch owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityLossScope {
    /// The machine-scoped NodeDaemon lease was lost: every live turn in this
    /// process was admitted under it.
    Process,
    /// One TeamRun's Supervisor lease was lost: only that run's live turns.
    TeamRun(String),
}

impl AuthorityLossScope {
    fn covers(&self, turn: &LiveProviderTurnEntry) -> bool {
        match self {
            Self::Process => true,
            Self::TeamRun(team_run_id) => turn.team_run_id == *team_run_id,
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Process => "process",
            Self::TeamRun(team_run_id) => team_run_id.as_str(),
        }
    }
}

/// What one live turn did with its cooperative interrupt request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityLossInterruptOutcome {
    /// The turn's own supervisor thread consumed the request and handed it to
    /// the adapter's native interrupt path.
    Dispatched,
    /// The turn ended before it observed the request; there was nothing left
    /// to interrupt.
    TurnEnded,
    /// The turn was still live at the deadline and had not polled control.
    /// The request stays latched for its next poll; SIGKILL is the backstop.
    NotObserved,
}

impl AuthorityLossInterruptOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dispatched => "dispatched",
            Self::TurnEnded => "turn_ended",
            Self::NotObserved => "not_observed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthorityLossInterruptTurn {
    pub provider: String,
    pub team_run_id: String,
    pub member_run_id: String,
    pub outcome: AuthorityLossInterruptOutcome,
}

/// The exact fan-out evidence one authority-loss latch produced.
#[derive(Debug, Clone)]
pub struct AuthorityLossInterruptReport {
    pub scope: String,
    pub reason: String,
    pub turns_live: usize,
    pub turns_interrupted: usize,
    pub turns: Vec<AuthorityLossInterruptTurn>,
}

impl AuthorityLossInterruptReport {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "scope": self.scope,
            "reason": self.reason,
            "turns_live": self.turns_live,
            "turns_interrupted": self.turns_interrupted,
            "turns": self
                .turns
                .iter()
                .map(|turn| serde_json::json!({
                    "provider": turn.provider,
                    "team_run_id": turn.team_run_id,
                    "member_run_id": turn.member_run_id,
                    "outcome": turn.outcome.as_str(),
                }))
                .collect::<Vec<_>>(),
        })
    }
}

#[derive(Debug)]
struct LiveProviderTurnEntry {
    provider: &'static str,
    team_run_id: String,
    member_run_id: String,
    /// The thread driving this turn. A latch reached from that same thread
    /// must never wait on it, or it would be waiting on itself.
    owner: ThreadId,
    requested: Option<String>,
    dispatched: bool,
}

#[derive(Debug, Default)]
struct LiveProviderTurns {
    next_token: u64,
    turns: HashMap<u64, LiveProviderTurnEntry>,
    /// Tokens whose cooperative interrupt was taken. A turn commonly dispatches
    /// its interrupt and then ends within the same observation window, so the
    /// dispatch fact must outlive the entry or the report would downgrade a
    /// real interrupt to `turn_ended`. The reporting fan-out drains what it
    /// reads, so this holds at most the turns that were live at one latch.
    dispatched: HashSet<u64>,
}

fn live_provider_turns() -> &'static (Mutex<LiveProviderTurns>, Condvar) {
    static TURNS: OnceLock<(Mutex<LiveProviderTurns>, Condvar)> = OnceLock::new();
    TURNS.get_or_init(|| (Mutex::new(LiveProviderTurns::default()), Condvar::new()))
}

/// Process-local registration for one live provider turn. The supervisor loop
/// keeps this guard for exactly as long as a provider cycle is being driven;
/// Drop removes it, so a finished turn is never reported as interruptible.
#[derive(Debug)]
pub struct LiveProviderTurn {
    token: u64,
}

pub fn register_live_provider_turn(
    provider: &'static str,
    team_run_id: &str,
    member_run_id: &str,
) -> LiveProviderTurn {
    let (lock, _) = live_provider_turns();
    let mut turns = lock.lock().unwrap_or_else(|error| error.into_inner());
    turns.next_token = turns.next_token.wrapping_add(1).max(1);
    let token = turns.next_token;
    turns.turns.insert(
        token,
        LiveProviderTurnEntry {
            provider,
            team_run_id: team_run_id.to_string(),
            member_run_id: member_run_id.to_string(),
            owner: std::thread::current().id(),
            requested: None,
            dispatched: false,
        },
    );
    LiveProviderTurn { token }
}

impl LiveProviderTurn {
    /// Consume this turn's pending cooperative interrupt exactly once. The
    /// supervisor's control poll calls this and, on `Some`, returns
    /// `CycleControl { interrupt: true }` so the adapter issues its own native
    /// interrupt primitive. No durable record is written on this path.
    pub fn take_authority_loss_interrupt(&self) -> Option<String> {
        let (lock, condvar) = live_provider_turns();
        let mut guard = lock.lock().unwrap_or_else(|error| error.into_inner());
        let turns = &mut *guard;
        let entry = turns.turns.get_mut(&self.token)?;
        if entry.dispatched {
            return None;
        }
        let reason = entry.requested.clone()?;
        entry.dispatched = true;
        turns.dispatched.insert(self.token);
        condvar.notify_all();
        Some(reason)
    }
}

impl Drop for LiveProviderTurn {
    fn drop(&mut self) {
        let (lock, condvar) = live_provider_turns();
        lock.lock()
            .unwrap_or_else(|error| error.into_inner())
            .turns
            .remove(&self.token);
        condvar.notify_all();
    }
}

/// Hand one cooperative interrupt to every live provider turn in `scope`, then
/// wait at most `observe_timeout` for those turns to pick it up.
///
/// The wait exists only so the caller can journal what actually happened; it
/// never gates the drain. Turns owned by the calling thread are never waited
/// on, because an authority latch can be reached from inside a turn's own
/// supervisor thread.
pub fn request_authority_loss_interrupt(
    scope: &AuthorityLossScope,
    reason: &str,
    observe_timeout: Duration,
) -> AuthorityLossInterruptReport {
    let (lock, condvar) = live_provider_turns();
    let mut turns = lock.lock().unwrap_or_else(|error| error.into_inner());

    // Snapshot each target's identity now: a turn may end before the report is
    // built, and an interrupted member must still be named in the evidence.
    let mut targets: Vec<(u64, String, String, String)> = Vec::new();
    for (token, entry) in turns.turns.iter_mut() {
        if !scope.covers(entry) {
            continue;
        }
        // One interrupt per turn for the life of that turn: a later latch
        // never overwrites or repeats an already-published request.
        if entry.requested.is_none() {
            entry.requested = Some(reason.to_string());
        }
        targets.push((
            *token,
            entry.provider.to_string(),
            entry.team_run_id.clone(),
            entry.member_run_id.clone(),
        ));
    }
    targets.sort_by(|left, right| left.0.cmp(&right.0));

    let current = std::thread::current().id();
    let deadline = Instant::now() + observe_timeout;
    loop {
        let waiting = targets.iter().any(|(token, ..)| {
            !turns.dispatched.contains(token)
                && turns
                    .turns
                    .get(token)
                    .is_some_and(|entry| entry.owner != current)
        });
        if !waiting {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        turns = condvar
            .wait_timeout(turns, remaining)
            .unwrap_or_else(|error| error.into_inner())
            .0;
    }

    let mut report_turns = Vec::with_capacity(targets.len());
    for (token, provider, team_run_id, member_run_id) in targets {
        let outcome = if turns.dispatched.remove(&token) {
            AuthorityLossInterruptOutcome::Dispatched
        } else if turns.turns.contains_key(&token) {
            AuthorityLossInterruptOutcome::NotObserved
        } else {
            AuthorityLossInterruptOutcome::TurnEnded
        };
        report_turns.push(AuthorityLossInterruptTurn {
            provider,
            team_run_id,
            member_run_id,
            outcome,
        });
    }

    AuthorityLossInterruptReport {
        scope: scope.label().to_string(),
        reason: reason.to_string(),
        turns_live: report_turns.len(),
        turns_interrupted: report_turns
            .iter()
            .filter(|turn| turn.outcome == AuthorityLossInterruptOutcome::Dispatched)
            .count(),
        turns: report_turns,
    }
}

#[cfg(test)]
mod tests;
