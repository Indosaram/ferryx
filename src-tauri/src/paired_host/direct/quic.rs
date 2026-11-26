use super::cert::{EphemeralCert, PinnedClientCertVerifier, PinnedServerCertVerifier};
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, Endpoint, EndpointConfig, ServerConfig, TokioRuntime, TransportConfig, VarInt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::version::TLS13;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

pub const DIRECT_ALPN: &[u8] = b"ferryx-direct-v1";
pub const DIRECT_DATAGRAM_BUFFER_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum DirectTransportError {
    #[error("I/O error during QUIC operation: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS configuration error: {0}")]
    Tls(#[from] rustls::Error),
    #[error("QUIC configuration error: {0}")]
    Config(String),
}

pub fn make_transport_config() -> Result<Arc<TransportConfig>, DirectTransportError> {
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(15)));
    let idle_timeout = Duration::from_secs(20)
        .try_into()
        .map_err(|e| DirectTransportError::Config(format!("{e}")))?;
    transport.max_idle_timeout(Some(idle_timeout));
    transport.max_concurrent_bidi_streams(VarInt::from_u32(64));
    transport.max_concurrent_uni_streams(VarInt::from_u32(0));
    // Remote-desktop media rides unreliable datagrams. The ALPN stays `ferryx-direct-v1`:
    // an older peer simply advertises no datagram support and senders fall back to streams.
    transport.datagram_receive_buffer_size(Some(DIRECT_DATAGRAM_BUFFER_BYTES));
    transport.datagram_send_buffer_size(DIRECT_DATAGRAM_BUFFER_BYTES);
    transport.mtu_discovery_config(Some(quinn::MtuDiscoveryConfig::default()));
    Ok(Arc::new(transport))
}

pub fn make_client_config(
    local_cert: &EphemeralCert,
    expected_peer_cert_der: &[u8],
) -> Result<ClientConfig, DirectTransportError> {
    let verifier = Arc::new(PinnedServerCertVerifier::new(expected_peer_cert_der.to_vec()));
    let cert_chain = vec![CertificateDer::from(local_cert.cert_der.clone())];
    let key = PrivateKeyDer::Pkcs8(local_cert.key_der.clone().into());

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut rustls_client_config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS13])
        .map_err(DirectTransportError::Tls)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(cert_chain, key)?;

    rustls_client_config.alpn_protocols = vec![DIRECT_ALPN.to_vec()];

    let quic_client_config = QuicClientConfig::try_from(Arc::new(rustls_client_config))
        .map_err(|e| DirectTransportError::Config(format!("{e:?}")))?;

    let mut client_config = ClientConfig::new(Arc::new(quic_client_config));
    client_config.transport_config(make_transport_config()?);
    Ok(client_config)
}

pub fn make_server_config(
    local_cert: &EphemeralCert,
    expected_peer_cert_der: &[u8],
) -> Result<ServerConfig, DirectTransportError> {
    let verifier = PinnedClientCertVerifier::new(expected_peer_cert_der.to_vec());
    let cert_chain = vec![CertificateDer::from(local_cert.cert_der.clone())];
    let key = PrivateKeyDer::Pkcs8(local_cert.key_der.clone().into());

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut rustls_server_config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS13])
        .map_err(DirectTransportError::Tls)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(cert_chain, key)?;

    rustls_server_config.alpn_protocols = vec![DIRECT_ALPN.to_vec()];

    let quic_server_config = QuicServerConfig::try_from(Arc::new(rustls_server_config))
        .map_err(|e| DirectTransportError::Config(format!("{e:?}")))?;

    let mut server_config = ServerConfig::with_crypto(Arc::new(quic_server_config));
    server_config.transport_config(make_transport_config()?);
    Ok(server_config)
}

pub fn create_quic_endpoint(
    std_socket: std::net::UdpSocket,
    local_cert: &EphemeralCert,
    expected_peer_cert_der: &[u8],
    is_server: bool,
) -> Result<Endpoint, DirectTransportError> {
    std_socket.set_nonblocking(true)?;

    let server_config = if is_server {
        Some(make_server_config(local_cert, expected_peer_cert_der)?)
    } else {
        None
    };

    let runtime = Arc::new(TokioRuntime);
    let mut endpoint = Endpoint::new(
        EndpointConfig::default(),
        server_config,
        std_socket,
        runtime,
    )?;

    let client_config = make_client_config(local_cert, expected_peer_cert_der)?;
    endpoint.set_default_client_config(client_config);

    Ok(endpoint)
}
