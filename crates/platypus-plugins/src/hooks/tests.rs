use std::path::{Path, PathBuf};
use std::time::Instant;

use platypus_store::MemorySecrets;
use serde_json::{Value, json};

use super::*;
use crate::manifest::MANIFEST_FILE;
use crate::runtime::DESCRIBE;
use crate::testing::{constant, guest};

const DESCRIBE_NOTHING: &str = r#"{"ok":{}}"#;

fn connection() -> HookConnection {
    HookConnection {
        name: "main".to_owned(),
        driver: "sqlite".to_owned(),
        database: "shop".to_owned(),
        production: false,
    }
}

fn before_query() -> HookContext {
    HookContext::BeforeQuery {
        connection: connection(),
        statement: HookStatement::new(Some("SELECT 1".to_owned())),
    }
}

fn manifest(id: &str, events: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"Test {id}\"\nversion = \"1\"\napi = 2\nkinds = [\"hooks\"]\nhooks = [{events}]\n[permissions]\nhooks = true\n"
    )
}

/// A package whose `platypus_hook` is `hook`.
fn package(root: &Path, id: &str, events: &str, hook: &str) -> PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), manifest(id, events)).unwrap();
    let body = [constant(DESCRIBE, 0, DESCRIBE_NOTHING), hook.to_owned()].join("\n");
    std::fs::write(dir.join(MODULE_FILE), guest(&body)).unwrap();
    dir
}

fn answering(json: &str) -> String {
    constant(HOOK, 4096, json)
}

const ALLOW: &str = r#"{"ok":{"action":"allow"}}"#;
const BLOCK: &str = r#"{"ok":{"action":"block","message":"not on Fridays"}}"#;
const BEFORE: &str = "\"before_query\"";

struct Setup {
    plugins: Plugins,
    table: HookTable,
    data: tempfile::TempDir,
    root: tempfile::TempDir,
}

fn setup() -> Setup {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path())
        .with_secrets(Arc::new(MemorySecrets::default()))
        .with_hook_limits(HookLimits {
            sandbox: Limits {
                // An endless loop should not take the test's time.
                fuel: 20_000_000,
                ..HookLimits::default().sandbox
            },
            // Slow machines: this test is about fuel, not about the wall time.
            time: Duration::from_secs(60),
        });
    Setup {
        plugins,
        table: HookTable::new(),
        data,
        root: tempfile::tempdir().unwrap(),
    }
}

impl Setup {
    async fn install(&self, id: &str, events: &str, hook: &str) {
        let source = package(self.root.path(), id, events, hook);
        let reviewed = self.plugins.inspect(&source).await.unwrap();
        self.plugins.install(&source, &reviewed.hash).await.unwrap();
        self.reload();
    }

    fn reload(&self) {
        self.table
            .load(&self.plugins, &self.plugins.list().unwrap());
    }
}

#[test]
fn nothing_wanted_once_read_and_empty() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let table = HookTable::new();
    // Until the list is read every event asks, so that no hook is missed at the start.
    assert!(table.wanted(HookEvent::BeforeQuery));
    table.load(&plugins, &plugins.list().unwrap());
    for event in [
        HookEvent::BeforeQuery,
        HookEvent::AfterQuery,
        HookEvent::OnConnect,
        HookEvent::BeforeSave,
    ] {
        assert!(!table.wanted(event));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_run_reads_the_list_when_nothing_did() {
    let s = setup();
    s.install("guard", BEFORE, &answering(BLOCK)).await;
    let fresh = HookTable::new();
    let report = fresh.run(&s.plugins, &before_query()).await;
    assert_eq!(report.blocked.unwrap().message, "not on Fridays");
    assert!(fresh.wanted(HookEvent::BeforeQuery));
    assert!(!fresh.wanted(HookEvent::AfterQuery));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_before_hook_blocks_with_its_message() {
    let s = setup();
    s.install("guard", BEFORE, &answering(BLOCK)).await;
    assert!(s.table.wanted(HookEvent::BeforeQuery));
    assert!(!s.table.wanted(HookEvent::AfterQuery));
    let report = s.table.run(&s.plugins, &before_query()).await;
    assert_eq!(
        report.blocked,
        Some(HookBlock {
            plugin: "Test guard".to_owned(),
            message: "not on Fridays".to_owned(),
        })
    );
    assert_eq!(report.failures, []);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_before_events_can_block() {
    let s = setup();
    s.install("late", "\"after_query\", \"on_connect\"", &answering(BLOCK))
        .await;
    let after = HookContext::AfterQuery {
        connection: connection(),
        statement: HookStatement::new(Some("SELECT 1".to_owned())),
        status: QueryStatus::Ok,
        elapsed_ms: 3,
        affected: None,
    };
    assert_eq!(s.table.run(&s.plugins, &after).await, HookReport::default());
    let connect = HookContext::OnConnect {
        connection: connection(),
    };
    assert_eq!(
        s.table.run(&s.plugins, &connect).await,
        HookReport::default()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_event_runs_only_the_hooks_of_that_event() {
    let s = setup();
    s.install("late", "\"after_query\"", &answering(BLOCK))
        .await;
    s.install("fine", BEFORE, &answering(ALLOW)).await;
    let report = s.table.run(&s.plugins, &before_query()).await;
    assert_eq!(report, HookReport::default());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_broken_hook_lets_the_action_through_and_is_told_once() {
    for (name, hook) in [
        (
            "crash",
            format!("(func (export \"{HOOK}\") (param i32 i32) (result i64) unreachable)"),
        ),
        (
            "endless",
            format!(
                "(func (export \"{HOOK}\") (param i32 i32) (result i64) (loop $l (br $l)) (i64.const 0))"
            ),
        ),
        ("nonsense", answering("not json")),
        ("error", answering(r#"{"error":"no database for me"}"#)),
        ("wrong", answering(r#"{"ok":{"action":"shrug"}}"#)),
    ] {
        let s = setup();
        s.install(name, BEFORE, &hook).await;
        let report = s.table.run(&s.plugins, &before_query()).await;
        assert_eq!(report.blocked, None, "{name}");
        assert_eq!(report.failures.len(), 1, "{name}");
        assert_eq!(report.failures[0].plugin, format!("Test {name}"));
        // The user is told once.
        let again = s.table.run(&s.plugins, &before_query()).await;
        assert_eq!(again, HookReport::default(), "{name}");
        // Told again of a plugin that changed.
        s.table.forgive(name);
        let forgiven = s.table.run(&s.plugins, &before_query()).await;
        assert_eq!(forgiven.failures.len(), 1, "{name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hook_cannot_rewrite_the_statement() {
    // The answer has no room for it: such a hook is a broken one and the action goes ahead.
    let s = setup();
    s.install(
        "rewrite",
        BEFORE,
        &answering(r#"{"ok":{"action":"allow","statement":"DROP TABLE t"}}"#),
    )
    .await;
    let report = s.table.run(&s.plugins, &before_query()).await;
    assert_eq!(report.blocked, None);
    assert_eq!(report.failures.len(), 1);
    assert!(
        report.failures[0].reason.contains("statement"),
        "{report:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hook_past_its_time_no_longer_holds_the_action() {
    let data = tempfile::tempdir().unwrap();
    let limits = HookLimits {
        sandbox: Limits {
            // Well past the wall time, stopped by its fuel in the end.
            fuel: 100_000_000,
            ..HookLimits::default().sandbox
        },
        time: Duration::from_millis(5),
    };
    let plugins = Plugins::new(data.path()).with_hook_limits(limits);
    let root = tempfile::tempdir().unwrap();
    let hook = format!(
        "(func (export \"{HOOK}\") (param i32 i32) (result i64) (loop $l (br $l)) (i64.const 0))"
    );
    let source = package(root.path(), "slow", BEFORE, &hook);
    let reviewed = plugins.inspect(&source).await.unwrap();
    plugins.install(&source, &reviewed.hash).await.unwrap();
    let table = HookTable::new();
    table.load(&plugins, &plugins.list().unwrap());
    let started = Instant::now();
    let report = table.run(&plugins, &before_query()).await;
    assert!(started.elapsed() < Duration::from_secs(2), "{report:?}");
    assert_eq!(report.blocked, None);
    assert!(report.failures[0].reason.contains("deadline"), "{report:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hook_reaches_only_the_host_functions() {
    // A guest using a host function during its hook, then answering.
    let s = setup();
    let hook = format!(
        r#"(data (i32.const 8192) "{{\22ok\22:{{\22action\22:\22allow\22}}}}")
  (func (export "{HOOK}") (param $ptr i32) (param $len i32) (result i64)
    (call $log (i32.const 2) (local.get $ptr) (local.get $len))
    (call $pack (i32.const 8192) (i32.const 25)))"#
    );
    s.install("sees", BEFORE, &hook).await;
    let report = s.table.run(&s.plugins, &before_query()).await;
    assert_eq!(report, HookReport::default());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_permission_is_never_called() {
    let s = setup();
    // A module that would block everything, should it run.
    s.install("guard", BEFORE, &answering(BLOCK)).await;
    assert!(s.table.wanted(HookEvent::BeforeQuery));

    // The approval taken away: the plugin stays installed and nothing of it runs.
    let state_file = s.data.path().join("plugins/plugins.json");
    let mut state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_file).unwrap()).unwrap();
    state["plugins"]["guard"]["approved"]["hooks"] = json!(false);
    std::fs::write(&state_file, state.to_string()).unwrap();
    s.reload();
    assert!(!s.table.wanted(HookEvent::BeforeQuery));
    assert_eq!(
        s.table.run(&s.plugins, &before_query()).await,
        HookReport::default()
    );
    let e = s.plugins.require("guard", Capability::Hooks).unwrap_err();
    assert!(matches!(e, PluginError::Denied(_)), "{e}");
    assert!(e.to_string().contains("no permission"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_turned_off_or_removed_runs_no_hook() {
    let s = setup();
    s.install("guard", BEFORE, &answering(BLOCK)).await;
    s.plugins.set_enabled("guard", false).unwrap();
    s.reload();
    assert!(!s.table.wanted(HookEvent::BeforeQuery));
    s.plugins.set_enabled("guard", true).unwrap();
    s.reload();
    assert!(s.table.wanted(HookEvent::BeforeQuery));
    s.plugins.remove("guard").unwrap();
    s.reload();
    assert!(!s.table.wanted(HookEvent::BeforeQuery));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_module_without_the_entry_point_is_refused_at_install() {
    let s = setup();
    let source = package(s.root.path(), "none", BEFORE, "");
    let e = s.plugins.inspect(&source).await.unwrap_err().to_string();
    assert!(e.contains("platypus_hook"), "{e}");
}

#[test]
fn what_a_hook_is_told_has_no_room_for_values() {
    let cases = [
        (
            serde_json::to_value(before_query()).unwrap(),
            json!({
                "event": "before_query",
                "connection": { "name": "main", "driver": "sqlite", "database": "shop", "production": false },
                "statement": { "text": "SELECT 1", "truncated": false },
            }),
        ),
        (
            serde_json::to_value(HookContext::AfterQuery {
                connection: connection(),
                statement: HookStatement::new(None),
                status: QueryStatus::Error,
                elapsed_ms: 12,
                affected: Some(3),
            })
            .unwrap(),
            json!({
                "event": "after_query",
                "connection": { "name": "main", "driver": "sqlite", "database": "shop", "production": false },
                "statement": { "text": null, "truncated": false },
                "status": "error",
                "elapsed_ms": 12,
                "affected": 3,
            }),
        ),
        (
            serde_json::to_value(HookContext::BeforeSave {
                connection: connection(),
                schema: Some("main".to_owned()),
                table: "items".to_owned(),
                updates: 1,
                inserts: 2,
                deletes: 3,
            })
            .unwrap(),
            json!({
                "event": "before_save",
                "connection": { "name": "main", "driver": "sqlite", "database": "shop", "production": false },
                "schema": "main",
                "table": "items",
                "updates": 1,
                "inserts": 2,
                "deletes": 3,
            }),
        ),
    ];
    for (got, want) in cases {
        assert_eq!(got, want);
    }
}

#[test]
fn a_long_statement_is_cut_on_a_character() {
    let long = "é".repeat(MAX_STATEMENT);
    let statement = HookStatement::new(Some(long));
    assert!(statement.truncated);
    let text = statement.text.unwrap();
    assert!(text.len() <= MAX_STATEMENT);
    assert!(text.chars().all(|c| c == 'é'));
    let short = HookStatement::new(Some("SELECT 1".to_owned()));
    assert!(!short.truncated);
}

#[test]
fn a_message_is_one_short_paragraph() {
    assert_eq!(clean_message("  a\nb\t"), "a b");
    assert_eq!(clean_message(" \n "), "no reason given");
    assert_eq!(
        clean_message(&"x".repeat(MAX_MESSAGE + 10)).chars().count(),
        MAX_MESSAGE
    );
}

#[test]
fn answers_are_only_allow_or_block() {
    let read = |json: Value| serde_json::from_value::<HookAnswer>(json);
    assert_eq!(
        read(json!({"action": "allow"})).unwrap().action,
        HookAction::Allow
    );
    let block = read(json!({"action": "block", "message": "m"})).unwrap();
    assert_eq!(
        (block.action, block.message.as_str()),
        (HookAction::Block, "m")
    );
    assert!(read(json!({})).is_err());
    assert!(read(json!({"action": "allow", "statement": "x"})).is_err());
    assert!(read(json!({"action": "rewrite", "statement": "x"})).is_err());
}
