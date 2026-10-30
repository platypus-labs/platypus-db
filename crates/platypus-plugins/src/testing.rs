//! Small guests written in the WebAssembly text format, for the tests.

/// A module with a bump allocator, a no-op `platypus_dealloc`, the host imports and
/// `body` (entry points and data segments). Allocations start at 64 KiB, after the data.
pub(crate) fn guest(body: &str) -> Vec<u8> {
    let text = format!(
        r#"(module
  (import "platypus" "http" (func $http (param i32 i32) (result i32)))
  (import "platypus" "take" (func $take (param i32)))
  (import "platypus" "log" (func $log (param i32 i32 i32)))
  (memory (export "memory") 4)
  (global $next (mut i32) (i32.const 65536))
  (func $alloc (export "platypus_alloc") (param $len i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (local.get $p))
  (func (export "platypus_dealloc") (param i32 i32))
  (func $pack (param $ptr i32) (param $len i32) (result i64)
    (i64.or
      (i64.shl (i64.extend_i32_u (local.get $ptr)) (i64.const 32))
      (i64.extend_i32_u (local.get $len))))
  {body}
)"#
    );
    wat::parse_str(text).expect("valid test module")
}

/// An entry point that answers `json`, kept at `offset` (below 64 KiB).
pub(crate) fn constant(entry: &str, offset: u32, json: &str) -> String {
    let escaped: String = json
        .bytes()
        .map(|b| match b {
            b'"' | b'\\' => format!("\\{b:02x}"),
            _ => char::from(b).to_string(),
        })
        .collect();
    format!(
        r#"(data (i32.const {offset}) "{escaped}")
  (func (export "{entry}") (param i32 i32) (result i64)
    (call $pack (i32.const {offset}) (i32.const {len})))"#,
        len = json.len()
    )
}

/// An entry point that answers its request as it came.
pub(crate) fn echo(entry: &str) -> String {
    format!(
        r#"(func (export "{entry}") (param $ptr i32) (param $len i32) (result i64)
    (call $pack (local.get $ptr) (local.get $len)))"#
    )
}

/// An entry point that sends its request to `http` and answers what it got.
pub(crate) fn http_forward(entry: &str) -> String {
    format!(
        r#"(func (export "{entry}") (param $ptr i32) (param $len i32) (result i64)
    (local $n i32) (local $out i32)
    (local.set $n (call $http (local.get $ptr) (local.get $len)))
    (local.set $out (call $alloc (local.get $n)))
    (call $take (local.get $out))
    (call $pack (local.get $out) (local.get $n)))"#
    )
}

/// Serves HTTP on a free local port until the test ends, answering each request with
/// `answer(method, path, body) -> (status, body)` as JSON.
pub(crate) async fn serve(
    answer: impl Fn(&str, &str, &str) -> (u16, String) + Send + Sync + 'static,
) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let answer = std::sync::Arc::new(answer);
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(reply(socket, std::sync::Arc::clone(&answer)));
        }
    });
    addr
}

async fn reply(
    mut socket: tokio::net::TcpStream,
    answer: std::sync::Arc<impl Fn(&str, &str, &str) -> (u16, String)>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let (head, body_start) = loop {
        let Ok(n) = socket.read(&mut buf).await else {
            return;
        };
        if n == 0 {
            return;
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break (String::from_utf8_lossy(&data[..end]).into_owned(), end + 4);
        }
    };
    let length: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())?
        })
        .unwrap_or(0);
    while data.len() < body_start + length {
        match socket.read(&mut buf).await {
            Ok(n) if n > 0 => data.extend_from_slice(&buf[..n]),
            _ => break,
        }
    }
    let body = String::from_utf8_lossy(&data[body_start..]).into_owned();
    let mut first = head.lines().next().unwrap_or_default().split(' ');
    let method = first.next().unwrap_or_default().to_owned();
    let path = first.next().unwrap_or_default().to_owned();
    let (status, text) = answer(&method, &path, &body);
    let response = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
}
