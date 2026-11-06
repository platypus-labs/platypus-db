//! The hooks and settings of the examples in `examples/plugins`, installed and run as the app
//! does, and what a hook costs a query.
//!
//! The cost is a measurement, not a test (`--release` to get real figures):
//! `cargo test --release -p platypus-plugins --test hooks -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::TryStreamExt;
use platypus_core::{
    Connection, ConnectionConfig, Driver, DriverKind, ExecOptions, ExecResult, SslMode,
};
use platypus_driver_sqlite::SqliteDriver;
use platypus_plugins::{
    ActionOutcome, Capability, HookConnection, HookContext, HookEvent, HookLimits, HookStatement,
    HookTable, Limits, PluginError, Plugins, QueryStatus,
};
use platypus_store::MemorySecrets;
use serde_json::{Value, json};

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins")
        .join(name)
}

async fn install(plugins: &Plugins, source: &Path) -> platypus_plugins::Installed {
    let reviewed = plugins.inspect(source).await.unwrap();
    plugins.install(source, &reviewed.hash).await.unwrap()
}

fn connection() -> HookConnection {
    HookConnection {
        name: "main".to_owned(),
        driver: "sqlite".to_owned(),
        database: "shop".to_owned(),
        production: false,
    }
}

fn before(text: &str) -> HookContext {
    HookContext::BeforeQuery {
        connection: connection(),
        statement: HookStatement::new(Some(text.to_owned())),
    }
}

fn after(text: &str) -> HookContext {
    HookContext::AfterQuery {
        connection: connection(),
        statement: HookStatement::new(Some(text.to_owned())),
        status: QueryStatus::Ok,
        elapsed_ms: 1,
        affected: Some(2),
    }
}

fn plugins(data: &Path) -> Plugins {
    Plugins::new(data)
        .with_secrets(Arc::new(MemorySecrets::default()))
        .with_hook_limits(HookLimits {
            sandbox: Limits {
                // An endless loop should not take the test's time.
                fuel: 20_000_000,
                ..HookLimits::default().sandbox
            },
            time: Duration::from_secs(60),
        })
}

static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn keep_log(_level: &str, line: &str) {
    LOGGED.lock().unwrap().push(line.to_owned());
}

#[tokio::test(flavor = "multi_thread")]
async fn query_guard_blocks_logs_and_survives_breaking() {
    platypus_plugins::set_log_sink(keep_log);
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    let installed = install(&plugins, &example("query-guard")).await;
    assert_eq!(installed.manifest.api, 2);
    assert!(installed.approved.hooks);
    let table = HookTable::new();
    table.load(&plugins, &plugins.list().unwrap());

    // Allowed, and blocked with its message.
    let fine = table.run(&plugins, &before("SELECT * FROM items")).await;
    assert_eq!(fine.blocked, None);
    assert_eq!(fine.failures, []);
    let refused = table
        .run(&plugins, &before("DELETE FROM forbidden_table"))
        .await;
    let block = refused.blocked.unwrap();
    assert_eq!(block.plugin, "Query guard");
    assert_eq!(block.message, "forbidden_table is off limits.");

    // What it sees is what it was given.
    let seen = table
        .run(
            &plugins,
            &before("SELECT 1 FROM show_hook_view WHERE email = '••••••'"),
        )
        .await;
    assert!(seen.blocked.unwrap().message.contains("'••••••'"));

    // The save hook, on the table it locks and another.
    let save = |table: &str| HookContext::BeforeSave {
        connection: connection(),
        schema: None,
        table: table.to_owned(),
        updates: 1,
        inserts: 0,
        deletes: 2,
    };
    let locked = table.run(&plugins, &save("locked_items")).await;
    assert!(locked.blocked.is_some());
    assert_eq!(table.run(&plugins, &save("items")).await.blocked, None);

    // The events that only tell, in the log.
    assert_eq!(table.run(&plugins, &after("SELECT 2")).await.blocked, None);
    let connect = HookContext::OnConnect {
        connection: connection(),
    };
    assert_eq!(table.run(&plugins, &connect).await.blocked, None);
    {
        let log = LOGGED.lock().unwrap();
        let has = |text: &str| log.iter().any(|l| l.contains(text));
        assert!(has("plugin query-guard: after_query"), "{log:?}");
        assert!(has("SELECT 2"), "{log:?}");
        assert!(has("on_connect"), "{log:?}");
        assert!(has("before_save"), "{log:?}");
    }

    // A crash and an endless loop: the action goes on, and the user hears of it once.
    let crash = table.run(&plugins, &before("SELECT hook_crash")).await;
    assert_eq!(crash.blocked, None);
    assert_eq!(crash.failures.len(), 1);
    assert!(crash.failures[0].reason.contains("error"), "{crash:?}");
    let endless = table.run(&plugins, &before("SELECT hook_endless")).await;
    assert_eq!(endless.blocked, None);
    assert!(endless.failures.is_empty(), "told once: {endless:?}");

    // And it keeps working: each call has a new instance.
    let again = table
        .run(&plugins, &before("SELECT * FROM forbidden_table"))
        .await;
    assert!(again.blocked.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn query_guard_without_its_permission_does_nothing() {
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    install(&plugins, &example("query-guard")).await;
    let state_file = data.path().join("plugins/plugins.json");
    let mut state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_file).unwrap()).unwrap();
    state["plugins"]["query-guard"]["approved"]["hooks"] = json!(false);
    std::fs::write(&state_file, state.to_string()).unwrap();

    let table = HookTable::new();
    table.load(&plugins, &plugins.list().unwrap());
    assert!(!table.wanted(HookEvent::BeforeQuery));
    let report = table
        .run(&plugins, &before("DELETE FROM forbidden_table"))
        .await;
    assert_eq!(report.blocked, None);
    assert_eq!(report.failures, []);
    assert!(matches!(
        plugins.require("query-guard", Capability::Hooks),
        Err(PluginError::Denied(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manifest_that_does_not_ask_for_hooks_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("no-permission");
    std::fs::create_dir_all(&source).unwrap();
    let guard = example("query-guard");
    std::fs::copy(guard.join("plugin.wasm"), source.join("plugin.wasm")).unwrap();
    let manifest = std::fs::read_to_string(guard.join("plugin.toml"))
        .unwrap()
        .replace("hooks = true", "");
    std::fs::write(source.join("plugin.toml"), manifest).unwrap();
    let data = tempfile::tempdir().unwrap();
    let e = plugins(data.path()).inspect(&source).await.unwrap_err();
    assert!(e.to_string().contains("permissions.hooks"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_demo_reads_its_own_settings_and_nothing_else() {
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    install(&plugins, &example("settings-demo")).await;

    let message = |outcome| match outcome {
        ActionOutcome::Message { text } => text,
        other => panic!("{other:?}"),
    };
    // The defaults of the manifest, and no secret yet.
    let shown = message(
        plugins
            .run_action("settings-demo", "show", json!({}))
            .await
            .unwrap(),
    );
    assert_eq!(
        shown,
        "endpoint=\"https://example.com\" retries=3 verbose=false mode=\"fast\" token: not set"
    );

    plugins
        .set_setting("settings-demo", "endpoint", &json!("https://x.example"))
        .unwrap();
    plugins
        .set_setting("settings-demo", "retries", &json!(7))
        .unwrap();
    plugins
        .set_setting("settings-demo", "verbose", &json!(true))
        .unwrap();
    plugins
        .set_setting("settings-demo", "mode", &json!("safe"))
        .unwrap();
    plugins
        .set_setting("settings-demo", "token", &json!("s3cret-value"))
        .unwrap();
    let shown = message(
        plugins
            .run_action("settings-demo", "show", json!({}))
            .await
            .unwrap(),
    );
    assert_eq!(
        shown,
        "endpoint=\"https://x.example\" retries=7 verbose=true mode=\"safe\" token: 12 characters"
    );

    // The secret is in no file of the data folder, and the page does not get it.
    let states = plugins.settings_of("settings-demo").unwrap();
    let token = states.iter().find(|s| s.key == "token").unwrap();
    assert!(token.set && token.value.is_none());
    assert_no_file_holds(data.path(), "s3cret-value");

    // A key the manifest does not declare: refused, and the action goes on.
    let refused = message(
        plugins
            .run_action("settings-demo", "other", json!({}))
            .await
            .unwrap(),
    );
    assert!(refused.starts_with("Refused: not allowed"), "{refused}");
    assert!(refused.contains("password_of_another_plugin"), "{refused}");
}

fn assert_no_file_holds(dir: &Path, text: &str) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            assert_no_file_holds(&path, text);
        } else if let Ok(content) = std::fs::read(&path) {
            let found = content.windows(text.len()).any(|w| w == text.as_bytes());
            assert!(!found, "{} holds it", path.display());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_of_abi_1_is_as_it_was() {
    let data = tempfile::tempdir().unwrap();
    let plugins = plugins(data.path());
    let installed = install(&plugins, &example("sql-tools")).await;
    assert_eq!(installed.manifest.api, 1);
    assert!(installed.approved.clipboard);
    let outcome = plugins
        .run_action(
            "sql-tools",
            "uppercase",
            json!({ "text": "select 1", "selection": null, "driver": "sqlite" }),
        )
        .await
        .unwrap();
    assert_eq!(
        outcome,
        ActionOutcome::Replace {
            text: "SELECT 1".to_owned()
        }
    );
    let table = HookTable::new();
    table.load(&plugins, &plugins.list().unwrap());
    for event in [HookEvent::BeforeQuery, HookEvent::AfterQuery] {
        assert!(!table.wanted(event));
    }
    assert_eq!(plugins.settings_of("sql-tools").unwrap(), []);
}

async fn run_query(conn: &dyn Connection, sql: &str) {
    match conn.execute(sql, &ExecOptions::default()).await.unwrap() {
        ExecResult::Rows(stream) => {
            let _: Vec<_> = stream.try_collect().await.unwrap();
        }
        ExecResult::Affected { .. } => {}
    }
}

/// Runs `one` `count` times; the time of one, on average.
async fn each<F: Future<Output = ()>>(count: u32, mut one: impl FnMut() -> F) -> Duration {
    let started = Instant::now();
    for _ in 0..count {
        one().await;
    }
    started.elapsed() / count
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "a measurement, see the top of this file"]
async fn what_a_hook_costs_a_query() {
    const QUERIES: u32 = 2000;
    const SQL: &str = "SELECT 1";
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let mut config = ConnectionConfig::new(DriverKind::Sqlite);
    config.database = ":memory:".to_owned();
    config.ssl_mode = SslMode::Disable;
    let conn = SqliteDriver.connect(&config).await.unwrap();
    let conn: &dyn Connection = conn.as_ref();

    let empty = HookTable::new();
    empty.load(&plugins, &plugins.list().unwrap());
    let without = each(QUERIES, || async {
        if empty.wanted(HookEvent::BeforeQuery) || empty.wanted(HookEvent::AfterQuery) {
            unreachable!("no hook");
        }
        run_query(conn, SQL).await;
    })
    .await;
    let bare = each(QUERIES, || run_query(conn, SQL)).await;
    let started = Instant::now();
    for _ in 0..1_000_000 {
        std::hint::black_box(empty.wanted(std::hint::black_box(HookEvent::BeforeQuery)));
    }
    let check = started.elapsed() / 1_000_000;

    install(&plugins, &example("query-guard")).await;
    let hooked = HookTable::new();
    hooked.load(&plugins, &plugins.list().unwrap());
    let started = Instant::now();
    let first = hooked.run(&plugins, &before(SQL)).await;
    let first_call = started.elapsed();
    assert_eq!(first.blocked, None);
    let with_before = each(QUERIES, || async {
        hooked.run(&plugins, &before(SQL)).await;
        run_query(conn, SQL).await;
    })
    .await;
    let with_both = each(QUERIES, || async {
        hooked.run(&plugins, &before(SQL)).await;
        run_query(conn, SQL).await;
        hooked.run(&plugins, &after(SQL)).await;
    })
    .await;
    println!(
        "`wanted`: {check:?}; `{SQL}` on SQLite: {bare:?} bare, {without:?} with the check and no hook, \
         {with_before:?} with a before hook, {with_both:?} with before and after (awaited here, \
         not in the app); first call of a hook, compiling its module: {first_call:?}"
    );
}
