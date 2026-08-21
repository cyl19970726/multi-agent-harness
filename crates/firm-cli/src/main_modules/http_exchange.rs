use super::*;

pub(super) struct HttpExchange<'a> {
    pub(super) projects: &'a ServeProjects,
    pub(super) stream: &'a mut TcpStream,
    pub(super) sse_manager: sse::SseManager,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) path_only: String,
    pub(super) project_param: Option<String>,
    pub(super) project_id: String,
    pub(super) store: HarnessStore,
    pub(super) company_os_path: bool,
    pub(super) body: Vec<u8>,
    pub(super) trust_transport_token: Option<String>,
    pub(super) trust_idempotency_key: Option<String>,
    pub(super) trust_expected_version: Option<u64>,
    pub(super) trust_confirmed_action: Option<String>,
    pub(super) trust_identity_override_header: bool,
    pub(super) live_provider_activity_token: Option<String>,
}
