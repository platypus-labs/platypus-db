use crate::{
    client::{config::Config, TrustConfig},
    error::IoErrorKind,
    Error,
};
use futures_util::io::{AsyncRead, AsyncWrite};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{ring, CryptoProvider},
    pki_types::{pem::PemObject, CertificateDer, ServerName, UnixTime},
    ClientConfig, DigitallySignedStruct, Error as RustlsError, RootCertStore, SignatureScheme,
};
use std::{
    fs, io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio_rustls::TlsConnector;
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use tracing::{event, Level};

impl From<RustlsError> for Error {
    fn from(e: RustlsError) -> Self {
        crate::Error::Tls(e.to_string())
    }
}

pub(crate) struct TlsStream<S: AsyncRead + AsyncWrite + Unpin + Send>(
    Compat<tokio_rustls::client::TlsStream<Compat<S>>>,
);

/// Accepts any server certificate (`TrustServerCertificate=true`), as tiberius always did.
/// The handshake signatures are not checked either: checking them parses the certificate with
/// webpki, which refuses the X.509 v1 certificate SQL Server generates for itself.
#[derive(Debug)]
struct NoCertVerifier(Arc<CryptoProvider>);

impl ServerCertVerifier for NoCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn get_server_name(config: &Config) -> crate::Result<ServerName<'static>> {
    match (
        ServerName::try_from(config.get_host().to_owned()),
        &config.trust,
    ) {
        (Ok(sn), _) => Ok(sn),
        (Err(_), TrustConfig::TrustAll) => Ok(ServerName::try_from("placeholder.domain.com")
            .map_err(|e| crate::Error::Tls(e.to_string()))?),
        (Err(e), _) => Err(crate::Error::Tls(e.to_string())),
    }
}

/// The certificates of a CA file: every certificate of a PEM file (a chain may hold several),
/// or the one certificate of a DER file.
fn ca_certificates(path: &std::path::Path) -> crate::Result<Vec<CertificateDer<'static>>> {
    let Ok(buf) = fs::read(path) else {
        return Err(Error::Io {
            kind: IoErrorKind::InvalidData,
            message: "Could not read provided CA certificate!".to_string(),
        });
    };
    let certs = match path.extension() {
        Some(ext)
            if ext.to_ascii_lowercase() == "pem" || ext.to_ascii_lowercase() == "crt" =>
        {
            CertificateDer::pem_slice_iter(&buf)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| Error::Io {
                    kind: IoErrorKind::InvalidData,
                    message: format!("Could not read the certificates of {}: {e}", path.to_string_lossy()),
                })?
        }
        Some(ext) if ext.to_ascii_lowercase() == "der" => vec![CertificateDer::from(buf)],
        Some(_) | None => return Err(crate::Error::Io {
            kind: IoErrorKind::InvalidInput,
            message: "Provided CA certificate with unsupported file-extension! Supported types are pem, crt and der.".to_string(),
        }),
    };
    if certs.is_empty() {
        return Err(crate::Error::Io {
            kind: IoErrorKind::InvalidInput,
            message: format!(
                "Certificate file {} contains no certificate",
                path.to_string_lossy()
            ),
        });
    }
    Ok(certs)
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> TlsStream<S> {
    pub(super) async fn new(config: &Config, stream: S) -> crate::Result<Self> {
        event!(Level::INFO, "Performing a TLS handshake");

        let provider = Arc::new(ring::default_provider());
        let builder = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()?;

        let client_config = match &config.trust {
            TrustConfig::CaCertificateLocation(path) => {
                let mut cert_store = RootCertStore::empty();
                for cert in ca_certificates(path)? {
                    cert_store.add(cert)?;
                }
                builder
                    .with_root_certificates(cert_store)
                    .with_no_client_auth()
            }
            TrustConfig::TrustAll => {
                event!(
                    Level::WARN,
                    "Trusting the server certificate without validation."
                );
                builder
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(NoCertVerifier(provider)))
                    .with_no_client_auth()
            }
            TrustConfig::Default => {
                event!(Level::INFO, "Using default trust configuration.");
                builder
                    .with_root_certificates(native_roots()?)
                    .with_no_client_auth()
            }
        };

        let connector = TlsConnector::from(Arc::new(client_config));

        let tls_stream = connector
            .connect(get_server_name(config)?, stream.compat())
            .await?;

        Ok(TlsStream(tls_stream.compat()))
    }

    pub(crate) fn get_mut(&mut self) -> &mut S {
        self.0.get_mut().get_mut().0.get_mut()
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> AsyncRead for TlsStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let inner = Pin::get_mut(self);
        Pin::new(&mut inner.0).poll_read(cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> AsyncWrite for TlsStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let inner = Pin::get_mut(self);
        Pin::new(&mut inner.0).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let inner = Pin::get_mut(self);
        Pin::new(&mut inner.0).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let inner = Pin::get_mut(self);
        Pin::new(&mut inner.0).poll_close(cx)
    }
}

/// The platform's CA certificates. PlatypusDB: an error instead of a panic when the platform
/// has none.
fn native_roots() -> crate::Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    let loaded = rustls_native_certs::load_native_certs();
    for err in &loaded.errors {
        tracing::event!(Level::DEBUG, "could not load a platform certificate: {err}");
    }
    let (valid, invalid) = roots.add_parsable_certificates(loaded.certs);
    tracing::event!(
        Level::TRACE,
        "native roots: {} valid and {} invalid certs",
        valid,
        invalid
    );
    if roots.is_empty() {
        return Err(Error::Tls("no CA certificates found".to_owned()));
    }
    Ok(roots)
}
