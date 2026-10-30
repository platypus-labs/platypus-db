// Unit tests of the automatic layout of the ER diagram (src/erd/layout.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { autoLayout, nearestInOrder, placeTables } from "../src/erd/layout.ts";
import { pathThrough, quickWire, routeWires } from "../src/erd/route.ts";

const box = (name, refs = [], height = 50) => ({ name, width: 100, height, refs });

test("a table goes right of the tables it references", () => {
  const at = autoLayout([box("items", ["orders", "products"]), box("orders", ["customers"]), box("customers"), box("products")]);
  assert.ok(at.customers.x < at.orders.x);
  assert.ok(at.orders.x < at.items.x);
  // A table referenced by one table only goes in the column just left of it.
  assert.equal(at.products.x, at.orders.x);
});

test("tables without foreign keys go in a grid below the others", () => {
  const at = autoLayout([box("a", ["b"]), box("b"), box("x"), box("y"), box("z")]);
  assert.ok(at.x.y > at.b.y + 50);
  assert.equal(at.x.y, at.y.y);
  assert.ok(at.z.y > at.x.y);
});

test("cycles and missing tables do not stop the layout", () => {
  const at = autoLayout([box("a", ["b"]), box("b", ["a"]), box("c", ["gone"])]);
  assert.deepEqual(Object.keys(at).sort(), ["a", "b", "c"]);
  assert.notEqual(at.a.x, at.b.x);
});

test("no two tables of a column overlap", () => {
  const at = autoLayout([box("p"), box("a", ["p"], 80), box("b", ["p"], 80), box("c", ["p"], 80)]);
  for (const [m, n] of [["a", "b"], ["a", "c"], ["b", "c"]]) {
    if (at[m].x === at[n].x) assert.ok(Math.abs(at[m].y - at[n].y) >= 80, JSON.stringify(at));
  }
});

test("placeTables keeps the placed tables and puts the others below them", () => {
  const fixed = { a: { x: 500, y: 300 }, b: { x: 20, y: 20 } };
  const at = placeTables([box("a", ["b"]), box("b"), box("c", ["a"]), box("d")], fixed);
  assert.deepEqual(at.a, fixed.a);
  assert.deepEqual(at.b, fixed.b);
  const bottom = 300 + 50;
  assert.ok(at.c.y >= bottom && at.d.y >= bottom, JSON.stringify(at));
  assert.deepEqual(Object.keys(at).sort(), ["a", "b", "c", "d"]);
});

test("placeTables lays out everything when nothing is placed, and ignores positions of dropped tables", () => {
  const boxes = [box("a", ["b"]), box("b"), box("c")];
  assert.deepEqual(placeTables(boxes, {}), autoLayout(boxes));
  assert.deepEqual(placeTables(boxes, { dropped: { x: 9, y: 9 } }), autoLayout(boxes));
});

test("placeTables has nothing to lay out when all are placed", () => {
  const fixed = { a: { x: 1, y: 2 }, b: { x: 3, y: 4 } };
  assert.deepEqual(placeTables([box("a"), box("b")], fixed), fixed);
});

// Quality of the layout on sample schemas: no box on another, no line through a table, and far
// fewer crossings than the first algorithm (a column per foreign key depth, ordered once).

/** A box of `columns` rows, as the diagram draws it, from 140 to 290 pixels wide. */
const table = (name, refs = [], columns = 4) => ({ name, width: 140 + ((name.length * 23 + name.charCodeAt(name.length - 1)) % 7) * 25, height: 34 + 20 * columns, refs });

/** A deterministic pseudo-random number generator, so that the schemas are always the same. */
function random(seed) {
  let s = seed;
  return () => {
    s = (s * 1103515245 + 12345) % 2147483648;
    return s / 2147483648;
  };
}

const SCHEMAS = {
  star: () => [
    table("users"),
    table("studies", ["users"]),
    ...Array.from({ length: 14 }, (_, i) => table(`u_${i}`, ["users"], 3 + (i % 4))),
    ...Array.from({ length: 12 }, (_, i) => table(`s_${i}`, ["studies", "users"], 2 + (i % 5))),
  ],
  chain: () => Array.from({ length: 20 }, (_, i) => table(`c_${i}`, i ? [`c_${i - 1}`] : [])),
  twoGroups: () => [
    table("a"),
    table("a1", ["a"]),
    table("a2", ["a"]),
    table("a3", ["a1", "a2"]),
    table("b"),
    table("b1", ["b"]),
    table("b2", ["b1"]),
    table("b3", ["b", "b2"]),
  ],
  cycle: () => [table("x", ["z"]), table("y", ["x"]), table("z", ["y"]), table("w", ["x", "z"])],
  selfReference: () => [table("tree", ["tree"]), table("node", ["tree", "node"]), table("leaf", ["node"]), table("alone", ["alone"])],
  realistic: () => realistic(60, 7),
  large: () => realistic(300, 11),
};

/** A schema like a real one: users and studies referenced by nearly everything, modules of a
 *  head table and its detail tables (some referencing other details), a few references across
 *  modules, a cycle and some tables alone. */
function realistic(n, seed) {
  const rnd = random(seed);
  const pick = (list) => list[Math.floor(rnd() * list.length)];
  const out = [table("users", [], 8), table("studies", ["users"], 10), table("sites", ["studies"], 5)];
  let module = [];
  for (let i = out.length; i < n; i++) {
    const refs = new Set();
    if (module.length === 0 || rnd() < 0.15) {
      module = [`t_${i}`];
      refs.add("studies");
      if (rnd() < 0.3) refs.add("sites");
    } else {
      refs.add(rnd() < 0.6 ? module[0] : pick(module));
      if (rnd() < 0.3) refs.add("studies");
      module.push(`t_${i}`);
    }
    if (rnd() < 0.6) refs.add("users");
    if (rnd() < 0.1) refs.add(pick(out).name);
    if (rnd() < 0.06) refs.clear();
    out.push(table(`t_${i}`, [...refs], 2 + Math.floor(rnd() * 9)));
  }
  // A cycle: a head table pointing back at one of its details.
  out[3].refs.push(out[4].name);
  return out;
}

/** The layout of the first version, to measure against. */
function oldLayout(boxes) {
  const byName = new Map(boxes.map((b) => [b.name, b]));
  const linked = new Set();
  for (const b of boxes) for (const r of b.refs) if (byName.has(r) && r !== b.name) linked.add(b.name).add(r);
  const depth = new Map();
  const visiting = new Set();
  const depthOf = (name) => {
    if (depth.has(name)) return depth.get(name);
    if (visiting.has(name)) return 0;
    visiting.add(name);
    const refs = byName.get(name).refs.filter((r) => byName.has(r) && r !== name);
    const d = refs.length ? 1 + Math.max(...refs.map(depthOf)) : 0;
    visiting.delete(name);
    depth.set(name, d);
    return d;
  };
  const columns = [];
  for (const b of boxes) if (linked.has(b.name)) (columns[depthOf(b.name)] ??= []).push(b);
  const at = {};
  let x = 20;
  let bottom = 20;
  for (const column of columns) {
    if (!column) continue;
    const centre = (b) => {
      const ys = b.refs.flatMap((r) => (at[r] ? [at[r].y] : []));
      return ys.length ? ys.reduce((s, y) => s + y, 0) / ys.length : Infinity;
    };
    const ordered = column.map((b) => ({ b, c: centre(b) })).sort((p, q) => p.c - q.c);
    let y = 20;
    for (const { b } of ordered) {
      at[b.name] = { x, y };
      y += b.height + 30;
    }
    bottom = Math.max(bottom, y);
    x += Math.max(...column.map((b) => b.width)) + 80;
  }
  const alone = boxes.filter((b) => !linked.has(b.name));
  const perRow = Math.max(1, Math.ceil(Math.sqrt(alone.length)));
  let rowY = linked.size ? bottom + 30 : 20;
  for (let i = 0; i < alone.length; i += perRow) {
    let rowX = 20;
    for (const b of alone.slice(i, i + perRow)) {
      at[b.name] = { x: rowX, y: rowY };
      rowX += b.width + 40;
    }
    rowY += Math.max(...alone.slice(i, i + perRow).map((b) => b.height)) + 30;
  }
  return at;
}

/** The foreign keys of a schema, once per pair of tables, self references included. */
const links = (boxes) => {
  const names = new Set(boxes.map((b) => b.name));
  return boxes.flatMap((b) => [...new Set(b.refs)].filter((r) => names.has(r)).map((r) => [b.name, r]));
};

function overlaps(boxes, at) {
  const out = [];
  for (const [i, a] of boxes.entries()) {
    for (const b of boxes.slice(i + 1)) {
      const p = at[a.name];
      const q = at[b.name];
      if (p.x < q.x + b.width && q.x < p.x + a.width && p.y < q.y + b.height && q.y < p.y + a.height) out.push(`${a.name}/${b.name}`);
    }
  }
  return out;
}

/** Whether two segments cross at a point inside both. */
function cross(p1, p2, q1, q2) {
  const d = (a, b, c) => (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
  const d1 = d(q1, q2, p1);
  const d2 = d(q1, q2, p2);
  const d3 = d(p1, p2, q1);
  const d4 = d(p1, p2, q2);
  return ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0));
}

/** The foreign keys as lines: from a middle row of the referencing table to the first row of
 *  the referenced one. */
const wiresOf = (boxes) => links(boxes).map(([from, to], i) => ({ from, fromRow: 34 + 20 * (1 + (i % 2)) + 10, to, toRow: 44 }));

/** The routed lines of a schema laid out at `at`. */
function routes(boxes, at) {
  const rects = Object.fromEntries(boxes.map((b) => [b.name, { ...at[b.name], width: b.width, height: b.height }]));
  const wires = wiresOf(boxes);
  return { wires, lines: routeWires(rects, wires) };
}

/** The lines the first version drew, as straight lines between the rows on the facing sides
 *  (it drew curves between those points); self references left out. */
function oldLines(boxes, at) {
  const byName = new Map(boxes.map((b) => [b.name, b]));
  return wiresOf(boxes)
    .filter((w) => w.from !== w.to)
    .map((w) => {
      const a = { ...at[w.from], width: byName.get(w.from).width };
      const b = { ...at[w.to], width: byName.get(w.to).width };
      const right = b.x + b.width / 2 > a.x + a.width / 2;
      return [
        { x: right ? a.x + a.width : a.x, y: a.y + w.fromRow },
        { x: right ? b.x : b.x + b.width, y: b.y + w.toRow },
      ];
    });
}

const segments = (points) => points.slice(1).map((p, i) => [points[i], p]);

/** The points where the lines cross, each counted once: lines running together (a bundle into
 *  the same row) cross another line once, as drawn. */
function routedCrossings(lines) {
  const points = new Set();
  const all = lines.map(segments);
  for (const [i, a] of all.entries()) {
    for (const b of all.slice(i + 1)) {
      for (const [p, q] of a) {
        for (const [r, s] of b) {
          if (!cross(p, q, r, s)) continue;
          // Where two segments cross: the parameter along p-q.
          const den = (q.x - p.x) * (s.y - r.y) - (q.y - p.y) * (s.x - r.x);
          const t = ((r.x - p.x) * (s.y - r.y) - (r.y - p.y) * (s.x - r.x)) / den;
          points.add(`${Math.round(p.x + t * (q.x - p.x))},${Math.round(p.y + t * (q.y - p.y))}`);
        }
      }
    }
  }
  return points.size;
}

/** The segments of a routed line running through a table that is not one of its ends. */
function throughTables(boxes, at, wires, lines) {
  const out = [];
  for (const [i, line] of lines.entries()) {
    for (const [p, q] of segments(line)) {
      for (const b of boxes) {
        if (b.name === wires[i].from || b.name === wires[i].to) continue;
        const r = at[b.name];
        const [x0, x1] = [Math.min(p.x, q.x), Math.max(p.x, q.x)];
        const [y0, y1] = [Math.min(p.y, q.y), Math.max(p.y, q.y)];
        if (x0 < r.x + b.width - 1 && x1 > r.x + 1 && y0 < r.y + b.height - 1 && y1 > r.y + 1) out.push(`${wires[i].from}->${wires[i].to} through ${b.name}`);
      }
    }
  }
  return out;
}

const measured = {};

for (const [name, make] of Object.entries(SCHEMAS)) {
  test(`the ${name} schema: no overlap, no line through a table`, () => {
    const boxes = make();
    const at = autoLayout(boxes);
    assert.deepEqual(Object.keys(at).sort(), boxes.map((b) => b.name).sort());
    assert.deepEqual(overlaps(boxes, at), []);
    const { wires, lines } = routes(boxes, at);
    assert.deepEqual(throughTables(boxes, at, wires, lines), []);
    for (const line of lines) for (const [p, q] of segments(line)) assert.ok(p.x === q.x || p.y === q.y, `not orthogonal: ${JSON.stringify(line)}`);
    measured[name] = {
      old: routedCrossings(oldLines(boxes, oldLayout(boxes))),
      now: routedCrossings(lines),
    };
  });
}

test("the lines cross far less than with the first layout", () => {
  if (process.env.ERD_LAYOUT_REPORT) console.log(measured);
  // With the first layout: star 70, realistic 658, large 19962 crossings.
  for (const name of ["realistic", "large", "star"]) {
    const m = measured[name];
    assert.ok(m.now * 2.5 < m.old, `${name}: ${JSON.stringify(m)}`);
  }
  assert.equal(measured.chain.now, 0);
  assert.equal(measured.selfReference.now, 0);
  assert.ok(measured.twoGroups.now <= 1 && measured.cycle.now <= 2, JSON.stringify(measured));
});

test("two unlinked groups are laid out apart, not interleaved", () => {
  const boxes = SCHEMAS.twoGroups();
  const at = autoLayout(boxes);
  const extent = (names) => {
    const bs = boxes.filter((b) => names.includes(b.name));
    return {
      x0: Math.min(...bs.map((b) => at[b.name].x)),
      x1: Math.max(...bs.map((b) => at[b.name].x + b.width)),
      y0: Math.min(...bs.map((b) => at[b.name].y)),
      y1: Math.max(...bs.map((b) => at[b.name].y + b.height)),
    };
  };
  const a = extent(["a", "a1", "a2", "a3"]);
  const b = extent(["b", "b1", "b2", "b3"]);
  assert.ok(a.x1 <= b.x0 || b.x1 <= a.x0 || a.y1 <= b.y0 || b.y1 <= a.y0, JSON.stringify({ a, b }));
});

test("a hub sits level with the middle of the tables referencing it", () => {
  const boxes = [table("users"), ...Array.from({ length: 7 }, (_, i) => table(`r_${i}`, ["users"]))];
  const at = autoLayout(boxes);
  const ys = boxes.slice(1).map((b) => at[b.name].y + b.height / 2);
  const middle = (Math.min(...ys) + Math.max(...ys)) / 2;
  assert.ok(Math.abs(at.users.y + boxes[0].height / 2 - middle) < 60, JSON.stringify(at));
});

test("a large schema is not laid out as one very wide row", () => {
  const boxes = SCHEMAS.large();
  const at = autoLayout(boxes);
  const width = Math.max(...boxes.map((b) => at[b.name].x + b.width));
  const height = Math.max(...boxes.map((b) => at[b.name].y + b.height));
  assert.ok(width / height < 4 && height / width < 4, `${width} x ${height}`);
});

test("a schema of 300 tables is laid out and routed in well under 200 ms", () => {
  const boxes = SCHEMAS.large();
  autoLayout(boxes);
  const started = performance.now();
  const at = autoLayout(boxes);
  routes(boxes, at);
  const took = performance.now() - started;
  assert.ok(took < 200, `${took} ms`);
});

test("lines leave and enter the boxes level, on the sides facing each other, round the boxes between", () => {
  const rects = { a: { x: 0, y: 0, width: 100, height: 80 }, b: { x: 300, y: 200, width: 100, height: 80 }, c: { x: 150, y: 0, width: 60, height: 400 } };
  const [line] = routeWires(rects, [{ from: "b", fromRow: 30, to: "a", toRow: 20 }]);
  assert.deepEqual(line[0], { x: 300, y: 230 });
  assert.deepEqual(line[line.length - 1], { x: 100, y: 20 });
  assert.equal(line[1].y, 230, "the first piece is level");
  assert.equal(line[line.length - 2].y, 20, "the last piece is level");
  // The wall `c` between them blocks everything from 0 to 400: the line goes round below it.
  assert.ok(line.some((p) => p.y > 400), JSON.stringify(line));
  const [loop] = routeWires(rects, [{ from: "a", fromRow: 50, to: "a", toRow: 20 }]);
  assert.ok(loop.every((p) => p.x >= 100) && loop[0].y === 50 && loop[loop.length - 1].y === 20, JSON.stringify(loop));
  assert.deepEqual(routeWires(rects, [{ from: "a", fromRow: 1, to: "gone", toRow: 1 }]), [[]]);
});

test("lines to different rows run beside each other in a channel, lines to the same row together", () => {
  const rects = {
    p: { x: 0, y: 0, width: 100, height: 80 },
    q: { x: 0, y: 100, width: 100, height: 80 },
    a: { x: 300, y: 200, width: 100, height: 80 },
    b: { x: 300, y: 400, width: 100, height: 80 },
  };
  const [one, two, three] = routeWires(rects, [
    { from: "a", fromRow: 30, to: "p", toRow: 30 },
    { from: "b", fromRow: 30, to: "p", toRow: 30 },
    { from: "b", fromRow: 50, to: "q", toRow: 30 },
  ]);
  assert.equal(one[2].x, two[2].x, "a bus out of p's row");
  assert.notEqual(three[2].x, two[2].x);
});

test("lines between tables placed anywhere by hand start and end on their rows, level and upright", () => {
  const rnd = random(3);
  const rects = Object.fromEntries(
    Array.from({ length: 40 }, (_, i) => [`r${i}`, { x: Math.round(rnd() * 2000), y: Math.round(rnd() * 1500), width: 140 + Math.round(rnd() * 150), height: 60 + Math.round(rnd() * 200) }]),
  );
  const wires = Array.from({ length: 80 }, (_, i) => ({ from: `r${i % 40}`, fromRow: 30, to: `r${Math.floor(rnd() * 40)}`, toRow: 36 }));
  for (const [i, line] of routeWires(rects, wires).entries()) {
    const { from, to } = wires[i];
    assert.equal(line[0].y, rects[from].y + 30);
    assert.ok(line[0].x === rects[from].x || line[0].x === rects[from].x + rects[from].width);
    assert.equal(line[line.length - 1].y, rects[to].y + 36);
    for (const [p, q] of segments(line)) assert.ok(p.x === q.x || p.y === q.y, JSON.stringify(line));
  }
});

test("pathThrough rounds the corners and keeps the ends", () => {
  const d = pathThrough([{ x: 0, y: 0 }, { x: 50, y: 0 }, { x: 50, y: 40 }, { x: 90, y: 40 }]);
  assert.match(d, /^M 0 0 L 44 0 Q 50 0 50 6 /);
  assert.match(d, / L 90 40$/);
  assert.equal(pathThrough([]), "");
});

test("nearestInOrder keeps the order and the gaps, as near as it can", () => {
  assert.deepEqual(nearestInOrder([0, 100], [1, 1], [50]), [0, 100]);
  assert.deepEqual(nearestInOrder([10, 10], [1, 1], [20]), [0, 20]);
  assert.deepEqual(nearestInOrder([10, 10], [3, 1], [20]), [5, 25]);
});

test("turns keep apart when a box the user placed covers the start of their channel", () => {
  // `c` sits over the right edge of `a`, so the channel the three lines turn in has no width.
  const rects = {
    a: { x: 0, y: 0, width: 100, height: 200 },
    c: { x: 105, y: 60, width: 100, height: 40 },
    b: { x: 400, y: 0, width: 100, height: 200 },
  };
  const wires = [0, 1, 2].map((i) => ({ from: "a", fromRow: 65 + i * 10, to: "b", toRow: 150 + i * 10 }));
  const lines = routeWires(rects, wires);
  const firstTurns = lines.map((l) => l[1].x);
  assert.equal(new Set(firstTurns).size, 3, JSON.stringify(firstTurns));
  for (const x of firstTurns) assert.ok(x >= 100 && x <= 112, String(x));
});

test("quickWire joins the facing sides of two boxes with one turn between them", () => {
  const rects = { a: { x: 0, y: 0, width: 100, height: 50 }, b: { x: 300, y: 100, width: 100, height: 50 } };
  const [from, , , to] = quickWire(rects, { from: "a", fromRow: 10, to: "b", toRow: 20 });
  assert.deepEqual(from, { x: 100, y: 10 });
  assert.deepEqual(to, { x: 300, y: 120 });
  const back = quickWire(rects, { from: "b", fromRow: 20, to: "a", toRow: 10 });
  assert.deepEqual(back[0], { x: 300, y: 120 });
  assert.deepEqual(back.at(-1), { x: 100, y: 10 });
});
