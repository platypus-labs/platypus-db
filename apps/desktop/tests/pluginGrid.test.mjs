// A result's grid with what plugins add: no call on the path of a frame, one batch at a time, masked
// values as dots, a failure that stays in its column; and what scrolling a large result costs.
//
// The figures of the last test show with `node --test --test-reporter=spec tests/pluginGrid.test.mjs`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_VALUE, Notes, createGridView, typeMatches } from "../src/plugins/gridView.ts";

const MASK = "••••••";
const settle = () => new Promise((done) => setTimeout(done, 60));

const column = (name, db_type = "text") => ({ name, db_type, numeric: false, binary: false });

function plugin(grid, over = {}) {
  return {
    manifest: { id: "demo", name: "Demo", grid },
    enabled: true,
    approved: { http: [], clipboard: false, hooks: false, tabs: false, grid: true },
    description: { actions: [], driver: null },
    ...over,
  };
}

/** A result of `count` rows: `name` is `n<row>`, `state` cycles three values. */
function result(count) {
  return {
    get: (row, col) => {
      if (row >= count) return undefined;
      if (col === 0) return `n${row}`;
      if (col === 1) return ["ok", "failed", "other"][row % 3];
      return undefined;
    },
    ensure: () => {},
  };
}

function backend(over = {}) {
  const calls = { column: [], render: [], failed: [] };
  return {
    calls,
    mask: MASK,
    column: async (plugin, extension, columns, rows) => {
      calls.column.push({ plugin, extension, columns, rows });
      return rows.map((r) => ({ text: String(r.join("").length) }));
    },
    render: async (plugin, extension, column, values) => {
      calls.render.push({ plugin, extension, column, values });
      return values.map((v) => ({ cell: v === "ok" ? { color: "#0f0", hint: "fine" } : null }));
    },
    failed: (p, title, reason) => calls.failed.push({ title, reason }),
    reason: (e) => String(e?.message ?? e),
    ...over,
  };
}

function view(grid, { count = 1000, masked = [false, false], topRows = 0, shown, plugins, api = backend(), columns = [column("name"), column("state", "status")] } = {}) {
  let redraws = 0;
  const made = createGridView(
    { columns, shown: shown ?? columns, rows: result(count), masked, topRows, redraw: () => (redraws += 1) },
    plugins ?? [plugin(grid)],
    api,
  );
  return { view: made, api, redraws: () => redraws };
}

const LENGTH = { columns: [{ id: "length", title: "Length", types: ["text"] }] };
const STATUS = { renderers: [{ id: "status", title: "Status", types: ["status"] }] };

test("a type matches without its size or case; no types match all", () => {
  assert.ok(typeMatches(undefined, "anything"));
  assert.ok(typeMatches([], "anything"));
  assert.ok(typeMatches(["text"], "TEXT"));
  assert.ok(typeMatches(["varchar"], "VARCHAR(255)"));
  assert.ok(typeMatches(["VarChar"], "varchar"));
  assert.ok(!typeMatches(["text"], "text2"));
  assert.ok(!typeMatches(["int"], "text"));
});

test("without a usable plugin there is nothing to go through", () => {
  const off = (over) => [plugin(LENGTH, over)];
  assert.equal(view(null, { plugins: [] }).view, null);
  assert.equal(view(null, { plugins: off({ enabled: false }) }).view, null);
  assert.equal(view(null, { plugins: off({ approved: { grid: false } }) }).view, null);
  assert.equal(view(null, { plugins: [plugin(undefined)] }).view, null);
});

test("a computed column asks in batches away from the frame, and caches by row", async () => {
  const { view: grid, api, redraws } = view(LENGTH);
  assert.equal(grid.columns.at(-1).name, "Length");
  const at = grid.columns.length - 1;
  // A frame: nothing is called while it draws.
  for (let row = 0; row < 40; row += 1) assert.equal(grid.rows.get(row, at), undefined);
  assert.equal(api.calls.column.length, 0);
  await settle();
  assert.equal(api.calls.column.length, 1);
  assert.equal(api.calls.column[0].rows.length, 40);
  assert.equal(redraws(), 1);
  // Only the columns it applies to, as `types` says.
  assert.deepEqual(api.calls.column[0].columns, [{ name: "name", type: "text", masked: false }]);
  assert.equal(grid.rows.get(7, at), "2");
  // Scrolling back costs a lookup.
  for (let row = 0; row < 40; row += 1) grid.rows.get(row, at);
  await settle();
  assert.equal(api.calls.column.length, 1);
  // The result's own columns go through untouched.
  assert.equal(grid.rows.get(7, 0), "n7");
  grid.dispose();
});

test("a type no column has adds no column", () => {
  const { view: grid } = view({ columns: [{ id: "x", title: "X", types: ["jsonb"] }] });
  assert.equal(grid.columns.length, 2);
  assert.equal(grid.rows.get(0, 2), undefined);
  grid.dispose();
});

test("a plugin's column does not take the name of another", () => {
  const taken = [column("Length"), column("state", "status")];
  const { view: grid } = view(LENGTH, { columns: taken, shown: [...taken, column("Length (Demo)")] });
  assert.equal(grid.columns.at(-1).name, "Length (Demo 2)");
  grid.dispose();
});

test("computed columns come after the user's", async () => {
  const columns = [column("name"), column("state", "status")];
  const shown = [...columns, column("mine", "= 1")];
  const { view: grid, api } = view(LENGTH, { columns, shown });
  assert.equal(grid.columns.length, 4);
  assert.equal(grid.columns[2].name, "mine");
  grid.rows.get(0, 3);
  await settle();
  assert.equal(api.calls.column.length, 1);
  grid.dispose();
});

test("a row not loaded is not asked for, and is noted again when it is drawn", async () => {
  const { view: grid, api } = view(LENGTH, { count: 10 });
  const at = grid.columns.length - 1;
  grid.rows.get(50, at);
  await settle();
  assert.equal(api.calls.column.length, 0);
  grid.rows.get(5, at);
  await settle();
  assert.equal(api.calls.column.length, 1);
  grid.dispose();
});

test("a plugin reads a masked cell as dots, and NULL as NULL", async () => {
  const seen = [];
  const api = backend({
    column: async (_, __, columns, rows) => {
      seen.push({ columns, rows });
      return rows.map(() => ({ text: "x" }));
    },
  });
  const columns = [column("email"), column("note")];
  const rows = {
    get: (row, col) => (col === 0 ? (row === 0 ? "secret@example.com" : null) : "plain"),
    ensure: () => {},
  };
  const grid = createGridView(
    { columns, shown: columns, rows, masked: [true, false], topRows: 0, redraw: () => {} },
    [plugin({ columns: [{ id: "c", title: "C" }] })],
    api,
  );
  grid.rows.get(0, 2);
  grid.rows.get(1, 2);
  await settle();
  assert.deepEqual(seen[0].columns, [
    { name: "email", type: "text", masked: true },
    { name: "note", type: "text", masked: false },
  ]);
  assert.deepEqual(seen[0].rows, [
    [MASK, "plain"],
    [null, "plain"],
  ]);
  assert.ok(!JSON.stringify(seen).includes("secret@example.com"));
  // The same for a command on the cell.
  assert.deepEqual(grid.cell(0, 0), { column: { name: "email", type: "text", masked: true }, value: MASK });
  assert.equal(grid.cell(1, 0).value, null);
  assert.equal(grid.cell(0, 1).value, "plain");
  grid.dispose();
});

test("a renderer asks once per value, and gets a masked value as dots", async () => {
  const { view: grid, api, redraws } = view(STATUS);
  assert.equal(grid.columns.length, 2);
  // Other columns cost one check.
  assert.equal(grid.decorate(0, 0), undefined);
  for (let row = 0; row < 300; row += 1) assert.equal(grid.decorate(row, 1), undefined);
  assert.equal(api.calls.render.length, 0);
  await settle();
  assert.equal(api.calls.render.length, 1);
  assert.deepEqual([...api.calls.render[0].values].sort(), ["failed", "ok", "other"]);
  assert.equal(redraws(), 1);
  assert.deepEqual(grid.decorate(0, 1), { color: "#0f0", hint: "fine" });
  assert.equal(grid.decorate(1, 1), undefined);
  for (let row = 0; row < 300; row += 1) grid.decorate(row, 1);
  await settle();
  assert.equal(api.calls.render.length, 1);
  grid.dispose();

  const masked = view(STATUS, { masked: [false, true] });
  for (let row = 0; row < 30; row += 1) masked.view.decorate(row, 1);
  await settle();
  assert.deepEqual(masked.api.calls.render[0].values, [MASK]);
  assert.equal(masked.api.calls.render[0].column.masked, true);
  masked.view.dispose();
});

test("the rows on top are not a renderer's, and NULL is not drawn by it", async () => {
  const { view: grid, api } = view(STATUS, { topRows: 2 });
  assert.equal(grid.decorate(0, 1), undefined);
  assert.equal(grid.decorate(1, 1), undefined);
  grid.decorate(2, 1);
  await settle();
  assert.deepEqual(api.calls.render[0].values, ["ok"]);
  assert.equal(grid.cell(0, 1), null);
  assert.equal(grid.cell(2, 1).value, "ok");
  grid.dispose();

  const rows = { get: () => null, ensure: () => {} };
  const api2 = backend();
  const columns = [column("state", "status")];
  const nulls = createGridView({ columns, shown: columns, rows, masked: [false], topRows: 0, redraw: () => {} }, [plugin(STATUS)], api2);
  assert.equal(nulls.decorate(0, 0), undefined);
  await settle();
  assert.equal(api2.calls.render.length, 0);
  nulls.dispose();
});

test("what a renderer can say is a decoration, and a bad cell shows as such", async () => {
  const api = backend({
    render: async (_, __, ___, values) =>
      values.map((v) =>
        v === "ok" ? { cell: { text: "T", color: "#111", background: "#222", hint: "H", strike: true } } : v === "failed" ? { error: "bad value" } : { cell: null },
      ),
  });
  const { view: grid } = view(STATUS, { api });
  for (let row = 0; row < 3; row += 1) grid.decorate(row, 1);
  await settle();
  assert.deepEqual(grid.decorate(0, 1), { text: "T", color: "#111", background: "#222", hint: "H", strike: true });
  assert.deepEqual(grid.decorate(1, 1), { hint: "⚠ bad value" });
  assert.equal(grid.decorate(2, 1), undefined);
  grid.dispose();
});

test("a value too long is drawn as it is, or cut for a column", async () => {
  const long = "x".repeat(MAX_VALUE + 1);
  const rows = { get: () => long, ensure: () => {} };
  const api = backend();
  const columns = [column("state", "status"), column("name")];
  const grid = createGridView({ columns, shown: columns, rows, masked: [], topRows: 0, redraw: () => {} }, [plugin({ ...STATUS, ...LENGTH })], api);
  assert.equal(grid.decorate(0, 0), undefined);
  grid.rows.get(0, 2);
  await settle();
  assert.equal(api.calls.render.length, 0);
  assert.equal(api.calls.column[0].rows[0][0].length, MAX_VALUE);
  grid.dispose();
});

test("a batch that fails shows in its column's cells, asks no more, and tells once", async () => {
  let asked = 0;
  const api = backend({
    column: async () => {
      asked += 1;
      throw new Error("the plugin used up its time");
    },
  });
  const { view: grid, redraws } = view({ ...LENGTH, ...STATUS }, { api });
  const at = grid.columns.length - 1;
  grid.rows.get(0, at);
  await settle();
  assert.equal(asked, 1);
  assert.equal(grid.rows.get(0, at), "⚠ the plugin used up its time");
  assert.equal(grid.rows.get(999, at), "⚠ the plugin used up its time");
  await settle();
  assert.equal(asked, 1);
  assert.equal(redraws(), 1);
  assert.deepEqual(api.calls.failed, [{ title: "Length", reason: "the plugin used up its time" }]);
  // The grid goes on.
  assert.equal(grid.rows.get(0, 0), "n0");
  grid.dispose();
});

test("an error of one row stays in that row", async () => {
  const api = backend({
    column: async (_, __, ___, rows) => rows.map((_r, i) => (i === 0 ? { error: "no" } : { text: null })),
  });
  const { view: grid } = view(LENGTH, { api });
  const at = grid.columns.length - 1;
  grid.rows.get(0, at);
  grid.rows.get(1, at);
  await settle();
  assert.equal(grid.rows.get(0, at), "⚠ no");
  assert.equal(grid.rows.get(1, at), null);
  grid.dispose();
});

test("a renderer whose batch fails goes back to the plain value", async () => {
  const api = backend({ render: () => Promise.reject(new Error("trap")) });
  const { view: grid } = view(STATUS, { api });
  grid.decorate(0, 1);
  await settle();
  assert.equal(grid.decorate(0, 1), undefined);
  assert.equal(api.calls.failed.length, 1);
  grid.dispose();
});

test("one batch at a time per extension, the newest first, the rest forgotten", async () => {
  let release;
  const batches = [];
  const api = backend({
    column: (_, __, ___, rows) => {
      batches.push(rows.length);
      return new Promise((resolve) => {
        release = () => resolve(rows.map(() => ({ text: "x" })));
      });
    },
  });
  const { view: grid } = view(LENGTH, { count: 1_000_000, api });
  const at = grid.columns.length - 1;
  for (let row = 0; row < 5000; row += 1) grid.rows.get(row, at);
  await settle();
  assert.deepEqual(batches, [200]);
  // Newest first: the rows at the end are what is on screen after the scroll.
  assert.equal(grid.rows.get(4999, at), undefined);
  for (let row = 5000; row < 5040; row += 1) grid.rows.get(row, at);
  await settle();
  assert.deepEqual(batches, [200]);
  release();
  await settle();
  assert.deepEqual(batches, [200, 41]);
  release();
  await settle();
  grid.dispose();
});

test("notes keep the latest keys and forget the rest", () => {
  const notes = new Notes();
  for (const k of [1, 2, 3, 4, 2]) notes.add(k);
  assert.equal(notes.size, 4);
  assert.deepEqual(notes.take(2), [3, 4]);
  assert.equal(notes.size, 0);
});

test("after dispose nothing more is asked", async () => {
  const { view: grid, api } = view(LENGTH);
  grid.rows.get(0, grid.columns.length - 1);
  grid.dispose();
  await settle();
  assert.equal(api.calls.column.length, 0);
});

test("a command reads only the result's columns", () => {
  const { view: grid } = view(LENGTH);
  assert.equal(grid.cell(0, 2), null);
  assert.equal(grid.cell(5000, 0), null);
  grid.dispose();
});

/** Reads the cells of a viewport as `DataGrid.draw` does, `frames` times over a scroll. */
function scroll(decorate, get, { frames, rows, columns, step }) {
  const start = performance.now();
  let sink = 0;
  for (let frame = 0; frame < frames; frame += 1) {
    const first = frame * step;
    for (let c = 0; c < columns; c += 1) {
      for (let r = first; r < first + rows; r += 1) {
        if (decorate) sink += decorate(r, c) ? 1 : 0;
        const value = get(r, c);
        sink += value == null ? 0 : value.length;
      }
    }
  }
  return { ms: (performance.now() - start) / frames, sink };
}

test("what scrolling a large result costs, with and without an extension", async (t) => {
  // A viewport of 45 rows and 12 columns on a million rows, as a fast scroll draws it.
  const shape = { frames: 2000, rows: 45, columns: 12, step: 400 };
  const COUNT = 1_000_000;
  const values = ["ok", "failed", "other"];
  const base = { get: (row, col) => (col === 1 ? values[row % 3] : `n${row}`), ensure: () => {} };
  const columns = Array.from({ length: 12 }, (_, i) => column(`c${i}`, i === 1 ? "status" : "text"));
  const attach = (grid, api) => createGridView({ columns, shown: columns, rows: base, masked: [], topRows: 0, redraw: () => {} }, [plugin(grid)], api);

  // Warm the engine the same way for each.
  scroll(undefined, base.get, shape);
  const bare = scroll(undefined, base.get, shape);

  const renderer = backend();
  const rendered = attach(STATUS, renderer);
  scroll(rendered.decorate, rendered.rows.get, { ...shape, frames: 10 });
  await settle();
  const withRenderer = scroll(rendered.decorate, rendered.rows.get, shape);
  await settle();
  rendered.dispose();

  const computed = backend();
  const added = attach({ columns: [{ id: "c", title: "C" }] }, computed);
  const withColumn = scroll(undefined, added.rows.get, { ...shape, columns: 13 });
  await settle();
  added.dispose();

  t.diagnostic(`a frame's cell reads, ${shape.rows} rows x ${shape.columns} columns on ${COUNT} rows:`);
  t.diagnostic(`  no extension: ${bare.ms.toFixed(4)} ms`);
  t.diagnostic(`  a renderer on one column, values seen: ${withRenderer.ms.toFixed(4)} ms, ${renderer.calls.render.length} batch(es)`);
  t.diagnostic(`  a computed column, every frame new rows: ${withColumn.ms.toFixed(4)} ms, ${computed.calls.column.length} batch(es) for ${shape.frames} frames`);

  // What is pinned down is the calls: none per frame, whatever the scroll.
  assert.equal(renderer.calls.render.length, 1);
  assert.ok(computed.calls.column.length <= 2, String(computed.calls.column.length));
  // And a frame stays far below its 16 ms.
  assert.ok(withRenderer.ms < 8, `${withRenderer.ms} ms`);
  assert.ok(withColumn.ms < 8, `${withColumn.ms} ms`);
});
