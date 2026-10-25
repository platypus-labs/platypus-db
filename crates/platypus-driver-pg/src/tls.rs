//! TLS for tokio-postgres: the protocol-level mode plus the shared rustls configuration.

use platypus_core::{ConnectionConfig, Result, SslMode};
use tokio_postgres::config::SslMode as PgSslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

/// The protocol-level mode for tokio-postgres plus the connector, if TLS may be used.
pub fn connector(config: &ConnectionConfig) -> Result<(PgSslMode, Option<MakeRustlsConnect>)> {
    let mode = match config.ssl_mode {
        SslMode::Disable => PgSslMode::Disable,
        SslMode::Prefer => PgSslMode::Prefer,
        SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull => PgSslMode::Require,
    };
    let tls = platypus_tls::client_config(config)?.map(MakeRustlsConnect::new);
    Ok((mode, tls))
}
