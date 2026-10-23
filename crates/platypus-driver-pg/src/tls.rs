//! TLS setup following libpq `sslmode` semantics, on rustls with the `ring` provider.

use std::path::Path;
use std::sync::Arc;

use platypus_core::{ConnectionConfig, Error, Result, SslMode};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use tokio_postgres::config::SslMode as PgSslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

/// The protocol-level mode for tokio-postgres plus the connector, if TLS may be used.
pub fn connector(config: &ConnectionConfig) -> Result<(PgSslMode, Option<MakeRustlsConnect>)> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let (mode, verifier): (_, Arc<dyn ServerCertVerifier>) = match config.ssl_mode {
        SslMode::Disable => return Ok((PgSslMode::Disable, None)),
        // libpq does not verify certificates in these modes.
        SslMode::Prefer => (
            PgSslMode::Prefer,
            Arc::new(NoVerification(Arc::clone(&provider))),
        ),
        SslMode::Require => (
            PgSslMode::Require,
            Arc::new(NoVerification(Arc::clone(&provider))),
        ),
        SslMode::VerifyCa => (
            PgSslMode::Require,
            Arc::new(IgnoreHostname(chain_verifier(
                config.ssl_root_cert.as_deref(),
                &provider,
            )?)),
        ),
        SslMode::VerifyFull => (
            PgSslMode::Require,
            chain_verifier(config.ssl_root_cert.as_deref(), &provider)?,
        ),
    };
    let tls = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok((mode, Some(MakeRustlsConnect::new(tls))))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn tls_error(e: rustls::Error) -> Error {
    Error::Config(format!("TLS setup failed: {e}"))
}

/// Verifies against `root_cert` (PEM) when given, otherwise against the OS trust store.
fn chain_verifier(
    root_cert: Option<&Path>,
    provider: &Arc<CryptoProvider>,
) -> Result<Arc<dyn ServerCertVerifier>> {
    let Some(path) = root_cert else {
        let verifier =
            rustls_platform_verifier::Verifier::new(Arc::clone(provider)).map_err(tls_error)?;
        return Ok(Arc::new(verifier));
    };
    let mut roots = RootCertStore::empty();
    let certs = CertificateDer::pem_file_iter(path)
        .map_err(|e| Error::Config(format!("cannot read {}: {e}", path.display())))?;
    for cert in certs {
        let cert = cert.map_err(|e| Error::Config(format!("invalid certificate: {e}")))?;
        roots.add(cert).map_err(tls_error)?;
    }
    WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(provider))
        .build()
        .map(|v| v as Arc<dyn ServerCertVerifier>)
        .map_err(|e| Error::Config(format!("invalid root certificates: {e}")))
}

/// Accepts any certificate but still checks handshake signatures (`sslmode=require`).
#[derive(Debug)]
struct NoVerification(Arc<CryptoProvider>);

impl ServerCertVerifier for NoVerification {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Verifies the chain but not the host name (`sslmode=verify-ca`).
#[derive(Debug)]
struct IgnoreHostname(Arc<dyn ServerCertVerifier>);

impl ServerCertVerifier for IgnoreHostname {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self
            .0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
        {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}
