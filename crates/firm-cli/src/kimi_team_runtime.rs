//! Application adapter for the Kimi provider package.


use harness_runtime_contract as rt;

fn callback_error(error: crate::CliError) -> harness_provider_kimi::KimiError {
    harness_provider_kimi::KimiError::Callback {
        supervisor_lease_lost: error.is_supervisor_lease_lost(),
        detail: error.to_string(),
    }
}

pub(crate) struct KimiTeamRuntime<'a>(harness_provider_kimi::KimiTeamRuntime<'a>);

impl<'a> KimiTeamRuntime<'a> {
    pub(crate) fn new(
        client: harness_provider_kimi::KimiAcpClient,
        mut on_provider_request: impl FnMut(&serde_json::Value) -> crate::CliResult<serde_json::Value>
            + 'a,
        mut on_provider_request_written: impl FnMut(&serde_json::Value) -> crate::CliResult<()> + 'a,
    ) -> Self {
        Self(harness_provider_kimi::KimiTeamRuntime::new(
            client,
            move |request| on_provider_request(request).map_err(callback_error),
            move |request| on_provider_request_written(request).map_err(callback_error),
        ))
    }
}

impl rt::TeamRuntimeAdapter for KimiTeamRuntime<'_> {
    type Error = crate::CliError;

    fn provider(&self) -> &'static str {
        rt::TeamRuntimeAdapter::provider(&self.0)
    }

    fn display_name(&self) -> &'static str {
        rt::TeamRuntimeAdapter::display_name(&self.0)
    }

    fn capability_bindings() -> Vec<rt::CapabilityBinding> {
        harness_provider_kimi::KimiTeamRuntime::capability_bindings()
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
        timeouts: rt::CycleTimeouts,
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
            timeouts,
            &mut |receipt| on_input_accepted(receipt).map_err(callback_error),
            &mut |request, result| on_steer_result(request, result).map_err(callback_error),
            on_event,
            poll_control,
        )?)
    }

    fn native_control<'b>(
        close: &'b mut bool,
        interrupt: &'b mut bool,
    ) -> Box<dyn rt::ProviderNativeControl + 'b> {
        harness_provider_kimi::KimiTeamRuntime::native_control(close, interrupt)
    }

    fn supports_inject_current_cycle(&self) -> bool {
        rt::TeamRuntimeAdapter::supports_inject_current_cycle(&self.0)
    }

    fn supports_native_boundary_queue(&self) -> bool {
        rt::TeamRuntimeAdapter::supports_native_boundary_queue(&self.0)
    }
}

impl rt::RuntimeAdapter for KimiTeamRuntime<'_> {
    fn describe(&self) -> &rt::RuntimeDescription {
        rt::RuntimeAdapter::describe(&self.0)
    }

    fn open_or_resume(
        &mut self,
        fence: rt::RuntimeBindingFence,
        native_session_ref: Option<&str>,
    ) -> Result<rt::RuntimeObservation, rt::RuntimeContractError> {
        rt::RuntimeAdapter::open_or_resume(&mut self.0, fence, native_session_ref)
    }

    fn execute_control(
        &mut self,
        fence: rt::RuntimeBindingFence,
        request: rt::ControlRequest,
    ) -> Result<rt::EffectReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::execute_control(&mut self.0, fence, request)
    }

    fn observe(
        &mut self,
        fence: rt::RuntimeBindingFence,
    ) -> Result<rt::RuntimeObservation, rt::RuntimeContractError> {
        rt::RuntimeAdapter::observe(&mut self.0, fence)
    }

    fn inspect_effect(
        &mut self,
        fence: rt::RuntimeBindingFence,
        effect_id: &str,
    ) -> Result<rt::EffectInspection, rt::RuntimeContractError> {
        rt::RuntimeAdapter::inspect_effect(&mut self.0, fence, effect_id)
    }

    fn reconcile(
        &mut self,
        fence: rt::RuntimeBindingFence,
        inspection: &rt::EffectInspection,
    ) -> Result<rt::ReconcileReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::reconcile(&mut self.0, fence, inspection)
    }

    fn close_runtime(
        &mut self,
        fence: rt::RuntimeBindingFence,
    ) -> Result<rt::MemberRuntimeCloseReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::close_runtime(&mut self.0, fence)
    }

    fn quiesce(
        &mut self,
        fence: rt::RuntimeBindingFence,
    ) -> Result<rt::QuiesceReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::quiesce(&mut self.0, fence)
    }

    fn release(
        &mut self,
        fence: rt::RuntimeBindingFence,
    ) -> Result<rt::ReleaseReceipt, rt::RuntimeContractError> {
        rt::RuntimeAdapter::release(&mut self.0, fence)
    }
}
