use super::*;


pub(super) struct HttpRequest {
    pub(super) stream: TcpStream,
    pub(super) method: String,
    pub(super) target: String,
    pub(super) headers: std::collections::BTreeMap<String, String>,
    pub(super) body: Vec<u8>,
}

pub(super) const STANDARD_FABRIC_HTTP_BODY_LIMIT: usize = 1024 * 1024;
pub(super) const MAX_FABRIC_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
// Artifact completion uses a closed JSON envelope with hexadecimal content.
// Reserve bounded space for the signed capability and JSON framing without
// widening any other Host REST endpoint beyond the normal 1 MiB limit.
pub(super) const ARTIFACT_COMPLETE_HTTP_BODY_LIMIT: usize = MAX_FABRIC_ARTIFACT_BYTES * 2 + 256 * 1024;

pub(super) fn is_artifact_complete_path(path: &str) -> bool {
    path.strip_prefix("/v1/fabric/artifacts/")
        .and_then(|rest| rest.strip_suffix("/complete"))
        .is_some_and(|artifact_id| !artifact_id.is_empty() && !artifact_id.contains('/'))
}

pub(super) fn fabric_http_body_limit(method: &str, target: &str) -> usize {
    let path = target.split('?').next().unwrap_or_default();
    if method == "POST" && is_artifact_complete_path(path) {
        ARTIFACT_COMPLETE_HTTP_BODY_LIMIT
    } else {
        STANDARD_FABRIC_HTTP_BODY_LIMIT
    }
}

pub(super) fn authorized_fabric_http_body_limit(
    method: &str,
    target: &str,
    headers: &std::collections::BTreeMap<String, String>,
    trusted_origin: &str,
    host_token: &str,
) -> usize {
    let requested_limit = fabric_http_body_limit(method, target);
    let large_body_authorized = requested_limit > STANDARD_FABRIC_HTTP_BODY_LIMIT
        && headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|presented| constant_time_secret_eq(presented, host_token))
        && headers
            .get("origin")
            .is_none_or(|origin| origin == trusted_origin);
    if large_body_authorized {
        requested_limit
    } else {
        STANDARD_FABRIC_HTTP_BODY_LIMIT
    }
}

pub(super) fn read_http_request(
    mut stream: TcpStream,
    trusted_origin: &str,
    host_token: &str,
) -> CliResult<HttpRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    if !matches!(method.as_str(), "GET" | "POST" | "OPTIONS")
        || (!target.starts_with("/v1/fabric/") && !target.starts_with("/v1/collaboration/"))
    {
        write_http_json(
            &mut stream,
            "404 Not Found",
            &serde_json::json!({"ok":false,"error":"unknown_fabric_endpoint"}),
            None,
        )?;
        return Err(CliError::Usage("unknown Remote Fabric endpoint".into()));
    }
    let mut headers = std::collections::BTreeMap::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| CliError::Usage("malformed HTTP header".into()))?;
        let name = name.trim().to_ascii_lowercase();
        if headers.insert(name.clone(), value.trim().into()).is_some() {
            return Err(CliError::Usage("duplicate HTTP header".into()));
        }
        if name == "content-length" {
            content_length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| CliError::Usage("invalid Content-Length".into()))?;
        }
    }
    let body_limit =
        authorized_fabric_http_body_limit(&method, &target, &headers, trusted_origin, host_token);
    if content_length > body_limit {
        return Err(CliError::Usage(format!(
            "Remote Fabric REST body exceeds endpoint limit of {body_limit} bytes"
        )));
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(HttpRequest {
        stream,
        method,
        target,
        headers,
        body,
    })
}

pub(super) fn handle_host_http<K: harness_fabric::ArtifactKeyBackend>(
    mut request: HttpRequest,
    trusted_origin: &str,
    host_token: &str,
    control: &ControlPlane<'_, K>,
    generation: u64,
    ca: &harness_fabric::pki::FabricCaMaterial,
    collaboration_root: &Path,
) -> CliResult<()> {
    let path = request.target.split('?').next().unwrap_or_default();
    if request.method == "OPTIONS" {
        let origin = request.headers.get("origin").map(String::as_str);
        if origin != Some(trusted_origin) {
            write_http_json(
                &mut request.stream,
                "403 Forbidden",
                &serde_json::json!({"ok":false,"error":"untrusted_origin"}),
                None,
            )?;
        } else {
            write_http_json(
                &mut request.stream,
                "200 OK",
                &serde_json::json!({"ok":true}),
                origin,
            )?;
        }
        return Ok(());
    }
    let origin = request.headers.get("origin").map(String::as_str);
    if origin.is_some_and(|origin| origin != trusted_origin) {
        return respond_fabric_error(
            &mut request.stream,
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "browser origin is not the configured trusted Control Plane origin",
            ),
            None,
        );
    }
    if path.starts_with("/v1/collaboration/") {
        let result = handle_collaboration_control_plane_http(
            &request.method,
            path,
            &request.target,
            &request.headers,
            &request.body,
            control,
            generation,
            collaboration_root,
        );
        return match result {
            Ok(value) => write_http_json(&mut request.stream, "200 OK", &value, origin),
            Err(error) => respond_fabric_error(&mut request.stream, error, origin),
        };
    }
    let node_enroll = request.method == "POST" && path == "/v1/fabric/nodes/enroll";
    let actor = if node_enroll {
        None
    } else {
        let presented = request
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "));
        if !presented.is_some_and(|presented| constant_time_secret_eq(presented, host_token))
            || request.headers.keys().any(|name| {
                matches!(
                    name.as_str(),
                    "x-agentfirm-actor-id" | "x-agentfirm-actor-kind" | "x-agentfirm-authority-id"
                )
            })
        {
            return respond_fabric_error(
                &mut request.stream,
                FabricError::none(
                    FabricErrorCode::UnauthorizedActor,
                    "Company-issued Host credential is missing or request attempted identity selection",
                ),
                origin,
            );
        }
        Some(AuthenticatedActor {
            company_id: control.company_id().into(),
            actor_id: "company-host:http".into(),
            actor_kind: harness_fabric::ActorKind::Human,
            role_bindings: std::collections::BTreeSet::from([
                "company_host".into(),
                "artifact_write".into(),
                "artifact_read".into(),
            ]),
            session_id: format!("host-http:{generation}"),
            issued_at_unix_ms: now_unix_ms().map_err(fabric_error)?,
            expires_at_unix_ms: now_unix_ms().map_err(fabric_error)?.saturating_add(30_000),
        })
    };
    let body = if request.body.is_empty() {
        serde_json::Value::Null
    } else {
        match serde_json::from_slice(&request.body) {
            Ok(value) => value,
            Err(error) => {
                return respond_fabric_error(
                    &mut request.stream,
                    FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()),
                    origin,
                )
            }
        }
    };
    let now = now_unix_ms().map_err(fabric_error)?;
    let result = route_host_http(
        &request.method,
        path,
        &request.target,
        &body,
        actor.as_ref(),
        control,
        generation,
        ca,
        now,
        host_token,
    );
    match result {
        Ok(value) => write_http_json(&mut request.stream, "200 OK", &value, origin),
        Err(error) => respond_fabric_error(&mut request.stream, error, origin),
    }
}
