use serde_json::json;

use super::*;
use crate::testing::{constant, echo, guest, http_forward, serve};

fn sandbox(body: &str, limits: Limits) -> Result<Sandbox, PluginError> {
    let module = compile(&guest(body))?;
    Sandbox::new(&module, &Grants::default(), limits)
}

/// Limits small enough for the tests to hit them quickly.
fn small() -> Limits {
    Limits {
        fuel: 5_000_000,
        ..Limits::default()
    }
}

#[test]
fn round_trips_json() {
    let mut sb = sandbox(&echo(ACTION), small()).unwrap();
    let request = json!({ "ok": { "text": "héllo", "n": [1, 2] } });
    let raw = sb.call_raw(ACTION, request.to_string().as_bytes()).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), request);
    // The envelope's `ok` is the answer.
    let answer: Value = sb.call(ACTION, &request).unwrap();
    assert_eq!(answer, json!({ "text": "héllo", "n": [1, 2] }));
    // An empty request is a null pointer.
    assert_eq!(sb.call_raw(ACTION, b"").unwrap().len(), 0);
    assert!(sb.exports(ACTION));
    assert!(!sb.exports(DRIVER));
}

#[test]
fn reads_errors_and_bad_answers() {
    let body = [
        constant(ACTION, 0, r#"{"error":"no such table"}"#),
        constant(DESCRIBE, 100, r#"{"neither":1}"#),
        constant(DRIVER, 200, "not json"),
    ]
    .join("\n");
    let mut sb = sandbox(&body, small()).unwrap();
    let e = sb.call::<_, Value>(ACTION, &json!({})).unwrap_err();
    assert!(
        matches!(&e, PluginError::Guest(m) if m == "no such table"),
        "{e}"
    );
    // A guest error leaves the sandbox usable.
    assert!(!sb.is_broken());
    let e = sb.call::<_, Value>(DESCRIBE, &json!({})).unwrap_err();
    assert!(e.to_string().contains("neither"), "{e}");
    let e = sb.call::<_, Value>(DRIVER, &json!({})).unwrap_err();
    assert!(e.to_string().contains("not JSON"), "{e}");
}

#[test]
fn stops_an_endless_loop() {
    let body = format!(
        r#"(func (export "{ACTION}") (param i32 i32) (result i64)
             (loop $l (br $l))
             (i64.const 0))"#
    );
    let mut sb = sandbox(&body, small()).unwrap();
    let e = sb.call_raw(ACTION, b"{}").unwrap_err();
    assert!(matches!(e, PluginError::OutOfFuel), "{e}");
    // The sandbox is not used again.
    assert!(sb.is_broken());
    assert!(matches!(
        sb.call_raw(ACTION, b"{}"),
        Err(PluginError::Trap(_))
    ));
}

#[test]
fn caps_memory() {
    // Asks for 32 MiB more, and traps when refused.
    let body = format!(
        r#"(func (export "{ACTION}") (param i32 i32) (result i64)
             (if (i32.eq (memory.grow (i32.const 512)) (i32.const -1)) (then unreachable))
             (i64.const 0))"#
    );
    let tight = Limits {
        memory: 16 << 20,
        ..small()
    };
    let e = sandbox(&body, tight)
        .unwrap()
        .call_raw(ACTION, b"")
        .unwrap_err();
    assert!(matches!(e, PluginError::Trap(_)), "{e}");
    assert!(
        sandbox(&body, small())
            .unwrap()
            .call_raw(ACTION, b"")
            .is_ok()
    );
    // A module whose initial memory is over the cap does not load.
    let huge = wat::parse_str(r#"(module (memory (export "memory") 2000))"#).unwrap();
    let e = Sandbox::new(&compile(&huge).unwrap(), &Grants::default(), tight).unwrap_err();
    assert!(!matches!(e, PluginError::OutOfFuel), "{e}");
}

#[test]
fn reports_traps_and_bad_buffers() {
    let body = [
        format!(r#"(func (export "{ACTION}") (param i32 i32) (result i64) unreachable)"#),
        // Points past the end of memory.
        format!(
            r#"(func (export "{DRIVER}") (param i32 i32) (result i64)
                 (call $pack (i32.const 1000000) (i32.const 10)))"#
        ),
    ]
    .join("\n");
    let e = sandbox(&body, small())
        .unwrap()
        .call_raw(ACTION, b"x")
        .unwrap_err();
    assert!(matches!(e, PluginError::Trap(_)), "{e}");
    let e = sandbox(&body, small())
        .unwrap()
        .call_raw(DRIVER, b"x")
        .unwrap_err();
    assert!(e.to_string().contains("outside"), "{e}");
    let e = sandbox(&body, small())
        .unwrap()
        .call_raw(DESCRIBE, b"x")
        .unwrap_err();
    assert!(e.to_string().contains("does not export"), "{e}");
}

#[test]
fn refuses_large_messages() {
    let limits = Limits {
        message: 1 << 20,
        ..small()
    };
    let mut sb = sandbox(&echo(ACTION), limits).unwrap();
    let e = sb.call_raw(ACTION, &vec![b' '; 2 << 20]).unwrap_err();
    assert!(matches!(e, PluginError::Denied(_)), "{e}");
}

#[test]
fn refuses_modules_that_do_not_fit_the_abi() {
    assert!(matches!(compile(b"not wasm"), Err(PluginError::Invalid(_))));
    // WASI, files, sockets: nothing but the `platypus` functions is linked.
    let wasi = wat::parse_str(
        r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))
                   (memory (export "memory") 1))"#,
    )
    .unwrap();
    let e = Sandbox::new(&compile(&wasi).unwrap(), &Grants::default(), small()).unwrap_err();
    assert!(e.to_string().contains("cannot be loaded"), "{e}");
}

#[test]
fn refuses_modules_without_the_allocator() {
    let no_alloc = wat::parse_str(r#"(module (memory (export "memory") 1))"#).unwrap();
    let e = Sandbox::new(&compile(&no_alloc).unwrap(), &Grants::default(), small()).unwrap_err();
    assert!(e.to_string().contains(ALLOC), "{e}");
}

#[test]
fn stops_an_endless_start_function() {
    // A start function that never ends is stopped too.
    let start = wat::parse_str(
        r#"(module (memory (export "memory") 1) (func $s (loop $l (br $l))) (start $s))"#,
    )
    .unwrap();
    let e = Sandbox::new(&compile(&start).unwrap(), &Grants::default(), small()).unwrap_err();
    assert!(matches!(e, PluginError::OutOfFuel), "{e}");
}

fn http_sandbox(allowed: &[&str]) -> Sandbox {
    let grants = Grants {
        plugin: "test".to_owned(),
        http: allowed.iter().map(|h| (*h).to_owned()).collect(),
        proxy: None,
    };
    let module = compile(&guest(&http_forward(ACTION))).unwrap();
    Sandbox::new(&module, &grants, small()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn sends_http_to_allowed_hosts_only() {
    let addr = serve(|method, path, body| {
        (
            200,
            json!({ "method": method, "path": path, "body": body }).to_string(),
        )
    })
    .await;
    let url = format!("http://127.0.0.1:{}/items?x=1", addr.port());
    tokio::task::spawn_blocking(move || {
        let mut allowed = http_sandbox(&["127.0.0.1"]);
        let response: Value = allowed
            .call(
                ACTION,
                &json!({ "method": "post", "url": url, "body": "hi" }),
            )
            .unwrap();
        assert_eq!(response["status"], 200);
        let echoed: Value = serde_json::from_str(response["body"].as_str().unwrap()).unwrap();
        assert_eq!(
            echoed,
            json!({ "method": "POST", "path": "/items?x=1", "body": "hi" })
        );

        let mut other = http_sandbox(&["api.example.com"]);
        let e = other
            .call::<_, Value>(ACTION, &json!({ "url": url }))
            .unwrap_err();
        assert!(e.to_string().contains("may not reach 127.0.0.1"), "{e}");
        // Refused requests are the guest's to handle: the sandbox goes on.
        assert!(!other.is_broken());

        let mut none = http_sandbox(&[]);
        let e = none
            .call::<_, Value>(ACTION, &json!({ "url": url }))
            .unwrap_err();
        assert!(e.to_string().contains("no permission for HTTP"), "{e}");

        let e = allowed
            .call::<_, Value>(ACTION, &json!({ "url": "file:///etc/passwd" }))
            .unwrap_err();
        assert!(e.to_string().contains("only http and https"), "{e}");
    })
    .await
    .unwrap();
}

#[test]
fn unpacks_pointers() {
    assert_eq!(unpack((5_i64 << 32) | 7), (5, 7));
    assert_eq!(unpack(0), (0, 0));
}
