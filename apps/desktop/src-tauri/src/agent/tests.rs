use std::sync::Mutex;

use platypus_mcp::{Agent, HttpLink, Profile};

use super::*;

type Events = Arc<Mutex<Vec<(String, serde_json::Value)>>>;

/// A host with one session, keeping the events it was given.
#[derive(Default)]
struct FakeHost {
    events: Events,
}

impl Host for FakeHost {
    fn sessions(&self) -> Vec<OpenSession> {
        vec![OpenSession {
            connection: "Shop".to_owned(),
            database: Some("sales".to_owned()),
            purpose: "main".to_owned(),
            manual_commit: false,
            transaction_open: false,
        }]
    }

    fn notify(&self, event: &str, payload: serde_json::Value) {
        lock(&self.events).push((event.to_owned(), payload));
    }
}

/// A bridge serving on a free port, with its files in `dir`.
async fn bridge(dir: &Path) -> (Bridge, Events) {
    let host = FakeHost::default();
    let events = Arc::clone(&host.events);
    let bridge = Bridge::new(Arc::new(Link::new(host)), Some(dir.to_owned()));
    let agent = Arc::new(Agent::new(Profile::at(dir)));
    bridge.start(true, 0, Ok(agent)).await;
    assert_eq!(bridge.status(true, 0).error, None);
    (bridge, events)
}

fn change(id: &str) -> WriteRequest {
    WriteRequest {
        id: id.to_owned(),
        agent: "claude-code".to_owned(),
        session: "s1".to_owned(),
        connection: "Shop".to_owned(),
        statement: "DELETE FROM t WHERE id = 1".to_owned(),
        estimated_rows: Some(1.0),
        plan: None,
    }
}

/// What `ready` gives once it gives something, within two seconds.
async fn eventually<T>(mut ready: impl FnMut() -> Option<T>) -> Option<T> {
    for _ in 0..200 {
        if let Some(value) = ready() {
            return Some(value);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    None
}

/// Waits until the bridge has a change waiting.
async fn asked(bridge: &Bridge) -> Vec<WaitingWrite> {
    eventually(|| Some(bridge.link().waiting()).filter(|w| !w.is_empty()))
        .await
        .expect("nothing asked")
}

#[tokio::test]
async fn agents_ask_the_user_through_the_bridge() {
    let dir = tempfile::tempdir().unwrap();
    let (bridge, events) = bridge(dir.path()).await;
    let link = HttpLink::new(dir.path());

    // The sessions of the app, and the agent shown connected.
    let sessions = link.sessions().await.unwrap();
    assert_eq!(sessions[0].connection, "Shop");
    link.hello("s1", "claude-code").await.unwrap();
    link.hello("s1", "claude-code").await.unwrap();
    assert_eq!(
        bridge.link().connected(),
        [ConnectedAgent {
            session: "s1".to_owned(),
            agent: "claude-code".to_owned()
        }]
    );
    let told = |name: &str| lock(&events).iter().filter(|(e, _)| e == name).count();
    assert_eq!(told(AGENTS_EVENT), 1, "a hello again is not news");

    // A change waits for the user's answer, then is gone.
    let asking = tokio::spawn(async move { link.confirm(change("c1")).await });
    let waiting = asked(&bridge).await;
    assert_eq!(waiting[0].request.statement, "DELETE FROM t WHERE id = 1");
    assert_eq!(told(WRITE_REQUEST_EVENT), 1);
    assert!(bridge.link().answer("c1", Verdict::Session));
    assert_eq!(asking.await.unwrap().unwrap(), Verdict::Session);
    assert_eq!(bridge.link().waiting(), []);
    assert_eq!(told(WRITE_SETTLED_EVENT), 1);
    assert!(
        !bridge.link().answer("c1", Verdict::Once),
        "answered already"
    );

    // Changes wait in the order they came, however their ids sort.
    let (first, second) = (HttpLink::new(dir.path()), HttpLink::new(dir.path()));
    let one = tokio::spawn(async move { first.confirm(change("zz")).await });
    asked(&bridge).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let two = tokio::spawn(async move { second.confirm(change("aa")).await });
    let waiting = eventually(|| Some(bridge.link().waiting()).filter(|w| w.len() == 2))
        .await
        .expect("two asked");
    assert_eq!(waiting[0].request.id, "zz");
    assert!(
        waiting[0].waited_ms >= waiting[1].waited_ms + 20,
        "{waiting:?}"
    );
    assert!(bridge.link().answer("zz", Verdict::Once));
    assert!(bridge.link().answer("aa", Verdict::Once));
    one.await.unwrap().unwrap();
    two.await.unwrap().unwrap();

    // An agent that goes away takes its change with it.
    let link = HttpLink::new(dir.path());
    let asking = tokio::spawn(async move { link.confirm(change("c2")).await });
    asked(&bridge).await;
    asking.abort();
    assert!(
        eventually(|| bridge.link().waiting().is_empty().then_some(()))
            .await
            .is_some(),
        "the change went with it"
    );

    // Stopped: the file is gone, and agents see the app closed.
    bridge.stop();
    assert!(!BridgeFile::path(dir.path()).exists());
    assert!(HttpLink::new(dir.path()).sessions().await.is_err());
    assert_eq!(bridge.link().connected(), []);
}

#[tokio::test]
async fn a_wrong_token_is_refused_and_a_new_one_replaces_the_old() {
    let dir = tempfile::tempdir().unwrap();
    let (bridge, _) = bridge(dir.path()).await;
    let token = bridge.token().unwrap();
    assert_eq!(token.len(), 64);
    assert_eq!(bridge.token().unwrap(), token, "kept across calls");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(BridgeFile::path(dir.path()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    // The bridge's file with another token, as with a client set up before a new token.
    let mut file = BridgeFile::read(dir.path()).unwrap();
    file.token = "wrong".to_owned();
    std::fs::write(
        BridgeFile::path(dir.path()),
        serde_json::to_string(&file).unwrap(),
    )
    .unwrap();
    let refused = HttpLink::new(dir.path()).sessions().await.unwrap_err();
    assert!(refused.contains("token"), "{refused}");

    bridge.regenerate().unwrap();
    assert_ne!(bridge.token().unwrap(), token);
}

#[tokio::test]
async fn the_mcp_endpoint_serves_the_tools() {
    let dir = tempfile::tempdir().unwrap();
    let (bridge, _) = bridge(dir.path()).await;
    let url = bridge.status(true, 0).url.unwrap();
    let port: u16 = url
        .trim_start_matches("http://127.0.0.1:")
        .trim_end_matches("/mcp")
        .parse()
        .unwrap();
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "1" }
        }
    })
    .to_string();
    let initialize = |token: &str| {
        format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
             Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    let send = |request: String| async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut answer = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut answer)).await;
        String::from_utf8_lossy(&answer).into_owned()
    };
    let answer = send(initialize(&bridge.token().unwrap())).await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.contains("platypus-mcp"), "{answer}");
    let refused = send(initialize("nope")).await;
    assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
    bridge.stop();
}

#[tokio::test]
async fn a_port_in_use_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let taken = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = taken.local_addr().unwrap().port();
    let bridge = Bridge::new(
        Arc::new(Link::new(FakeHost::default())),
        Some(dir.path().to_owned()),
    );
    // A file left by a run that crashed goes, though this start fails.
    std::fs::write(BridgeFile::path(dir.path()), r#"{"port":1,"token":"old"}"#).unwrap();
    let agent = Arc::new(Agent::new(Profile::at(dir.path())));
    bridge.start(true, port, Ok(agent)).await;
    let status = bridge.status(true, port);
    assert!(status.url.is_none());
    assert!(
        status.error.as_deref().unwrap().contains(&port.to_string()),
        "{status:?}"
    );
    assert!(!BridgeFile::path(dir.path()).exists());
}
