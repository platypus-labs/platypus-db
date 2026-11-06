//! The grid extensions of `examples/plugins/grid-demo`, installed and run as the app does, and what
//! a batch costs.
//!
//! The cost is a measurement, not a test (`--release` to get real figures):
//! `cargo test --release -p platypus-plugins --test grid -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use platypus_plugins::{
    ActionOutcome, Capability, GridColumn, GridLimits, GridRendered, GridValue, Limits,
    PluginError, Plugins,
};
use serde_json::{Value, json};

const ID: &str = "grid-demo";

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/grid-demo")
}

fn plugins(data: &Path) -> Plugins {
    Plugins::new(data).with_grid_limits(GridLimits {
        sandbox: Limits {
            // An endless loop should not take the test's time.
            fuel: 20_000_000,
            ..GridLimits::default().sandbox
        },
        time: Duration::from_secs(60),
    })
}

async fn installed(data: &Path) -> Plugins {
    let plugins = plugins(data);
    let reviewed = plugins.inspect(&example()).await.unwrap();
    plugins.install(&example(), &reviewed.hash).await.unwrap();
    plugins
}

fn column(name: &str) -> GridColumn {
    GridColumn {
        name: name.to_owned(),
        db_type: "text".to_owned(),
        masked: false,
    }
}

fn row(values: &[&str]) -> Vec<Option<String>> {
    values.iter().map(|v| Some((*v).to_owned())).collect()
}

fn value(text: &str) -> GridValue {
    GridValue::Value {
        text: Some(text.to_owned()),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_computed_column_answers_a_batch() {
    let data = tempfile::tempdir().unwrap();
    let plugins = installed(data.path()).await;
    let values = plugins
        .grid_column(
            ID,
            "length",
            vec![column("a"), column("b")],
            vec![row(&["ab", "cde"]), vec![None, Some("xyz".to_owned())]],
        )
        .await
        .unwrap();
    assert_eq!(values, [value("5"), value("3")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_row_costs_its_own_cell() {
    let data = tempfile::tempdir().unwrap();
    let plugins = installed(data.path()).await;
    let values = plugins
        .grid_column(
            ID,
            "length",
            vec![column("a")],
            vec![row(&["ok"]), row(&["boom"]), row(&["fine"])],
        )
        .await
        .unwrap();
    assert_eq!(values[0], value("2"));
    assert_eq!(
        values[1],
        GridValue::Error {
            error: "this row cannot be counted".to_owned()
        }
    );
    assert_eq!(values[2], value("4"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_or_an_endless_loop_is_the_error_of_its_batch() {
    let data = tempfile::tempdir().unwrap();
    let plugins = installed(data.path()).await;
    let crash = plugins
        .grid_column(ID, "length", vec![column("a")], vec![row(&["crash"])])
        .await
        .unwrap_err();
    assert!(matches!(crash, PluginError::Trap(_)), "{crash}");
    let endless = plugins
        .grid_column(ID, "length", vec![column("a")], vec![row(&["endless"])])
        .await
        .unwrap_err();
    assert!(matches!(endless, PluginError::OutOfFuel), "{endless}");
    // The next batch starts from a new sandbox.
    let again = plugins
        .grid_column(ID, "length", vec![column("a")], vec![row(&["ok"])])
        .await
        .unwrap();
    assert_eq!(again, [value("2")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_batch_past_its_time_stops_waiting() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path()).with_grid_limits(GridLimits {
        sandbox: Limits {
            // Enough to outlast `time`, and the runtime still ends with the test.
            fuel: 100_000_000,
            ..GridLimits::default().sandbox
        },
        time: Duration::from_millis(5),
    });
    let reviewed = plugins.inspect(&example()).await.unwrap();
    plugins.install(&example(), &reviewed.hash).await.unwrap();
    let start = Instant::now();
    let e = plugins
        .grid_column(ID, "length", vec![column("a")], vec![row(&["endless"])])
        .await
        .unwrap_err();
    assert!(matches!(e, PluginError::Timeout), "{e}");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_renderer_says_how_to_draw_and_nothing_more() {
    let data = tempfile::tempdir().unwrap();
    let plugins = installed(data.path()).await;
    let cells = plugins
        .grid_render(
            ID,
            "status",
            column("state"),
            ["ok", "failed", "other", "markup"]
                .map(str::to_owned)
                .to_vec(),
        )
        .await
        .unwrap();
    let as_json: Vec<Value> = cells
        .iter()
        .map(|c| serde_json::to_value(c).unwrap())
        .collect();
    assert_eq!(
        as_json[0],
        json!({"cell": {"color": "#1a7f37", "hint": "fine"}})
    );
    assert_eq!(
        as_json[1],
        json!({"cell": {"text": "FAILED", "background": "#ffd7d5"}})
    );
    assert_eq!(as_json[2], json!({"cell": null}));
    // Asking to draw markup is an error of that cell: there is no such thing to draw.
    assert!(
        matches!(cells[3], GridRendered::Error { .. }),
        "{:?}",
        cells[3]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_masked_value_is_only_dots_to_the_plugin() {
    let data = tempfile::tempdir().unwrap();
    let plugins = installed(data.path()).await;
    // The page sends what the grid shows: all the module can count is the dots.
    let masked = GridColumn {
        masked: true,
        ..column("email")
    };
    let values = plugins
        .grid_column(ID, "length", vec![masked], vec![row(&["••••••"])])
        .await
        .unwrap();
    assert_eq!(values, [value("6")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_answers_as_an_action_does() {
    let data = tempfile::tempdir().unwrap();
    let plugins = installed(data.path()).await;
    let outcome = plugins
        .grid_action(ID, "shout", column("a"), Some("hey".to_owned()))
        .await
        .unwrap();
    assert_eq!(
        outcome,
        ActionOutcome::Message {
            text: "HEY".to_owned()
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn only_what_it_declares_of_each_kind_runs() {
    let data = tempfile::tempdir().unwrap();
    let plugins = installed(data.path()).await;
    let cases = [
        plugins
            .grid_column(ID, "nope", vec![], vec![])
            .await
            .map(|_| ()),
        // A renderer is not a column, or a column a command.
        plugins
            .grid_column(ID, "status", vec![], vec![])
            .await
            .map(|_| ()),
        plugins
            .grid_render(ID, "length", column("a"), vec![])
            .await
            .map(|_| ()),
        plugins
            .grid_action(ID, "length", column("a"), None)
            .await
            .map(|_| ()),
    ];
    for case in cases {
        assert!(matches!(case, Err(PluginError::Denied(_))), "{case:?}");
    }
    let too_many = vec![row(&["x"]); platypus_plugins::grid::MAX_BATCH + 1];
    let e = plugins
        .grid_column(ID, "length", vec![column("a")], too_many)
        .await
        .unwrap_err();
    assert!(matches!(e, PluginError::Denied(_)), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn without_its_permission_nothing_runs() {
    let data = tempfile::tempdir().unwrap();
    let plugins = installed(data.path()).await;
    let file = data.path().join("plugins/plugins.json");
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    state["plugins"][ID]["approved"]["grid"] = json!(false);
    std::fs::write(&file, state.to_string()).unwrap();

    assert!(matches!(
        plugins.require(ID, Capability::Grid),
        Err(PluginError::Denied(_))
    ));
    let column_call = plugins
        .grid_column(ID, "length", vec![column("a")], vec![row(&["crash"])])
        .await;
    assert!(matches!(column_call, Err(PluginError::Denied(_))));
    let render = plugins
        .grid_render(ID, "status", column("a"), vec!["ok".to_owned()])
        .await;
    assert!(matches!(render, Err(PluginError::Denied(_))));
    let action = plugins.grid_action(ID, "shout", column("a"), None).await;
    assert!(matches!(action, Err(PluginError::Denied(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_module_without_the_entry_point_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let package = tempfile::tempdir().unwrap();
    std::fs::copy(
        example().join("plugin.toml"),
        package.path().join("plugin.toml"),
    )
    .unwrap();
    // A module of actions has no `platypus_grid`.
    std::fs::copy(
        example().join("../query-guard/plugin.wasm"),
        package.path().join("plugin.wasm"),
    )
    .unwrap();
    let e = plugins(data.path())
        .inspect(package.path())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("platypus_grid"), "{e}");
}

/// What a batch of the size the page sends costs; the page sends at most one per extension at a
/// time, away from drawing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a measurement"]
async fn cost_of_a_batch() {
    let data = tempfile::tempdir().unwrap();
    let plugins = installed(data.path()).await;
    let plugins = plugins.with_grid_limits(GridLimits::default());
    for size in [1, 50, 200, 1000] {
        let rows = vec![row(&["a value of a typical length, some 40 chars"]); size];
        let values: Vec<String> = (0..size).map(|i| format!("value {i}")).collect();
        let rounds = 50_u32;
        let start = Instant::now();
        for _ in 0..rounds {
            plugins
                .grid_column(ID, "length", vec![column("a")], rows.clone())
                .await
                .unwrap();
        }
        let per_column = start.elapsed() / rounds;
        let start = Instant::now();
        for _ in 0..rounds {
            plugins
                .grid_render(ID, "status", column("a"), values.clone())
                .await
                .unwrap();
        }
        println!(
            "{size} per batch: column {per_column:?}, renderer {:?}",
            start.elapsed() / rounds
        );
    }
}
