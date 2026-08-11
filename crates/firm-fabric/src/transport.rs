use crate::protocol::{FabricError, FabricErrorCode, FabricFrame};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{ServerConnection, StreamOwned};
use std::fs::File;
use std::io::BufReader;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, WebSocket};

pub const MAX_FABRIC_FRAME_BYTES: usize = 256 * 1024;
pub const FABRIC_WEBSOCKET_SUBPROTOCOL: &str = "agentfirm.node.v1";
pub const FABRIC_GATEWAY_PATH: &str = "/v1/node-gateway/connect";
pub const GATEWAY_HEARTBEAT_INTERVAL_MS: u64 = 10_000;
pub const GATEWAY_LEASE_DURATION_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMtlsPeer {
    pub company_id: String,
    pub node_id: String,
    pub certificate_serial: String,
    pub public_key_fingerprint: String,
    pub tls_version: String,
    pub websocket_subprotocol: String,
}

impl VerifiedMtlsPeer {
    pub fn validate_node_hello(
        &self,
        hello: &crate::protocol::NodeHello,
    ) -> Result<(), FabricError> {
        if self.tls_version != "TLS1.3"
            || self.websocket_subprotocol != FABRIC_WEBSOCKET_SUBPROTOCOL
        {
            return Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "Remote Fabric requires verified mutual TLS 1.3 and agentfirm.node.v1",
            ));
        }
        if self.company_id != hello.company_id
            || self.node_id != hello.node_id
            || self.certificate_serial != hello.certificate_serial
            || self.public_key_fingerprint != hello.public_key_fingerprint
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "NodeHello identity does not match the verified mTLS peer certificate",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFabricConfig {
    pub company_id: String,
    pub node_id: String,
    pub control_plane_url: String,
    pub reconnect_floor_ms: u64,
    pub reconnect_ceiling_ms: u64,
}

/// File-backed TLS material selected by the Node credential-store adapter.
/// Production macOS code exports only short-lived PEM handles from Keychain;
/// private key bytes are never serialized into Fabric frames or journals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeTlsIdentityFiles {
    pub client_certificate_chain_pem: PathBuf,
    pub client_private_key_pem: PathBuf,
    pub control_plane_ca_pem: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPlaneTlsFiles {
    pub server_certificate_chain_pem: PathBuf,
    pub server_private_key_pem: PathBuf,
    pub node_ca_pem: PathBuf,
}

impl ControlPlaneTlsFiles {
    pub fn validate(&self) -> Result<(), FabricError> {
        NodeTlsIdentityFiles {
            client_certificate_chain_pem: self.server_certificate_chain_pem.clone(),
            client_private_key_pem: self.server_private_key_pem.clone(),
            control_plane_ca_pem: self.node_ca_pem.clone(),
        }
        .validate()
    }
}

impl NodeTlsIdentityFiles {
    pub fn validate(&self) -> Result<(), FabricError> {
        for (label, path) in [
            ("client certificate", &self.client_certificate_chain_pem),
            ("client private key", &self.client_private_key_pem),
            ("Control Plane CA", &self.control_plane_ca_pem),
        ] {
            let metadata = std::fs::symlink_metadata(path)
                .map_err(|error| transport_error(format!("{label} is unavailable: {error}")))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(transport_error(format!(
                    "{label} must be a regular non-symlink file"
                )));
            }
        }
        Ok(())
    }
}

pub type NodeGatewaySocket = WebSocket<MaybeTlsStream<TcpStream>>;
pub type ControlPlaneGatewaySocket = WebSocket<StreamOwned<ServerConnection, TcpStream>>;

/// Establish the only permitted collaboration connection: an outbound WSS
/// socket authenticated with TLS 1.3, exact hostname verification, a client
/// certificate and the frozen WebSocket subprotocol.
pub fn connect_outbound_mtls(
    config: &NodeFabricConfig,
    identity: &NodeTlsIdentityFiles,
) -> Result<NodeGatewaySocket, FabricError> {
    config.validate()?;
    identity.validate()?;
    let mut roots = rustls::RootCertStore::empty();
    for certificate in read_certificates(&identity.control_plane_ca_pem)? {
        roots
            .add(certificate)
            .map_err(|error| transport_error(format!("Control Plane CA is invalid: {error}")))?;
    }
    if roots.is_empty() {
        return Err(transport_error("Control Plane CA contains no certificate"));
    }
    let certificates = read_certificates(&identity.client_certificate_chain_pem)?;
    if certificates.is_empty() {
        return Err(transport_error("client certificate chain is empty"));
    }
    let private_key = read_private_key(&identity.client_private_key_pem)?;
    let tls = rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .with_root_certificates(roots)
        .with_client_auth_cert(certificates, private_key)
        .map_err(|error| transport_error(format!("client TLS identity is invalid: {error}")))?;

    let mut request = config
        .control_plane_url
        .as_str()
        .into_client_request()
        .map_err(|error| transport_error(format!("gateway URL is invalid: {error}")))?;
    request.headers_mut().insert(
        SEC_WEBSOCKET_PROTOCOL,
        tungstenite::http::HeaderValue::from_static(FABRIC_WEBSOCKET_SUBPROTOCOL),
    );
    let uri = request.uri();
    let host = uri
        .host()
        .ok_or_else(|| transport_error("gateway URL has no hostname"))?;
    let port = uri.port_u16().unwrap_or(443);
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| transport_error(format!("gateway DNS failed: {error}")))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(transport_error("gateway DNS returned no address"));
    }
    let mut last_error = None;
    let mut connected = None;
    for address in addresses {
        match TcpStream::connect(address) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let tcp = connected.ok_or_else(|| {
        transport_error(format!(
            "gateway TCP connect failed: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "no reachable address".into())
        ))
    })?;
    tcp.set_nodelay(true)
        .map_err(|error| transport_error(format!("gateway TCP setup failed: {error}")))?;
    let (socket, response) = tungstenite::client_tls_with_config(
        request,
        tcp,
        None,
        Some(Connector::Rustls(Arc::new(tls))),
    )
    .map_err(|error| transport_error(format!("mutual TLS WebSocket failed: {error}")))?;
    if response
        .headers()
        .get(SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        != Some(FABRIC_WEBSOCKET_SUBPROTOCOL)
    {
        return Err(FabricError::none(
            FabricErrorCode::ProtocolIncompatible,
            "Control Plane did not negotiate agentfirm.node.v1",
        ));
    }
    Ok(socket)
}

/// Accept one Control Plane-side WSS connection. TLS client authentication is
/// completed before the HTTP upgrade; the returned peer identity comes only
/// from the verified certificate, never from NodeHello JSON.
pub fn accept_control_plane_mtls(
    tcp: TcpStream,
    identity: &ControlPlaneTlsFiles,
) -> Result<(ControlPlaneGatewaySocket, VerifiedMtlsPeer), FabricError> {
    identity.validate()?;
    let mut roots = rustls::RootCertStore::empty();
    for certificate in read_certificates(&identity.node_ca_pem)? {
        roots
            .add(certificate)
            .map_err(|error| transport_error(format!("Node client CA is invalid: {error}")))?;
    }
    if roots.is_empty() {
        return Err(transport_error("Node client CA contains no certificate"));
    }
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|error| transport_error(format!("Node client verifier is invalid: {error}")))?;
    let server = rustls::ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            read_certificates(&identity.server_certificate_chain_pem)?,
            read_private_key(&identity.server_private_key_pem)?,
        )
        .map_err(|error| {
            transport_error(format!("Control Plane TLS identity is invalid: {error}"))
        })?;
    let connection = ServerConnection::new(Arc::new(server))
        .map_err(|error| transport_error(format!("Control Plane TLS setup failed: {error}")))?;
    let tls = StreamOwned::new(connection, tcp);
    let socket = tungstenite::accept_hdr(
        tls,
        |request: &tungstenite::handshake::server::Request,
         mut response: tungstenite::handshake::server::Response| {
            if request.uri().path() != FABRIC_GATEWAY_PATH
                || request
                    .headers()
                    .get(SEC_WEBSOCKET_PROTOCOL)
                    .and_then(|value| value.to_str().ok())
                    != Some(FABRIC_WEBSOCKET_SUBPROTOCOL)
            {
                let rejection = tungstenite::http::Response::builder()
                    .status(tungstenite::http::StatusCode::BAD_REQUEST)
                    .body(Some("Remote Fabric path/subprotocol mismatch".into()))
                    .expect("static rejection response");
                return Err(rejection);
            }
            response.headers_mut().insert(
                SEC_WEBSOCKET_PROTOCOL,
                tungstenite::http::HeaderValue::from_static(FABRIC_WEBSOCKET_SUBPROTOCOL),
            );
            Ok(response)
        },
    )
    .map_err(|error| transport_error(format!("Control Plane WebSocket upgrade failed: {error}")))?;
    let certificate = socket
        .get_ref()
        .conn
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| transport_error("verified Node client certificate is unavailable"))?;
    if socket.get_ref().conn.protocol_version() != Some(rustls::ProtocolVersion::TLSv1_3) {
        return Err(FabricError::none(
            FabricErrorCode::ProtocolIncompatible,
            "Remote Fabric negotiated a TLS version other than TLS 1.3",
        ));
    }
    let identity = crate::pki::parse_peer_node_identity(certificate)?;
    Ok((
        socket,
        VerifiedMtlsPeer {
            company_id: identity.company_id,
            node_id: identity.node_id,
            certificate_serial: identity.certificate_serial,
            public_key_fingerprint: identity.public_key_fingerprint,
            tls_version: "TLS1.3".into(),
            websocket_subprotocol: FABRIC_WEBSOCKET_SUBPROTOCOL.into(),
        },
    ))
}

fn read_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, FabricError> {
    let file = File::open(path)
        .map_err(|error| transport_error(format!("TLS certificate open failed: {error}")))?;
    rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| transport_error(format!("TLS certificate parse failed: {error}")))
}

fn read_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, FabricError> {
    let file = File::open(path)
        .map_err(|error| transport_error(format!("TLS private key open failed: {error}")))?;
    rustls_pemfile::private_key(&mut BufReader::new(file))
        .map_err(|error| transport_error(format!("TLS private key parse failed: {error}")))?
        .ok_or_else(|| transport_error("TLS private key file contains no supported key"))
}

fn transport_error(message: impl Into<String>) -> FabricError {
    FabricError::none(FabricErrorCode::StoreUnavailable, message)
}

impl NodeFabricConfig {
    pub fn validate(&self) -> Result<(), FabricError> {
        if self.company_id.trim().is_empty() || self.node_id.trim().is_empty() {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "Remote Fabric config requires exact Company and Node identity",
            ));
        }
        let endpoint = self
            .control_plane_url
            .strip_prefix("wss://")
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ProtocolIncompatible,
                    "Node Fabric permits outbound wss:// Control Plane connections only",
                )
            })?;
        if endpoint.is_empty()
            || endpoint.starts_with('/')
            || endpoint.contains('@')
            || endpoint.contains('#')
            || endpoint.contains('?')
        {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "Control Plane endpoint must be an authority/path without credentials, query, or fragment",
            ));
        }
        if !endpoint.ends_with(FABRIC_GATEWAY_PATH) {
            return Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "Control Plane endpoint must use the frozen /v1/node-gateway/connect path",
            ));
        }
        if self.reconnect_floor_ms == 0
            || self.reconnect_ceiling_ms < self.reconnect_floor_ms
            || self.reconnect_ceiling_ms > GATEWAY_LEASE_DURATION_MS
        {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "reconnect backoff must be bounded within the gateway lease duration",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricSessionFence {
    pub company_id: String,
    pub node_id: String,
    pub gateway_generation: u64,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub control_plane_generation: u64,
}

impl FabricSessionFence {
    pub fn validate_frame(&self, frame: &FabricFrame) -> Result<(), FabricError> {
        frame.validate()?;
        if frame.company_id != self.company_id
            || frame.node_id != self.node_id
            || frame.gateway_generation != self.gateway_generation
            || frame.node_daemon_id != self.node_daemon_id
            || frame.node_daemon_generation != self.node_daemon_generation
            || frame.control_plane_generation != self.control_plane_generation
        {
            return Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "FabricFrame does not match the authenticated session fence",
            ));
        }
        Ok(())
    }
}

pub fn encode_frame(frame: &FabricFrame) -> Result<Vec<u8>, FabricError> {
    frame.validate()?;
    let encoded = serde_json::to_vec(frame).map_err(|error| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("FabricFrame encoding failed: {error}"),
        )
    })?;
    if encoded.len() > MAX_FABRIC_FRAME_BYTES {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "FabricFrame exceeds the 256 KiB wire limit; use an artifact capability",
        ));
    }
    Ok(encoded)
}

pub fn decode_frame(bytes: &[u8]) -> Result<FabricFrame, FabricError> {
    if bytes.is_empty() || bytes.len() > MAX_FABRIC_FRAME_BYTES {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "FabricFrame is empty or exceeds the 256 KiB wire limit",
        ));
    }
    let frame: FabricFrame = serde_json::from_slice(bytes).map_err(|error| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("FabricFrame is not valid closed-contract JSON: {error}"),
        )
    })?;
    frame.validate()?;
    Ok(frame)
}

pub fn write_frame<S: std::io::Read + std::io::Write>(
    socket: &mut WebSocket<S>,
    frame: &FabricFrame,
) -> Result<(), FabricError> {
    let encoded = encode_frame(frame)?;
    socket
        .send(tungstenite::Message::Binary(encoded.into()))
        .map_err(|error| transport_error(format!("Fabric frame send failed: {error}")))
}

pub fn read_frame<S: std::io::Read + std::io::Write>(
    socket: &mut WebSocket<S>,
) -> Result<FabricFrame, FabricError> {
    loop {
        let message = socket.read().map_err(|error| match &error {
            tungstenite::Error::Io(io_error)
                if matches!(
                    io_error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                let mut timeout = FabricError::none(
                    FabricErrorCode::TargetOffline,
                    "Fabric frame read timed out without changing connection authority",
                );
                timeout.retryable = true;
                timeout
            }
            _ => transport_error(format!("Fabric frame read failed: {error}")),
        })?;
        match message {
            tungstenite::Message::Binary(bytes) => return decode_frame(&bytes),
            tungstenite::Message::Text(text) => return decode_frame(text.as_bytes()),
            tungstenite::Message::Ping(payload) => socket
                .send(tungstenite::Message::Pong(payload))
                .map_err(|error| transport_error(format!("Fabric pong failed: {error}")))?,
            tungstenite::Message::Pong(_) => {}
            tungstenite::Message::Close(_) => {
                return Err(FabricError::none(
                    FabricErrorCode::TargetOffline,
                    "Fabric peer closed before the next frame",
                ))
            }
            tungstenite::Message::Frame(_) => {
                return Err(FabricError::none(
                    FabricErrorCode::ProtocolIncompatible,
                    "raw WebSocket frames are not an application message",
                ))
            }
        }
    }
}

pub fn set_node_gateway_read_timeout(
    socket: &mut NodeGatewaySocket,
    timeout: Option<std::time::Duration>,
) -> Result<(), FabricError> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
        MaybeTlsStream::Rustls(stream) => stream.get_mut().set_read_timeout(timeout),
        _ => {
            return Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "NodeGateway uses an unsupported TLS backend",
            ))
        }
    }
    .map_err(|error| transport_error(format!("gateway read timeout setup failed: {error}")))
}
