//! TLS setup following libpq `sslmode` semantics, on rustls with the `ring` provider.

use std::path::Path;
use std::sync::Arc;

use platypus_core::{ConnectionConfig, Error, Result, SslMode};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use tokio_postgres::config::SslMode as PgSslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

/// The protocol-level mode for tokio-postgres plus the connector, if TLS may be used.
pub fn connector(config: &ConnectionConfig) -> Result<(PgSslMode, Option<MakeRustlsConnect>)> {
    let mode = match config.ssl_mode {
        SslMode::Disable => PgSslMode::Disable,
        SslMode::Prefer => PgSslMode::Prefer,
        SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull => PgSslMode::Require,
    };
    Ok((mode, client_config(config)?.map(MakeRustlsConnect::new)))
}

/// The rustls client configuration for `config.ssl_mode`; `None` when TLS is disabled.
/// `prefer` and `require` do not verify the certificate, as in libpq. Also used by the
/// libSQL driver.
pub fn client_config(config: &ConnectionConfig) -> Result<Option<ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier: Arc<dyn ServerCertVerifier> = match config.ssl_mode {
        SslMode::Disable => return Ok(None),
        SslMode::Prefer | SslMode::Require => Arc::new(NoVerification(Arc::clone(&provider))),
        SslMode::VerifyCa => Arc::new(IgnoreHostname(chain_verifier(
            config.ssl_root_cert.as_deref(),
            &provider,
        )?)),
        SslMode::VerifyFull => chain_verifier(config.ssl_root_cert.as_deref(), &provider)?,
    };
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(verifier);
    let tls = match client_identity(config)? {
        Some((certs, key)) => builder
            .with_client_auth_cert(certs, key)
            .map_err(|e| Error::Config(format!("invalid client certificate or key: {e}")))?,
        None => builder.with_no_client_auth(),
    };
    Ok(Some(tls))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn tls_error(e: rustls::Error) -> Error {
    Error::Config(format!("TLS setup failed: {e}"))
}

/// The client certificate chain and private key (PEM), when both are set.
fn client_identity(
    config: &ConnectionConfig,
) -> Result<Option<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>> {
    let (cert, key) = match (&config.ssl_client_cert, &config.ssl_client_key) {
        (Some(cert), Some(key)) => (cert, key),
        (None, None) => return Ok(None),
        _ => {
            return Err(Error::Config(
                "a client certificate needs both the certificate and its key".to_owned(),
            ));
        }
    };
    let unreadable = |path: &Path, e: &dyn std::fmt::Display| {
        Error::Config(format!("cannot read {}: {e}", path.display()))
    };
    let certs = CertificateDer::pem_file_iter(cert)
        .map_err(|e| unreadable(cert, &e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| unreadable(cert, &e))?;
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| unreadable(key, &e))?;
    Ok(Some((certs, key)))
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

#[cfg(test)]
mod tests {
    use platypus_core::DriverKind;

    use super::*;

    fn error_of(config: &ConnectionConfig) -> String {
        match connector(config) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn client_certificate_needs_cert_and_key() {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.ssl_mode = SslMode::Require;
        assert!(connector(&config).is_ok());
        config.ssl_client_cert = Some("client.crt".into());
        assert!(error_of(&config).contains("both the certificate and its key"));
        config.ssl_client_key = Some("client.key".into());
        assert!(error_of(&config).contains("client.crt"));
    }
}
