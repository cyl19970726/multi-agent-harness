//! Provider-neutral Agent Team supervisor state machine.
//!
//! The supervisor owns the only top-level wake/claim → provider cycle →
//! durable-settlement progression. Application ports provide concrete Work,
//! Message, Store, RuntimeCommand, and provider effects without taking
//! ownership of loop order.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorWake<C, T> {
    Cycle(C),
    Complete(T),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorDirective<T> {
    Continue,
    Complete(T),
}

/// Narrow application boundary consumed by the shared supervisor. The three
/// methods expose no provider or store vocabulary, but fix the phase order so
/// no executable can bypass wake/claim or start a cycle before settlement.
pub trait SupervisorApplicationPort {
    type Cycle;
    type DrivenCycle;
    type Outcome;
    type Error;

    fn wake_and_claim(
        &mut self,
        round: u32,
    ) -> Result<SupervisorWake<Self::Cycle, Self::Outcome>, Self::Error>;
    fn drive_cycle(
        &mut self,
        round: u32,
        cycle: Self::Cycle,
    ) -> Result<Self::DrivenCycle, Self::Error>;
    fn settle_cycle(
        &mut self,
        round: u32,
        driven: Self::DrivenCycle,
    ) -> Result<SupervisorDirective<Self::Outcome>, Self::Error>;
}

/// Run the single monotonically increasing Agent Team state machine. A round
/// advances only after durable settlement returns `Continue`.
pub fn run_team_supervisor<P: SupervisorApplicationPort>(
    port: &mut P,
) -> Result<P::Outcome, P::Error> {
    let mut round = 1u32;
    loop {
        let cycle = match port.wake_and_claim(round)? {
            SupervisorWake::Cycle(cycle) => cycle,
            SupervisorWake::Complete(outcome) => return Ok(outcome),
        };
        let driven = port.drive_cycle(round, cycle)?;
        match port.settle_cycle(round, driven)? {
            SupervisorDirective::Continue => {
                round = round
                    .checked_add(1)
                    .expect("supervisor round counter exhausted");
            }
            SupervisorDirective::Complete(outcome) => return Ok(outcome),
        }
    }
}

/// Composition adapter. All phases operate on one explicit application state,
/// preventing independently captured state from becoming parallel drivers.
pub struct SupervisorPortFn<S, W, D, T> {
    pub state: S,
    wake: W,
    drive: D,
    settle: T,
}

impl<S, W, D, T> SupervisorPortFn<S, W, D, T> {
    pub fn new(state: S, wake: W, drive: D, settle: T) -> Self {
        Self {
            state,
            wake,
            drive,
            settle,
        }
    }
}

impl<S, W, D, T, C, R, O, E> SupervisorApplicationPort for SupervisorPortFn<S, W, D, T>
where
    W: FnMut(&mut S, u32) -> Result<SupervisorWake<C, O>, E>,
    D: FnMut(&mut S, u32, C) -> Result<R, E>,
    T: FnMut(&mut S, u32, R) -> Result<SupervisorDirective<O>, E>,
{
    type Cycle = C;
    type DrivenCycle = R;
    type Outcome = O;
    type Error = E;

    fn wake_and_claim(&mut self, round: u32) -> Result<SupervisorWake<C, O>, E> {
        (self.wake)(&mut self.state, round)
    }

    fn drive_cycle(&mut self, round: u32, cycle: C) -> Result<R, E> {
        (self.drive)(&mut self.state, round, cycle)
    }

    fn settle_cycle(&mut self, round: u32, driven: R) -> Result<SupervisorDirective<O>, E> {
        (self.settle)(&mut self.state, round, driven)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervisor_owns_phase_order_and_round_progression() {
        let mut port = SupervisorPortFn::new(
            Vec::new(),
            |events: &mut Vec<String>, round| -> Result<_, ()> {
                events.push(format!("wake:{round}"));
                Ok(SupervisorWake::Cycle(round))
            },
            |events: &mut Vec<String>, round, cycle| -> Result<_, ()> {
                events.push(format!("drive:{round}"));
                Ok(cycle)
            },
            |events: &mut Vec<String>, round, _| -> Result<_, ()> {
                events.push(format!("settle:{round}"));
                Ok(if round == 2 {
                    SupervisorDirective::Complete(events.clone())
                } else {
                    SupervisorDirective::Continue
                })
            },
        );
        let events = run_team_supervisor(&mut port).unwrap();
        assert_eq!(
            events,
            ["wake:1", "drive:1", "settle:1", "wake:2", "drive:2", "settle:2"]
        );
    }

    #[test]
    fn failed_settlement_fences_the_next_wake() {
        let mut port = SupervisorPortFn::new(
            Vec::new(),
            |events: &mut Vec<&str>, _| -> Result<_, &str> {
                events.push("wake");
                Ok(SupervisorWake::Cycle(()))
            },
            |events: &mut Vec<&str>, _, _| -> Result<_, &str> {
                events.push("drive");
                Ok(())
            },
            |events: &mut Vec<&str>, _, _| -> Result<SupervisorDirective<()>, &str> {
                events.push("settle");
                Err("fenced")
            },
        );
        assert_eq!(run_team_supervisor(&mut port), Err("fenced"));
        assert_eq!(port.state, ["wake", "drive", "settle"]);
    }
}
