//! Shared budgets; protocol observation and application I/O remain at their ports.

use std::time::Duration;

/// Only preparation known to have produced no command may use this retry.
/// Each attempt revalidates authority before touching the operation.
pub fn retry_pre_effect_admission<T, E>(
    mut revalidate: impl FnMut() -> Result<(), E>,
    mut operation: impl FnMut() -> Result<T, E>,
    is_contention: impl Fn(&E) -> bool,
    mut wait: impl FnMut(Duration),
) -> Result<T, E> {
    let mut retries = 0u32;
    loop {
        revalidate()?;
        match operation() {
            Err(error) if retries < 3 && is_contention(&error) => {
                wait(Duration::from_millis(50u64 << retries));
                retries += 1;
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_are_bounded_and_every_attempt_revalidates() {
        let mut validations = 0;
        let mut delays = Vec::new();
        let result: Result<(), &str> = retry_pre_effect_admission(
            || {
                validations += 1;
                Ok(())
            },
            || Err("contention"),
            |e| *e == "contention",
            |delay| delays.push(delay.as_millis()),
        );
        assert_eq!(result, Err("contention"));
        assert_eq!(validations, 4);
        assert_eq!(delays, [50, 100, 200]);
    }

    #[test]
    fn authority_loss_and_non_contention_never_retry() {
        let result: Result<(), &str> = retry_pre_effect_admission(
            || Err("fenced"),
            || panic!("must not prepare"),
            |_| true,
            |_| panic!("must not wait"),
        );
        assert_eq!(result, Err("fenced"));
        let result: Result<(), &str> = retry_pre_effect_admission(
            || Ok(()),
            || Err("unknown"),
            |_| false,
            |_| panic!("must not retry unknown effects"),
        );
        assert_eq!(result, Err("unknown"));
    }
}
