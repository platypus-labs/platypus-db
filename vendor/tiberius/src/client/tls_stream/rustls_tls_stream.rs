use crate::{
    client::{config::Config, TrustConfig},
    error::IoErrorKind,
    Error,
};
use futures_util::io::{AsyncRead, AsyncWrite};
use std::{
    fs, io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::SystemTime,
};
use tokio_rustls::{
    rustls::{
        client::{
            HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
            WantsTransparencyPolicyOrClientCert,
        },
        Certificate, ClientConfig, ConfigBuilder, DigitallySignedStruct, Error as RustlsError,
        RootCertStore, ServerName, WantsVerifier,
    },
    TlsConnector,
};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use tracing::{event, Level};

impl From<tokio_rustls::rustls::Error> for Error {
    fn from(e: tokio_rustls::rustls::Error) -> Self {
        crate::Error::Tls(e.to_string())
    }
}

pub(crate) struct TlsStream<S: AsyncRead + AsyncWrite + Unpin + Send>(
    Compat<tokio_rustls::client::TlsStream<Compat<S>>>,
);

struct NoCertVerifier;

impl ServerCertVerifier for NoCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: SystemTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &Certificate,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }
}

fn get_server_name(config: &Config) -> crate::Result<ServerName> {
    match (ServerName::try_from(config.get_host()), &config.trust) {
        (Ok(sn), _) => Ok(sn),
        (Err(_), TrustConfig::TrustAll) => {
            Ok(ServerName::try_from("placeholder.domain.com").unwrap())
        }
        (Err(e), _) => Err(crate::Error::Tls(e.to_string())),
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> TlsStream<S> {
    pub(super) async fn new(config: &Config, stream: S) -> crate::Result<Self> {
        event!(Level::INFO, "Performing a TLS handshake");

        let builder = ClientConfig::builder().with_safe_defaults();

        let client_config = match &config.trust {
            TrustConfig::CaCertificateLocation(path) => {
                // PlatypusDB: a PEM file may hold a chain of several certificates.
                if let Ok(buf) = fs::read(path) {
                    let certs = match path.extension() {
                        Some(ext)
                            if ext.to_ascii_lowercase() == "pem"
                                || ext.to_ascii_lowercase() == "crt" =>
                        {
                            rustls_pemfile::certs(&mut buf.as_slice())?
                                .into_iter()
                                .map(Certificate)
                                .collect::<Vec<_>>()
                        }
                        Some(ext) if ext.to_ascii_lowercase() == "der" => vec![Certificate(buf)],
                        Some(_) | None => return Err(crate::Error::Io {
                            kind: IoErrorKind::InvalidInput,
                            message: "Provided CA certificate with unsupported file-extension! Supported types are pem, crt and der.".to_string(),
                        }),
                    };
                    if certs.is_empty() {
                        return Err(crate::Error::Io {
                            kind: IoErrorKind::InvalidInput,
                            message: format!("Certificate file {} contains no certificate", path.to_string_lossy()),
                        });
                    }
                    let mut cert_store = RootCertStore::empty();
                    for cert in &certs {
                        cert_store.add(cert)?;
                    }
                    builder
                        .with_root_certificates(cert_store)
                        .with_no_client_auth()
                } else {
                    return Err(Error::Io {
                        kind: IoErrorKind::InvalidData,
                        message: "Could not read provided CA certificate!".to_string(),
                    });
                }
            }
            TrustConfig::TrustAll => {
                event!(
                    Level::WARN,
                    "Trusting the server certificate without validation."
                );
                let mut config = builder
                    .with_root_certificates(RootCertStore::empty())
                    .with_no_client_auth();
                config
                    .dangerous()
                    .set_certificate_verifier(Arc::new(NoCertVerifier {}));
                // config.enable_sni = false;
                config
            }
            TrustConfig::Default => {
                event!(Level::INFO, "Using default trust configuration.");
                builder.with_native_roots()?.with_no_client_auth()
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

trait ConfigBuilderExt {
    fn with_native_roots(
        self,
    ) -> crate::Result<ConfigBuilder<ClientConfig, WantsTransparencyPolicyOrClientCert>>;
}

impl ConfigBuilderExt for ConfigBuilder<ClientConfig, WantsVerifier> {
    // PlatypusDB: errors instead of panics when the platform has no certificates.
    fn with_native_roots(
        self,
    ) -> crate::Result<ConfigBuilder<ClientConfig, WantsTransparencyPolicyOrClientCert>> {
        let mut roots = RootCertStore::empty();
        let mut valid_count = 0;
        let mut invalid_count = 0;

        let certs = rustls_native_certs::load_native_certs()
            .map_err(|e| Error::Tls(format!("could not load platform certificates: {e}")))?;
        for cert in certs {
            let cert = Certificate(cert.0);
            match roots.add(&cert) {
                Ok(_) => valid_count += 1,
                Err(err) => {
                    tracing::event!(Level::TRACE, "invalid cert der {:?}", cert.0);
                    tracing::event!(Level::DEBUG, "certificate parsing failed: {:?}", err);
                    invalid_count += 1
                }
            }
        }
        tracing::event!(
            Level::TRACE,
            "with_native_roots processed {} valid and {} invalid certs",
            valid_count,
            invalid_count
        );
        if roots.is_empty() {
            return Err(Error::Tls("no CA certificates found".to_owned()));
        }

        Ok(self.with_root_certificates(roots))
    }
}
