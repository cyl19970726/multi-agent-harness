use super::*;

pub(super) struct HttpExchange<'a> {
    pub(super) projects: &'a ServeProjects,
    pub(super) stream: &'a mut HttpResponseWriter<TcpStream>,
    pub(super) sse_manager: sse::SseManager,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) path_only: String,
    pub(super) project_param: Option<String>,
    /// Identity of the coordination store this request resolved to, never a
    /// Project Binding. `?space=<id>` selects it; in compatibility mode only,
    /// a legacy `?project=<id>` store id may still resolve one. Either way the
    /// resolved id IS that store's own Execution Space identity, which is why
    /// it may be read as an `ExecutionSpaceId`. Provider cwd comes from
    /// `project_param` instead -- selecting a project never switches the store.
    pub(super) coordination_store_id: String,
    pub(super) store: HarnessStore,
    pub(super) company_os_path: bool,
    pub(super) body: Vec<u8>,
    pub(super) trust_transport_token: Option<String>,
    pub(super) trust_idempotency_key: Option<String>,
    pub(super) trust_expected_version: Option<u64>,
    pub(super) trust_confirmed_action: Option<String>,
    pub(super) trust_identity_override_header: bool,
    pub(super) native_session_wake_token: Option<String>,
}
