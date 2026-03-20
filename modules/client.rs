use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;

/// Build an HTTP client that accepts self-signed certs.
pub fn build_client() -> Client {
    build_client_with_pool(100)
}

/// Build an HTTP client with a configurable connection pool size.
pub fn build_client_with_pool(pool_max_idle: usize) -> Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Client::builder()
        .danger_accept_invalid_certs(true)
        // Force HTTP/1.1 so each VU gets its own TCP connection.
        // With HTTP/2, all VUs multiplex over one connection and a
        // TCP load balancer (HAProxy) sends everything to one backend.
        .http1_only()
        .pool_max_idle_per_host(pool_max_idle)
        .timeout(Duration::from_secs(5))
        .build()
        .expect("failed to build reqwest client")
}

/// Build an HTTP client without request timeout for long-lived streaming connections (SSE).
pub fn build_streaming_client() -> Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Client::builder()
        .danger_accept_invalid_certs(true)
        .pool_max_idle_per_host(100)
        .build()
        .expect("failed to build streaming reqwest client")
}

/// Build a WebSocket TLS connector that accepts self-signed certs.
pub fn build_ws_connector() -> tokio_tungstenite::Connector {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertVerifier))
        .with_no_client_auth();
    // Enable TLS session resumption — reuse sessions for faster reconnects
    config.resumption = rustls::client::Resumption::in_memory_sessions(65536);
    tokio_tungstenite::Connector::Rustls(Arc::new(config))
}

/// Rustls certificate verifier that accepts all certs (for dev/benchmark use).
#[derive(Debug)]
struct NoCertVerifier;

impl rustls::client::danger::ServerCertVerifier for NoCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
