//! Application adapter for the Claude provider package.

use std::time::Duration;

use harness_runtime_contract as rt;

pub(crate) use harness_provider_claude::ClaudeTeamRuntimeConfig;

fn callback_error(error: crate::CliError) -> harness_provider_claude::ClaudeError {
    harness_provider_claude::ClaudeError::Callback {
        supervisor_lease_lost: error.is_supervisor_lease_lost(),
        detail: error.to_string(),
    }
}

impl From<harness_provider_claude::ClaudeError> for crate::CliError {
    fn from(error: harness_provider_claude::ClaudeError) -> Self {
        match error {
            harness_provider_claude::ClaudeError::Callback {
                detail,
                supervisor_lease_lost: true,
            } => crate::CliError::SupervisorLeaseLost(detail),
            other => crate::CliError::Usage(other.to_string()),
        }
    }
}

pub(crate) struct ClaudeTeamRuntime(harness_provider_claude::ClaudeTeamRuntime);

impl ClaudeTeamRuntime {
    pub(crate) fn spawn(config: ClaudeTeamRuntimeConfig) -> crate::CliResult<Self> {
        Ok(Self(harness_provider_claude::ClaudeTeamRuntime::spawn(
            config,
        )?))
    }
}

impl rt::TeamRuntimeAdapter for ClaudeTeamRuntime {
    type Error = crate::CliError;

    fn provider(&self) -> &'static str {
        rt::TeamRuntimeAdapter::provider(&self.0)
    }

    fn display_name(&self) -> &'static str {
        rt::TeamRuntimeAdapter::display_name(&self.0)
    }

    fn capability_bindings() -> Vec<rt::CapabilityBinding> {
        harness_provider_claude::ClaudeTeamRuntime::capability_bindings()
    }

    fn ensure_alive(&mut self) -> crate::CliResult<()> {
        Ok(rt::TeamRuntimeAdapter::ensure_alive(&mut self.0)?)
    }

    fn native_session_locator(&self) -> &str {
        rt::TeamRuntimeAdapter::native_session_locator(&self.0)
    }

    fn native_locator_kind(&self) -> &'static str {
        rt::TeamRuntimeAdapter::native_locator_kind(&self.0)
    }

    fn bind_authority_session(
        &mut self,
        session: harness_core::agentfirm_api::AgentSession,
        profile: &harness_core::ProviderIntegrationProfile,
    ) -> crate::CliResult<()> {
        Ok(rt::TeamRuntimeAdapter::bind_authority_session(
            &mut self.0,
            session,
            profile,
        )?)
    }

    fn run_cycle(
        &mut self,
        input: &str,
        idle_timeout: Duration,
        on_input_accepted: &mut dyn FnMut(&rt::ControlTransportReceipt) -> crate::CliResult<()>,
        on_steer_result: &mut dyn FnMut(
            &rt::SteerRequest,
            &rt::SteerProviderResult,
        ) -> crate::CliResult<()>,
        on_event: &mut dyn FnMut(&serde_json::Value),
        poll_control: &mut dyn FnMut() -> rt::CycleControl,
    ) -> crate::CliResult<rt::ExecutionCycleOutcome> {
        Ok(rt::TeamRuntimeAdapter::run_cycle(
            &mut self.0,
            input,
            idle_timeout,
            &mut |receipt| on_input_accepted(receipt).map_err(callback_error),
            &mut |request, result| on_steer_result(request, result).map_err(callback_error),
            on_event,
            poll_control,
        )?)
    }

    fn project_live(event: &serde_json::Value) -> Option<(rt::LiveProviderActivityKind, String)> {
        harness_provider_claude::ClaudeTeamRuntime::project_live(event)
    }

    fn native_control<'a>(
        close: &'a mut bool,
        interrupt: &'a mut bool,
    ) -> Box<dyn rt::ProviderNativeControl + 'a> {
        harness_provider_claude::ClaudeTeamRuntime::native_control(close, interrupt)
    }

    fn supports_inject_current_cycle(&self) -> bool {
        rt::TeamRuntimeAdapter::supports_inject_current_cycle(&self.0)
    }

    fn supports_native_boundary_queue(&self) -> bool {
        rt::TeamRuntimeAdapter::supports_native_boundary_queue(&self.0)
    }
}

impl rt::RuntimeAdapter for ClaudeTeamRuntime {
    fn describe(&self) -> &rt::RuntimeDescription {
        rt::RuntimeAdapter::describe(&self.0)
    }

    fn open_or_resume(
        &mut self,
        fence: rt::RuntimeFence<'_>,
        native_session_ref: Option<&str>,
    ) -> Result<rt::RuntimeObservation, rt::RuntimeContractError> {
        rt::RuntimeAdapter::open_or_resume(&mut self.0, fence, native_session_ref)
    }

    fn execute_control(
        &mut self,
        fence: rt::RuntimeFence<'_>,
        request: rt::ControlRequest,
    ) -> Result<rt::EffectReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::execute_control(&mut self.0, fence, request)
    }

    fn observe(
        &mut self,
        fence: rt::RuntimeFence<'_>,
    ) -> Result<rt::RuntimeObservation, rt::RuntimeContractError> {
        rt::RuntimeAdapter::observe(&mut self.0, fence)
    }

    fn inspect_effect(
        &mut self,
        fence: rt::RuntimeFence<'_>,
        effect_id: &str,
    ) -> Result<rt::EffectInspection, rt::RuntimeContractError> {
        rt::RuntimeAdapter::inspect_effect(&mut self.0, fence, effect_id)
    }

    fn reconcile(
        &mut self,
        fence: rt::RuntimeFence<'_>,
        inspection: &rt::EffectInspection,
    ) -> Result<rt::ReconcileReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::reconcile(&mut self.0, fence, inspection)
    }

    fn close_runtime(
        &mut self,
        fence: rt::RuntimeFence<'_>,
    ) -> Result<rt::MemberRuntimeCloseReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::close_runtime(&mut self.0, fence)
    }

    fn quiesce(
        &mut self,
        fence: rt::RuntimeFence<'_>,
    ) -> Result<rt::QuiesceReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::quiesce(&mut self.0, fence)
    }

    fn release(
        &mut self,
        fence: rt::RuntimeFence<'_>,
    ) -> Result<rt::ReleaseReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::release(&mut self.0, fence)
    }
}
