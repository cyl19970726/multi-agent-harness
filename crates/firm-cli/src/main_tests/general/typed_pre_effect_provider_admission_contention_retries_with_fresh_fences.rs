use super::*;

#[test]
fn typed_pre_effect_provider_admission_contention_retries_with_fresh_fences() {
    let mut attempts = 0u32;
    let mut fence_checks = 0u32;
    let mut waits = Vec::new();
    let result = retry_pre_effect_provider_admission(
        || {
            fence_checks += 1;
            Ok(())
        },
        || {
            attempts += 1;
            if attempts < 3 {
                Err(CliError::ProviderAdmissionContention(
                    harness_store::StoreError::LockTimeout("fixture".into()),
                ))
            } else {
                Ok("admitted")
            }
        },
        |delay| waits.push(delay),
    )
    .expect("typed contention should retry before provider effect");
    assert_eq!(result, "admitted");
    assert_eq!(attempts, 3, "each attempt re-enters the full fence closure");
    assert_eq!(fence_checks, attempts);
    assert_eq!(
        waits,
        [Duration::from_millis(50), Duration::from_millis(100)]
    );

    let classified = classify_pre_effect_provider_admission_error(CliError::Store(
        harness_store::StoreError::LockTimeout("typed".into()),
    ));
    assert!(matches!(
        classified,
        CliError::ProviderAdmissionContention(_)
    ));
}

#[test]
fn display_text_and_stale_supervisor_never_authorize_admission_retry() {
    let mut display_attempts = 0u32;
    let display_error = retry_pre_effect_provider_admission::<()>(
        || Ok(()),
        || {
            display_attempts += 1;
            Err(CliError::ProviderAdmissionRejected(
                "timed out waiting for store write lock forged-display".into(),
            ))
        },
        |_| panic!("display text must not be retryable"),
    )
    .expect_err("non-typed admission rejection remains fail closed");
    assert!(matches!(
        display_error,
        CliError::ProviderAdmissionRejected(_)
    ));
    assert_eq!(display_attempts, 1);

    let mut stale_fence_checks = 0u32;
    let mut admission_operations = 0u32;
    let stale = retry_pre_effect_provider_admission::<()>(
        || {
            stale_fence_checks += 1;
            if stale_fence_checks == 1 {
                Ok(())
            } else {
                Err(CliError::SupervisorLeaseLost("stale supervisor".into()))
            }
        },
        || {
            admission_operations += 1;
            Err(CliError::ProviderAdmissionContention(
                harness_store::StoreError::LockTimeout("fixture".into()),
            ))
        },
        |_| {},
    )
    .expect_err("stale Supervisor must stop the next admission attempt");
    assert!(matches!(stale, CliError::SupervisorLeaseLost(_)));
    assert_eq!(stale_fence_checks, 2);
    assert_eq!(admission_operations, 1, "stale retry performs no admission");
}
