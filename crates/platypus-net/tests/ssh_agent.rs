//! An SSH tunnel that logs in with the keys of the SSH agent. The agent is served by the test
//! itself where the app looks for it (the `openssh-ssh-agent` named pipe on Windows,
//! `SSH_AUTH_SOCK` elsewhere), so no real agent and none of the user's keys are involved. If
//! the place is taken by a real agent (Windows service already running), the test is skipped.
// The test server's handlers keep the shape of russh's async trait.
#![allow(clippy::unused_async_trait_impl)]

use std::net::SocketAddr;
use std::sync::Arc;

use platypus_core::{SshAuth, SshConfig};
use platypus_net::Tunnel;
use russh::Channel;
use russh::keys::agent::client::AgentClient;
use russh::keys::agent::server::serve;
use russh::keys::ssh_key::{self, PrivateKey};
use russh::server::{self, Auth, Msg, Server as _, Session};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const USER: &str = "platypus";

fn random_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), ssh_key::Algorithm::Ed25519).unwrap()
}

/// An SSH server that accepts one public key and forwards to any local port.
#[derive(Clone)]
struct KeyServer {
    accepted: ssh_key::PublicKey,
}

impl server::Server for KeyServer {
    type Handler = Self;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self {
        self.clone()
    }
}

impl server::Handler for KeyServer {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &ssh_key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        Ok(
            if user == USER && key.key_data() == self.accepted.key_data() {
                Auth::Accept
            } else {
                Auth::reject()
            },
        )
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        let Ok(mut socket) = TcpStream::connect((host, u16::try_from(port).unwrap())).await else {
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
        });
        Ok(())
    }
}

async fn ssh_server(accepted: ssh_key::PublicKey) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = Arc::new(server::Config {
        keys: vec![random_key()],
        auth_rejection_time: std::time::Duration::from_millis(10),
        ..server::Config::default()
    });
    let mut server = KeyServer { accepted };
    tokio::spawn(async move {
        let _ = server.run_on_socket(config, &listener).await;
    });
    port
}

async fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut read, mut write) = socket.split();
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
        }
    });
    port
}

/// Serves an empty agent where the app looks for one, and adds `key` to it. `None`: the place
/// is taken, and the test is skipped.
#[cfg(windows)]
async fn start_agent(key: &PrivateKey, _dir: &std::path::Path) -> Option<()> {
    use tokio::net::windows::named_pipe::ServerOptions;
    const PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
    let first = match ServerOptions::new().first_pipe_instance(true).create(PIPE) {
        Ok(pipe) => pipe,
        Err(e) => {
            eprintln!("skipping ssh_agent_tunnel: {PIPE} is taken: {e}");
            return None;
        }
    };
    let stream = futures::stream::unfold(Some(first), |next| async move {
        let pipe = next?;
        let connected = pipe.connect().await.map(|()| pipe);
        let following = ServerOptions::new().create(PIPE).ok();
        Some((connected, following))
    });
    tokio::spawn(serve(Box::pin(stream), ()));
    let mut client = AgentClient::connect_named_pipe(PIPE).await.unwrap();
    client.add_identity(key, &[]).await.unwrap();
    Some(())
}

#[cfg(unix)]
#[allow(unsafe_code)]
async fn start_agent(key: &PrivateKey, dir: &std::path::Path) -> Option<()> {
    use tokio::net::UnixListener;
    let path = dir.join("agent.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let stream = futures::stream::unfold(listener, |listener| async move {
        let accepted = listener.accept().await.map(|(socket, _)| socket);
        Some((accepted, listener))
    });
    tokio::spawn(serve(Box::pin(stream), ()));
    // This file holds the only test: nothing else reads the environment meanwhile.
    unsafe { std::env::set_var("SSH_AUTH_SOCK", &path) };
    let mut client = AgentClient::connect_env().await.unwrap();
    client.add_identity(key, &[]).await.unwrap();
    Some(())
}

fn config(port: u16) -> SshConfig {
    SshConfig {
        host: "127.0.0.1".to_owned(),
        port,
        user: USER.to_owned(),
        auth: SshAuth::Agent,
        jump: None,
    }
}

async fn round_trip(tunnel: &Tunnel) {
    let mut socket = TcpStream::connect(tunnel.local_addr()).await.unwrap();
    socket.write_all(b"SELECT 1").await.unwrap();
    let mut buf = [0; 8];
    socket.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"SELECT 1");
}

#[tokio::test]
async fn ssh_agent_tunnel() {
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    let agent_key = random_key();
    let target = echo_server().await;
    let accepting = ssh_server(agent_key.public_key().clone()).await;
    if start_agent(&agent_key, dir.path()).await.is_none() {
        return;
    }

    // The server accepts the agent's key: the tunnel carries data.
    let tunnel = Tunnel::open(&config(accepting), "127.0.0.1", target, Some(&known_hosts))
        .await
        .unwrap();
    round_trip(&tunnel).await;

    // A server that accepts another key refuses the agent's: no fallback to anything else.
    let refusing = ssh_server(random_key().public_key().clone()).await;
    let err = Tunnel::open(&config(refusing), "127.0.0.1", target, Some(&known_hosts))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("authentication failed"), "{err}");
}
