//! Provider-neutral Agent Team supervisor loop.
//!
//! This package owns only loop progression. The application port owns Work,
//! Message, Store, RuntimeCommand, Host acceptance, and every durable write;
//! provider packages own native protocol effects. Keeping the port generic
//! makes those forbidden dependency edges compile-time visible.

/// Result of one fully driven and durably settled supervisor round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorDirective<T> {
    /// The application settled the round and wants the next wake/cycle.
    Continue,
    /// The application reached an honest terminal member outcome.
    Complete(T),
}

/// Narrow application boundary consumed by the shared supervisor.
///
/// One call must cover prepare → provider cycle → terminal fencing → durable
/// settlement. Returning `Continue` before that boundary is an application
/// contract violation, tested where the concrete port is implemented.
pub trait SupervisorApplicationPort {
    type Outcome;
    type Error;

    fn drive_and_settle_round(
        &mut self,
        round: u32,
    ) -> Result<SupervisorDirective<Self::Outcome>, Self::Error>;
}

/// Own the single monotonically increasing Agent Team supervisor loop.
///
/// Provider-native goals/plans/subagents stay behind the application/provider
/// ports and cannot create a second top-level driver through this API.
pub fn run_team_supervisor<P: SupervisorApplicationPort>(
    port: &mut P,
) -> Result<P::Outcome, P::Error> {
    let mut round = 1u32;
    loop {
        match port.drive_and_settle_round(round)? {
            SupervisorDirective::Continue => {
                round = round
                    .checked_add(1)
                    .expect("supervisor round counter exhausted");
            }
            SupervisorDirective::Complete(outcome) => return Ok(outcome),
        }
    }
}

/// Closure adapter used by executable composition without exposing its Store
/// or coordination types to this package.
pub struct SupervisorPortFn<F>(F);

impl<F> SupervisorPortFn<F> {
    pub fn new(port: F) -> Self {
        Self(port)
    }
}

impl<F, T, E> SupervisorApplicationPort for SupervisorPortFn<F>
where
    F: FnMut(u32) -> Result<SupervisorDirective<T>, E>,
{
    type Outcome = T;
    type Error = E;

    fn drive_and_settle_round(
        &mut self,
        round: u32,
    ) -> Result<SupervisorDirective<Self::Outcome>, Self::Error> {
        (self.0)(round)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shared_loop_numbers_rounds_monotonically_until_terminal() {
        let mut observed = Vec::new();
        let result = {
            let mut port = SupervisorPortFn::new(|round| -> Result<_, ()> {
                observed.push(round);
                Ok(if round == 3 {
                    SupervisorDirective::Complete("done")
                } else {
                    SupervisorDirective::Continue
                })
            });
            run_team_supervisor(&mut port)
        };
        assert_eq!(result, Ok("done"));
        assert_eq!(observed, vec![1, 2, 3]);
    }

    #[test]
    fn application_failure_stops_without_driving_another_round() {
        let mut observed = Vec::new();
        let result = {
            let mut port = SupervisorPortFn::new(|round| {
                observed.push(round);
                if round == 2 {
                    Err("fenced")
                } else {
                    Ok(SupervisorDirective::<()>::Continue)
                }
            });
            run_team_supervisor(&mut port)
        };
        assert_eq!(result, Err("fenced"));
        assert_eq!(observed, vec![1, 2]);
    }
}
