//! Shared attempt-scoped failure classification for adoption and admission.
use crate::CliError;

/// Codes that describe this daemon generation or a lost race, never a durable
/// property of the TeamRun. They are matched as the error's own leading code
/// token, never as a substring of the whole chain: an error that merely quotes
/// a fenced code while reporting something structural must still hold.
const TRANSIENT_START_FAILURE_CODES: &[&str] = &[
    "NODE_HAS_NO_REGISTERED_PROJECT",
    "NODE_DAEMON_GENERATION_FENCED",
    "NODE_DAEMON_MACHINE_AUTHORITY_LOST",
    "SUPERVISOR_GENERATION_FENCED",
    // Defense in depth. The start path resolves TeamRun scope through the
    // typed route, so this arrives as `CliError::Store(Conflict)` and is
    // already transient; listing the code keeps a future re-flattening from
    // silently turning a lost race back into a structural verdict.
    "TEAM_RUN_CHANGED",
];

/// The two capacity/ownership rejections `start_supervising` writes as prose
/// rather than as a code. Matched as a prefix for the same reason.
const TRANSIENT_START_FAILURE_PREFIXES: &[&str] = &[
    crate::supervisor_daemon::AT_CAPACITY_REFUSAL,
    "NodeDaemon already manages",
];

/// The leading `CODE:` token of a message, when it has one.
fn leading_error_code(message: &str) -> Option<&str> {
    let code = message.split(':').next()?.trim();
    (!code.is_empty()
        && code.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        }))
    .then_some(code)
}

/// Start failures that say nothing durable about this TeamRun. Holding
/// adoption on them would suppress a run that a freed concurrency slot, a won
/// CAS retry or a restored daemon generation makes startable again with no
/// canonical change at all.
///
/// Classification is typed first. `CliError::Usage` is the catch-all this
/// codebase flattens Store conflicts into, so its text is consulted only for
/// the error's own leading code token: matching a substring anywhere in the
/// chain let an ordinary CAS conflict — or any error that merely mentioned a
/// fenced code — decide the outcome for a whole TeamRun.
pub(crate) fn start_failure_is_transient(error: &CliError) -> bool {
    match error {
        // Store contention, provider-admission contention and a lost
        // machine/Supervisor authority are properties of this attempt, never
        // of the run.
        CliError::Store(_)
        | CliError::ProviderAdmissionContention(_)
        | CliError::SupervisorLeaseLost(_)
        | CliError::ProviderProcessAdmissionClosed(_) => true,
        // Defensive, and not currently reachable from `start_supervising`:
        // that path stops at the durable Supervisor registration and the
        // Planning→Running lifecycle CAS, before any provider effect is
        // prepared, so neither variant can be the start failure being
        // classified. Should a future start step prepare an effect, an
        // accepted or ambiguous one is emphatically not a no-progress
        // observation — the unresolved-RuntimeCommand branch above already
        // owns it and writes the stronger recovery-required marker — so
        // silently converting it into a weak canonical-state hold would lose
        // that diagnosis. Kept explicit rather than left to the `_` arm.
        CliError::ProviderEffectAccepted(_) | CliError::RuntimeRecoveryRequired(_) => true,
        CliError::Usage(message) => {
            TRANSIENT_START_FAILURE_PREFIXES
                .iter()
                .any(|prefix| message.starts_with(prefix))
                || leading_error_code(message)
                    .is_some_and(|code| TRANSIENT_START_FAILURE_CODES.contains(&code))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leading_error_code_reads_only_a_real_leading_code_token() {
        assert_eq!(
            leading_error_code("NODE_DAEMON_GENERATION_FENCED: detail"),
            Some("NODE_DAEMON_GENERATION_FENCED")
        );
        assert_eq!(leading_error_code("CODE_9: detail"), Some("CODE_9"));
        assert_eq!(leading_error_code("team run r is pinned"), None);
        assert_eq!(
            leading_error_code("store error: SUPERVISOR_GENERATION_FENCED"),
            None
        );
        assert_eq!(leading_error_code(""), None);
    }
}
