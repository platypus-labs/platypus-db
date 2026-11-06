//! PlatypusDB: the GSS login (Kerberos), which tokio-postgres refuses. The tokens come from
//! `cross-krb5` behind the `kerberos` feature: SSPI on Windows, GSSAPI elsewhere. They go in
//! `GSSResponse` messages, and the server's in `AuthenticationGSSContinue`, until it says
//! `AuthenticationOk`.
//!
//! A login happens only when [`Config::gss_target`](crate::Config::gss_target) is set, and then
//! it is the only way in: no password is sent, and a server that does not ask for it is refused.

use std::{error, fmt};

/// Why a login with Kerberos did not happen.
#[derive(Debug)]
#[non_exhaustive]
pub enum GssError {
    /// This build has no `kerberos` feature.
    NotBuilt,
    /// The server asks for Kerberos, and the connection does not use it.
    NotChosen,
    /// The connection uses Kerberos, and the server asks for a password.
    PasswordAsked,
    /// The connection uses Kerberos, and the server let it in without asking for it.
    NotAsked,
    /// The server said the login is done before proving who it is.
    Unproven,
    /// The system's Kerberos could not go on: no ticket, no KDC, an unknown service.
    Failed {
        /// The service principal the ticket is for, `service/host`.
        target: String,
        /// What the system's Kerberos said.
        detail: String,
    },
}

impl fmt::Display for GssError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GssError::NotBuilt => f.write_str("this build has no Kerberos"),
            GssError::NotChosen => f.write_str("the server asks for Kerberos"),
            GssError::PasswordAsked => f.write_str("the server asks for a password"),
            GssError::NotAsked => f.write_str("the server did not ask for Kerberos"),
            GssError::Unproven => f.write_str("the server did not prove who it is"),
            GssError::Failed { target, detail } => write!(f, "{target}: {detail}"),
        }
    }
}

impl error::Error for GssError {}

#[cfg(feature = "kerberos")]
pub(crate) use with_kerberos::authenticate;
#[cfg(not(feature = "kerberos"))]
pub(crate) use without_kerberos::authenticate;

#[cfg(not(feature = "kerberos"))]
mod without_kerberos {
    use super::GssError;
    use crate::Error;
    use crate::connect_raw::StartupStream;

    pub(crate) async fn authenticate<S, T>(
        _stream: &mut StartupStream<S, T>,
        _target: &str,
        _negotiate: bool,
    ) -> Result<(), Error> {
        Err(Error::authentication(Box::new(GssError::NotBuilt)))
    }
}

#[cfg(feature = "kerberos")]
mod with_kerberos {
    use super::GssError;
    use crate::Error;
    use crate::codec::FrontendMessage;
    use crate::connect_raw::StartupStream;
    use bytes::BytesMut;
    use cross_krb5::{ClientCtx, InitiateFlags, PendingClientCtx, Step};
    use futures_util::{SinkExt, TryStreamExt};
    use postgres_protocol::message::backend::Message;
    use postgres_protocol::message::frontend;
    use tokio::io::{AsyncRead, AsyncWrite};

    /// The context still waiting for the server, none when it is established, and the token
    /// to send, if any.
    type Stepped = (Option<PendingClientCtx>, Option<Vec<u8>>);

    pub(crate) async fn authenticate<S, T>(
        stream: &mut StartupStream<S, T>,
        target: &str,
        negotiate: bool,
    ) -> Result<(), Error>
    where
        S: AsyncRead + AsyncWrite + Unpin,
        T: AsyncRead + AsyncWrite + Unpin,
    {
        let owned = target.to_owned();
        let (mut pending, token) = blocking(target, move || begin(&owned, negotiate)).await?;
        send_token(stream, &token).await?;
        loop {
            match stream.try_next().await.map_err(Error::io)? {
                Some(Message::AuthenticationGssContinue(body)) => {
                    let Some(context) = pending.take() else {
                        return Err(Error::unexpected_message());
                    };
                    let data = body.data().to_vec();
                    let (next, token) = blocking(target, move || step(context, &data)).await?;
                    pending = next;
                    if let Some(token) = token {
                        send_token(stream, &token).await?;
                    }
                }
                // Mutual authentication is asked for, so the last token is the server's proof.
                Some(Message::AuthenticationOk) => {
                    return match pending {
                        None => Ok(()),
                        Some(_) => Err(Error::authentication(Box::new(GssError::Unproven))),
                    };
                }
                Some(Message::ErrorResponse(body)) => return Err(Error::db(body)),
                Some(_) => return Err(Error::unexpected_message()),
                None => return Err(Error::closed()),
            }
        }
    }

    /// Runs a step off the async threads: it may read the ticket cache and ask the KDC.
    async fn blocking<R, F>(target: &str, work: F) -> Result<R, Error>
    where
        F: FnOnce() -> anyhow::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let failed = |detail: String| {
            Error::authentication(Box::new(GssError::Failed {
                target: target.to_owned(),
                detail,
            }))
        };
        match tokio::task::spawn_blocking(work).await {
            Ok(Ok(done)) => Ok(done),
            Ok(Err(e)) => Err(failed(format!("{e:#}"))),
            Err(e) => Err(failed(e.to_string())),
        }
    }

    fn begin(target: &str, negotiate: bool) -> anyhow::Result<(Option<PendingClientCtx>, Vec<u8>)> {
        // A server's `sspi` line (AuthenticationSSPI) takes SPNEGO tokens, its `gss` line plain
        // Kerberos ones. The flag only means something to SSPI, and allows Kerberos alone.
        let flags = if negotiate {
            InitiateFlags::NEGOTIATE_TOKEN
        } else {
            InitiateFlags::empty()
        };
        let (context, token) = ClientCtx::new(flags, None, target, None)?;
        Ok((Some(context), token.to_vec()))
    }

    fn step(context: PendingClientCtx, token: &[u8]) -> anyhow::Result<Stepped> {
        Ok(match context.step(token)? {
            Step::Finished((_established, token)) => (None, token.map(|t| t.to_vec())),
            Step::Continue((context, token)) => (Some(context), Some(token.to_vec())),
        })
    }

    async fn send_token<S, T>(stream: &mut StartupStream<S, T>, token: &[u8]) -> Result<(), Error>
    where
        S: AsyncRead + AsyncWrite + Unpin,
        T: AsyncRead + AsyncWrite + Unpin,
    {
        // `GSSResponse` is `SASLResponse` on the wire: `p` and the bytes, with no final zero.
        let mut buf = BytesMut::new();
        frontend::sasl_response(token, &mut buf).map_err(Error::encode)?;
        stream
            .send(FrontendMessage::Raw(buf.freeze()))
            .await
            .map_err(Error::io)
    }
}
