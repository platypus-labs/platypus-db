//! A SOCKS5 or HTTP proxy between the app and the database server, or its SSH server: the
//! connection's `proxy` option, a URL such as `socks5://user:password@proxy:1080` or
//! `http://proxy:3128`. SOCKS5 resolves the server's name on the proxy (as `socks5h`);
//! HTTP proxies are asked to `CONNECT`.

use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use base64::Engine as _;
use platypus_core::{ConnectionConfig, Error, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The option with the proxy's URL.
pub const PROXY: &str = "proxy";

/// How long reaching the proxy and its answer may take, as the SSH server's.
const PROXY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Socks5,
    Http,
}

/// A proxy, read from its URL.
#[derive(Clone, PartialEq, Eq)]
pub struct Proxy {
    kind: Kind,
    host: String,
    port: u16,
    login: Option<(String, String)>,
}

impl fmt::Debug for Proxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The password stays out of logs.
        f.debug_struct("Proxy")
            .field("kind", &self.kind)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.login.as_ref().map(|(user, _)| user))
            .finish()
    }
}

impl fmt::Display for Proxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = match self.kind {
            Kind::Socks5 => "SOCKS5",
            Kind::Http => "HTTP",
        };
        write!(f, "{scheme} proxy {}:{}", self.host, self.port)
    }
}

fn decode(text: &str) -> String {
    percent_encoding::percent_decode_str(text)
        .decode_utf8_lossy()
        .into_owned()
}

impl Proxy {
    /// The proxy of `config`, if it has one; a blank option is none.
    pub fn of(config: &ConnectionConfig) -> Result<Option<Self>> {
        match config.options.get(PROXY).map(|url| url.trim()) {
            None | Some("") => Ok(None),
            Some(url) => Self::parse(url).map(Some),
        }
    }

    /// Reads `url`: `socks5://`, `socks5h://` or `http://`, then `[user[:password]@]host[:port]`,
    /// by default port 1080 for SOCKS5 and 8080 for HTTP.
    pub fn parse(url: &str) -> Result<Self> {
        let invalid = |why: &str| Error::Config(format!("proxy {url:?}: {why}"));
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| invalid("write it as socks5://host:port or http://host:port"))?;
        let (kind, default_port) = match scheme.to_ascii_lowercase().as_str() {
            "socks5" | "socks5h" | "socks" => (Kind::Socks5, 1080),
            "http" => (Kind::Http, 8080),
            _ => return Err(invalid("the scheme is not socks5 or http")),
        };
        let rest = rest.trim_end_matches('/');
        let (login, address) = match rest.rsplit_once('@') {
            Some((login, address)) => {
                let (user, password) = login.split_once(':').unwrap_or((login, ""));
                (Some((decode(user), decode(password))), address)
            }
            None => (None, rest),
        };
        let (host, port) = if let Some(bracketed) = address.strip_prefix('[') {
            // An IPv6 address: [::1]:1080.
            let (host, after) = bracketed
                .split_once(']')
                .ok_or_else(|| invalid("an IPv6 address without its ]"))?;
            (host, after.strip_prefix(':'))
        } else {
            match address.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (address, None),
            }
        };
        if host.is_empty() || host.contains('/') {
            return Err(invalid("no host"));
        }
        let port = match port {
            None => default_port,
            Some(port) => port
                .parse()
                .ok()
                .filter(|&p| p != 0)
                .ok_or_else(|| invalid("the port is not a number from 1 to 65535"))?,
        };
        Ok(Self {
            kind,
            host: host.to_owned(),
            port,
            login,
        })
    }

    fn error(&self, why: impl fmt::Display) -> Error {
        Error::Connect(format!("{self}: {why}"))
    }

    /// A stream to `host:port` through the proxy.
    pub async fn dial(&self, host: &str, port: u16) -> Result<TcpStream> {
        let dialing = async {
            let mut stream = TcpStream::connect((self.host.as_str(), self.port))
                .await
                .map_err(|e| self.error(e))?;
            let _ = stream.set_nodelay(true);
            match self.kind {
                Kind::Socks5 => self.socks5(&mut stream, host, port).await?,
                Kind::Http => self.http(&mut stream, host, port).await?,
            }
            Ok(stream)
        };
        tokio::time::timeout(PROXY_TIMEOUT, dialing)
            .await
            .map_err(|_| self.error(format!("no answer within {} s", PROXY_TIMEOUT.as_secs())))?
    }

    /// The SOCKS5 handshake (RFC 1928), with a user and password (RFC 1929) when it has one.
    async fn socks5(&self, stream: &mut TcpStream, host: &str, port: u16) -> Result<()> {
        let io = |e: std::io::Error| self.error(e);
        // Version 5, then the methods offered: none, and user and password with a login.
        let hello: &[u8] = if self.login.is_some() {
            &[5, 2, 0, 2]
        } else {
            &[5, 1, 0]
        };
        stream.write_all(hello).await.map_err(io)?;
        let mut chosen = [0u8; 2];
        stream.read_exact(&mut chosen).await.map_err(io)?;
        if chosen[0] != 5 {
            return Err(self.error("not a SOCKS5 proxy"));
        }
        match (chosen[1], &self.login) {
            (0, _) => {}
            (2, Some((user, password))) => {
                let mut login = vec![1];
                for text in [user, password] {
                    self.push_short(&mut login, text, "the user or password")?;
                }
                stream.write_all(&login).await.map_err(io)?;
                let mut status = [0u8; 2];
                stream.read_exact(&mut status).await.map_err(io)?;
                if status[1] != 0 {
                    return Err(self.error("the user or password was refused"));
                }
            }
            _ => return Err(self.error("it asks for a login the connection does not give")),
        }
        let mut request = vec![5, 1, 0];
        match host.parse::<IpAddr>() {
            Ok(IpAddr::V4(ip)) => {
                request.push(1);
                request.extend_from_slice(&ip.octets());
            }
            Ok(IpAddr::V6(ip)) => {
                request.push(4);
                request.extend_from_slice(&ip.octets());
            }
            Err(_) => {
                request.push(3);
                self.push_short(&mut request, host, "the server's name")?;
            }
        }
        request.extend_from_slice(&port.to_be_bytes());
        stream.write_all(&request).await.map_err(io)?;
        let mut reply = [0u8; 4];
        stream.read_exact(&mut reply).await.map_err(io)?;
        if reply[1] != 0 {
            return Err(self.error(format!(
                "cannot reach {host}:{port}: {}",
                socks5_reply(reply[1])
            )));
        }
        // The address the proxy bound, which nothing needs.
        let bound = match reply[3] {
            1 => 4,
            4 => 16,
            3 => usize::from(stream.read_u8().await.map_err(io)?),
            _ => return Err(self.error("an answer that is not SOCKS5")),
        };
        let mut skip = vec![0u8; bound + 2];
        stream.read_exact(&mut skip).await.map_err(io)?;
        Ok(())
    }

    /// Appends `text` after its length in one byte, as SOCKS5 sends names; `what` names it
    /// in the error when it is too long.
    fn push_short(&self, buf: &mut Vec<u8>, text: &str, what: &str) -> Result<()> {
        let len = u8::try_from(text.len())
            .map_err(|_| self.error(format!("{what} is over 255 bytes")))?;
        buf.push(len);
        buf.extend_from_slice(text.as_bytes());
        Ok(())
    }

    /// An HTTP `CONNECT`, with Basic authentication when it has a login.
    async fn http(&self, stream: &mut TcpStream, host: &str, port: u16) -> Result<()> {
        use std::fmt::Write as _;
        let io = |e: std::io::Error| self.error(e);
        let target = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
        if let Some((user, password)) = &self.login {
            let token =
                base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
            let _ = write!(request, "Proxy-Authorization: Basic {token}\r\n");
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).await.map_err(io)?;
        // Read the head byte by byte: what follows it belongs to the server.
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() > 16 * 1024 {
                return Err(self.error("an answer that is not HTTP"));
            }
            head.push(stream.read_u8().await.map_err(io)?);
        }
        let head = String::from_utf8_lossy(&head);
        let status = head.lines().next().unwrap_or_default();
        let code = status.split_whitespace().nth(1).unwrap_or_default();
        if code.starts_with('2') {
            return Ok(());
        }
        Err(self.error(match code {
            "407" => "the proxy asks for a user and password, or refused them".to_owned(),
            "" => "an answer that is not HTTP".to_owned(),
            _ => format!("cannot reach {target}: {}", status.trim()),
        }))
    }
}

fn socks5_reply(code: u8) -> &'static str {
    match code {
        1 => "general failure",
        2 => "not allowed by the proxy's rules",
        3 => "network unreachable",
        4 => "host unreachable",
        5 => "connection refused",
        6 => "time to live expired",
        7 => "command not supported",
        8 => "address type not supported",
        _ => "refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn reads_the_urls() {
        let socks = Proxy::parse("socks5://ada:p%40ss@proxy.lan:1081").unwrap();
        assert_eq!(socks.kind, Kind::Socks5);
        assert_eq!((socks.host.as_str(), socks.port), ("proxy.lan", 1081));
        assert_eq!(socks.login, Some(("ada".to_owned(), "p@ss".to_owned())));
        assert!(
            !format!("{socks:?}").contains("p@ss"),
            "no password in logs"
        );

        let http = Proxy::parse("http://10.0.0.1/").unwrap();
        assert_eq!((http.kind, http.port, http.login), (Kind::Http, 8080, None));
        assert_eq!(Proxy::parse("socks5h://[::1]").unwrap().host, "::1");
        assert_eq!(Proxy::parse("SOCKS5://h").unwrap().port, 1080);

        for bad in [
            "proxy:1080",
            "ftp://h",
            "socks5://",
            "http://h:0",
            "http://h:x",
            "socks5://[::1",
        ] {
            assert!(Proxy::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_blank_option_is_no_proxy() {
        let mut config = ConnectionConfig::new(platypus_core::DriverKind::Postgres);
        assert_eq!(Proxy::of(&config).unwrap(), None);
        config.options.insert(PROXY.to_owned(), " ".to_owned());
        assert_eq!(Proxy::of(&config).unwrap(), None);
        config.options.insert(PROXY.to_owned(), "nope".to_owned());
        assert!(Proxy::of(&config).is_err());
    }

    /// An echo server, the target behind the proxies.
    async fn echo() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (mut r, mut w) = socket.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });
        port
    }

    /// A SOCKS5 proxy that wants `login` when given one, and records the target it is asked for.
    async fn fake_socks5(
        login: Option<(&'static str, &'static str)>,
    ) -> (u16, tokio::sync::mpsc::UnboundedReceiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (asked, receiver) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let asked = asked.clone();
                tokio::spawn(async move {
                    let mut head = [0u8; 2];
                    socket.read_exact(&mut head).await.unwrap();
                    let mut methods = vec![0u8; usize::from(head[1])];
                    socket.read_exact(&mut methods).await.unwrap();
                    if let Some((user, password)) = login {
                        if !methods.contains(&2) {
                            socket.write_all(&[5, 0xff]).await.unwrap();
                            return;
                        }
                        socket.write_all(&[5, 2]).await.unwrap();
                        let mut version = [0u8; 2];
                        socket.read_exact(&mut version).await.unwrap();
                        let mut u = vec![0u8; usize::from(version[1])];
                        socket.read_exact(&mut u).await.unwrap();
                        let mut p = vec![0u8; usize::from(socket.read_u8().await.unwrap())];
                        socket.read_exact(&mut p).await.unwrap();
                        let ok = u == user.as_bytes() && p == password.as_bytes();
                        socket.write_all(&[1, u8::from(!ok)]).await.unwrap();
                        if !ok {
                            return;
                        }
                    } else {
                        socket.write_all(&[5, 0]).await.unwrap();
                    }
                    let mut request = [0u8; 4];
                    socket.read_exact(&mut request).await.unwrap();
                    assert_eq!(request[3], 3, "the name goes to the proxy");
                    let mut name = vec![0u8; usize::from(socket.read_u8().await.unwrap())];
                    socket.read_exact(&mut name).await.unwrap();
                    let port = socket.read_u16().await.unwrap();
                    let name = String::from_utf8(name).unwrap();
                    let _ = asked.send(format!("{name}:{port}"));
                    let Ok(mut target) = TcpStream::connect(("127.0.0.1", port)).await else {
                        socket
                            .write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0])
                            .await
                            .unwrap();
                        return;
                    };
                    socket
                        .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                        .await
                        .unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut target).await;
                });
            }
        });
        (port, receiver)
    }

    /// An HTTP proxy that wants `token` (Basic) when given one.
    async fn fake_http(token: Option<&'static str>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        head.push(socket.read_u8().await.unwrap());
                    }
                    let head = String::from_utf8(head).unwrap();
                    if let Some(token) = token
                        && !head.contains(&format!("Proxy-Authorization: Basic {token}\r\n"))
                    {
                        socket
                            .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                            .await
                            .unwrap();
                        return;
                    }
                    let target = head.split_whitespace().nth(1).unwrap().to_owned();
                    let port: u16 = target.rsplit_once(':').unwrap().1.parse().unwrap();
                    let Ok(mut server) = TcpStream::connect(("127.0.0.1", port)).await else {
                        socket
                            .write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
                            .await
                            .unwrap();
                        return;
                    };
                    socket
                        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .await
                        .unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut server).await;
                });
            }
        });
        port
    }

    async fn round_trip(mut stream: TcpStream) {
        stream.write_all(b"ping").await.unwrap();
        let mut back = [0u8; 4];
        stream.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"ping");
    }

    async fn closed_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn through_a_socks5_proxy() {
        let target = echo().await;
        let (port, mut asked) = fake_socks5(None).await;
        let proxy = Proxy::parse(&format!("socks5://127.0.0.1:{port}")).unwrap();
        round_trip(proxy.dial("localhost", target).await.unwrap()).await;
        assert_eq!(asked.recv().await.unwrap(), format!("localhost:{target}"));
    }

    #[tokio::test]
    async fn a_socks5_proxy_with_a_login() {
        let target = echo().await;
        let (port, _) = fake_socks5(Some(("ada", "s3cret"))).await;
        let with = Proxy::parse(&format!("socks5://ada:s3cret@127.0.0.1:{port}")).unwrap();
        round_trip(with.dial("localhost", target).await.unwrap()).await;

        let wrong = Proxy::parse(&format!("socks5://ada:nope@127.0.0.1:{port}")).unwrap();
        let error = wrong
            .dial("localhost", target)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("refused"), "{error}");
        let without = Proxy::parse(&format!("socks5://127.0.0.1:{port}")).unwrap();
        let error = without
            .dial("localhost", target)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("asks for a login"), "{error}");
    }

    #[tokio::test]
    async fn a_socks5_proxy_says_why_it_cannot_reach() {
        let (port, _) = fake_socks5(None).await;
        let proxy = Proxy::parse(&format!("socks5://127.0.0.1:{port}")).unwrap();
        let closed = closed_port().await;
        let error = proxy
            .dial("localhost", closed)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("SOCKS5 proxy"), "{error}");
        assert!(error.contains("connection refused"), "{error}");
    }

    #[tokio::test]
    async fn through_an_http_proxy() {
        let target = echo().await;
        let port = fake_http(Some("YWRhOnMzY3JldA==")).await;
        let with = Proxy::parse(&format!("http://ada:s3cret@127.0.0.1:{port}")).unwrap();
        round_trip(with.dial("localhost", target).await.unwrap()).await;

        let without = Proxy::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        let error = without
            .dial("localhost", target)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("user and password"), "{error}");
    }

    #[tokio::test]
    async fn an_http_proxy_says_why_it_cannot_reach() {
        let port = fake_http(None).await;
        let proxy = Proxy::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        let closed = closed_port().await;
        let error = proxy
            .dial("localhost", closed)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("502"), "{error}");
    }

    #[tokio::test]
    async fn an_unreachable_proxy_is_an_error() {
        let closed = closed_port().await;
        let proxy = Proxy::parse(&format!("socks5://127.0.0.1:{closed}")).unwrap();
        let error = proxy.dial("localhost", 5432).await.unwrap_err().to_string();
        assert!(
            error.starts_with("connection failed: SOCKS5 proxy 127.0.0.1:"),
            "{error}"
        );
    }
}
