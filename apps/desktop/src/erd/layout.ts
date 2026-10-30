/** Where the tables of an ER diagram go when the user has not placed them. Each set of linked
 *  tables is laid out on its own as a layered drawing (Sugiyama): a table goes in a column right
 *  of the tables it references, a foreign key spanning several columns reserves a gap in each
 *  column it crosses (so its line has a channel between the boxes), the order of every column
 *  is chosen to cut the crossings, and the boxes of a column are placed level with the tables
 *  they are linked to. The sets are then packed in rows, and the tables without any foreign key
 *  go in a compact grid below them. */

export interface Box {
  name: string;
  width: number;
  height: number;
  /** The tables it references, itself excluded. */
  refs: string[];
}

export type Point = { x: number; y: number };

const GAP_X = 80;
const GAP_Y = 30;
/** The widest a channel between two columns gets, however many lines cross it. */
const MAX_CHANNEL = 160;
/** The height a line reserves in a column it crosses, and the space around it. */
const DUMMY_H = 14;
const DUMMY_GAP = 4;
/** The gap between the sets of linked tables. */
const BLOCK_GAP = 60;
/** The shape aimed at for the whole diagram and for each column, width over height. */
const ASPECT = 1.6;
/** A column is never cut shorter than this: about a screen. */
const MIN_COLUMN = 1000;
/** Rounds of crossing reduction, each a sweep right then left. */
const SWEEPS = 12;

/** A box or a point a long line passes through, in the layered drawing of one set. */
interface Node {
  /** The table; `null` for a point of a line crossing the column. */
  name: string | null;
  width: number;
  height: number;
  layer: number;
  /** Its neighbours in the column before (`up`) and after (`down`). */
  up: number[];
  down: number[];
}

/** Where every box goes, the diagram starting at `origin`. */
export function autoLayout(boxes: Box[], origin: Point = { x: 20, y: 20 }): Record<string, Point> {
  const byName = new Map(boxes.map((b) => [b.name, b]));
  const parents = new Map(boxes.map((b) => [b.name, [...new Set(b.refs)].filter((r) => r !== b.name && byName.has(r))]));
  const groups = components(boxes, parents);
  const linked = groups.filter((g) => g.length > 1);
  const alone = groups.filter((g) => g.length === 1).map((g) => byName.get(g[0]!)!);

  const blocks = linked.map((g) => layoutGroup(g.map((n) => byName.get(n)!), parents));
  const at: Record<string, Point> = {};
  const packed = pack(blocks, origin, at);
  const top = linked.length ? packed.bottom + GAP_Y : origin.y;
  grid(alone, { x: origin.x, y: top }, packed.width, at);
  return at;
}

/** The sets of tables linked by foreign keys, each in the order of `boxes`. */
function components(boxes: Box[], parents: Map<string, string[]>): string[][] {
  const root = new Map(boxes.map((b) => [b.name, b.name]));
  const find = (n: string): string => {
    let r = n;
    while (root.get(r) !== r) r = root.get(r)!;
    root.set(n, r);
    return r;
  };
  for (const b of boxes) for (const p of parents.get(b.name)!) root.set(find(b.name), find(p));
  const groups = new Map<string, string[]>();
  for (const b of boxes) {
    const r = find(b.name);
    if (!groups.has(r)) groups.set(r, []);
    groups.get(r)!.push(b.name);
  }
  return [...groups.values()];
}

/** A set of linked tables laid out from (0, 0): where each goes, and the size of the block. */
interface Block {
  at: Record<string, Point>;
  width: number;
  height: number;
}

function layoutGroup(boxes: Box[], parents: Map<string, string[]>): Block {
  const layer = assignLayers(boxes, acyclic(boxes, parents));
  const { nodes, layers } = buildLayers(boxes, layer);
  order(nodes, layers);
  const centre = placeInColumns(nodes, layers);
  // A column is as wide as its widest box; the channel after it grows with the bundles of lines
  // turning in it, one per node with lines going on.
  const xs: number[] = [];
  let x = 0;
  for (const column of layers) {
    xs.push(x);
    const bundles = column.filter((v) => nodes[v]!.down.length > 0).length;
    x += Math.max(0, ...column.map((v) => nodes[v]!.width)) + Math.min(MAX_CHANNEL, Math.max(GAP_X, 24 + 6 * bundles));
  }
  const at: Record<string, Point> = {};
  let width = 0;
  let height = 0;
  let top = Infinity;
  for (const [v, n] of nodes.entries()) {
    if (n.name === null) continue;
    top = Math.min(top, centre[v]! - n.height / 2);
  }
  for (const [v, n] of nodes.entries()) {
    if (n.name === null) continue;
    const p = { x: xs[n.layer]!, y: Math.round(centre[v]! - n.height / 2 - top) };
    at[n.name] = p;
    width = Math.max(width, p.x + n.width);
    height = Math.max(height, p.y + n.height);
  }
  return { at, width, height };
}

/** The foreign keys as edges from the referenced table to the referencing one, with the
 *  edges closing a cycle turned around, so that every table can go right of its parents. */
function acyclic(boxes: Box[], parents: Map<string, string[]>): [string, string][] {
  const children = new Map<string, string[]>(boxes.map((b) => [b.name, []]));
  for (const b of boxes) for (const p of parents.get(b.name)!) children.get(p)!.push(b.name);
  // 1 while a table is on the walk's path, 2 once all its children are walked.
  const state = new Map<string, 1 | 2>();
  const edges: [string, string][] = [];
  // From the tables referencing nothing first, so that a cycle is cut where it is entered.
  const starts = [...boxes.filter((b) => parents.get(b.name)!.length === 0), ...boxes];
  for (const { name } of starts) {
    if (state.has(name)) continue;
    const stack: [string, number][] = [[name, 0]];
    state.set(name, 1);
    while (stack.length) {
      const top = stack[stack.length - 1]!;
      const next = children.get(top[0])![top[1]++];
      if (next === undefined) {
        state.set(top[0], 2);
        stack.pop();
      } else if (state.get(next) === 1) {
        edges.push([next, top[0]]);
      } else {
        edges.push([top[0], next]);
        if (!state.has(next)) {
          state.set(next, 1);
          stack.push([next, 0]);
        }
      }
    }
  }
  return edges;
}

/** The column of every table: right of all its parents (longest path), then moved right next
 *  to its children when it has more children than parents, and the tables referenced by
 *  nobody moved on to the next column while a column is much taller than the drawing is meant
 *  to be. */
function assignLayers(boxes: Box[], edges: [string, string][]): Map<string, number> {
  const ups = new Map<string, string[]>(boxes.map((b) => [b.name, []]));
  const downs = new Map<string, string[]>(boxes.map((b) => [b.name, []]));
  for (const [p, c] of edges) {
    ups.get(c)!.push(p);
    downs.get(p)!.push(c);
  }
  const waiting = new Map(boxes.map((b) => [b.name, ups.get(b.name)!.length]));
  const topo = boxes.filter((b) => waiting.get(b.name) === 0).map((b) => b.name);
  for (let i = 0; i < topo.length; i++) {
    for (const c of downs.get(topo[i]!)!) {
      waiting.set(c, waiting.get(c)! - 1);
      if (waiting.get(c) === 0) topo.push(c);
    }
  }
  const layer = new Map<string, number>();
  for (const n of topo) layer.set(n, Math.max(-1, ...ups.get(n)!.map((p) => layer.get(p)!)) + 1);
  for (const n of [...topo].reverse()) {
    const down = downs.get(n)!;
    if (down.length > ups.get(n)!.length) layer.set(n, Math.min(...down.map((c) => layer.get(c)!)) - 1);
  }

  const byName = new Map(boxes.map((b) => [b.name, b]));
  const area = boxes.reduce((s, b) => s + (b.width + GAP_X) * (b.height + GAP_Y), 0);
  const cap = Math.max(MIN_COLUMN, ...boxes.map((b) => b.height), Math.sqrt(area / ASPECT));
  for (let l = 0; l <= Math.max(...layer.values()); l++) {
    const column = topo.filter((n) => layer.get(n) === l);
    let height = column.reduce((s, n) => s + byName.get(n)!.height + GAP_Y, -GAP_Y);
    for (const n of [...column].reverse()) {
      if (height <= cap) break;
      if (downs.get(n)!.length > 0 || column.length < 2) continue;
      layer.set(n, l + 1);
      height -= byName.get(n)!.height + GAP_Y;
    }
  }
  return layer;
}

/** The nodes of the layered drawing: the tables, and a point in every column a foreign key
 *  crosses; `layers` lists the nodes of each column. The long lines leaving the same table
 *  share their points, as the router draws them together: a hub referenced from every column
 *  takes one gap per column, not one per line. */
function buildLayers(boxes: Box[], layer: Map<string, number>): { nodes: Node[]; layers: number[][] } {
  const nodes: Node[] = boxes.map((b) => ({ name: b.name, width: b.width, height: b.height, layer: layer.get(b.name)!, up: [], down: [] }));
  const index = new Map(boxes.map((b, i) => [b.name, i]));
  const link = (a: number, b: number) => {
    if (nodes[a]!.down.includes(b)) return;
    nodes[a]!.down.push(b);
    nodes[b]!.up.push(a);
  };
  const shared = new Map<string, number>();
  for (const b of boxes) {
    for (const r of b.refs) {
      const p = index.get(r);
      const c = index.get(b.name)!;
      if (p === undefined || p === c) continue;
      const [from, to] = nodes[p]!.layer < nodes[c]!.layer ? [p, c] : [c, p];
      let last = from;
      for (let l = nodes[from]!.layer + 1; l < nodes[to]!.layer; l++) {
        const key = `${from}/${l}`;
        if (!shared.has(key)) {
          nodes.push({ name: null, width: 0, height: DUMMY_H, layer: l, up: [], down: [] });
          shared.set(key, nodes.length - 1);
        }
        link(last, shared.get(key)!);
        last = shared.get(key)!;
      }
      link(last, to);
    }
  }
  // The first order: the tables as they came, a line's points after the table it leaves. The
  // columns left empty (by the moves of `assignLayers`) are dropped.
  const used = [...new Set(nodes.map((n) => n.layer))].sort((a, b) => a - b);
  const layers: number[][] = used.map(() => []);
  for (const [v, n] of nodes.entries()) {
    n.layer = used.indexOf(n.layer);
    layers[n.layer]!.push(v);
  }
  return { nodes, layers };
}

/** Orders every column to cut the crossings between neighbouring columns: sweeps right and
 *  left putting each node at the mean position of its neighbours in the column just swept,
 *  keeping the best order seen. */
function order(nodes: Node[], layers: number[][]): void {
  const pos = new Float64Array(nodes.length);
  const renumber = (column: number[]) => column.forEach((v, i) => (pos[v] = i));
  layers.forEach(renumber);
  let best = layers.map((l) => [...l]);
  let fewest = crossings(nodes, layers, pos);
  const sortBy = (column: number[], side: "up" | "down") => {
    const key = new Map(column.map((v) => {
      const ns = nodes[v]![side];
      return [v, ns.length ? ns.reduce((s, n) => s + pos[n]!, 0) / ns.length : pos[v]!];
    }));
    column.sort((a, b) => key.get(a)! - key.get(b)! || pos[a]! - pos[b]!);
    renumber(column);
  };
  for (let round = 0; round < SWEEPS && fewest > 0; round++) {
    for (let l = 1; l < layers.length; l++) sortBy(layers[l]!, "up");
    for (let l = layers.length - 2; l >= 0; l--) sortBy(layers[l]!, "down");
    const now = crossings(nodes, layers, pos);
    if (now < fewest) {
      fewest = now;
      best = layers.map((l) => [...l]);
    }
  }
  best.forEach((l, i) => {
    layers[i] = l;
    renumber(l);
  });
}

/** The crossings between all neighbouring columns, counted as the inversions of the edges
 *  sorted by their ends (with a Fenwick tree). */
function crossings(nodes: Node[], layers: number[][], pos: Float64Array): number {
  let total = 0;
  for (let l = 0; l + 1 < layers.length; l++) {
    const ends: number[] = [];
    for (const v of layers[l]!) for (const d of [...nodes[v]!.down].sort((a, b) => pos[a]! - pos[b]!)) ends.push(pos[d]!);
    const size = layers[l + 1]!.length;
    const tree = new Int32Array(size + 1);
    for (let i = 0; i < ends.length; i++) {
      // The edges before this one ending further down cross it.
      let below = 0;
      for (let k = size; k > 0; k -= k & -k) below += tree[k]!;
      for (let k = ends[i]! + 1; k > 0; k -= k & -k) below -= tree[k]!;
      total += below;
      for (let k = ends[i]! + 1; k <= size; k += k & -k) tree[k]!++;
    }
  }
  return total;
}

/** The vertical centre of every node: the order of each column is kept with room between the
 *  boxes, and each node goes as near as it can to the mean of its neighbours (a line's points
 *  weigh more, to keep long lines straight). */
function placeInColumns(nodes: Node[], layers: number[][]): Float64Array {
  const centre = new Float64Array(nodes.length);
  for (const column of layers) {
    let y = 0;
    for (const v of column) {
      centre[v] = y + nodes[v]!.height / 2;
      y += nodes[v]!.height + GAP_Y;
    }
  }
  const place = (column: number[], sides: ("up" | "down")[]) => {
    const wanted = column.map((v) => {
      const ns = sides.flatMap((s) => nodes[v]![s]);
      return ns.length ? ns.reduce((s, n) => s + centre[n]!, 0) / ns.length : centre[v]!;
    });
    const weight = column.map((v) => (nodes[v]!.name === null ? 4 : 1) * Math.max(1, nodes[v]!.up.length + nodes[v]!.down.length));
    const gaps = column.slice(1).map((v, i) => {
      const prev = nodes[column[i]!]!;
      const gap = prev.name === null && nodes[v]!.name === null ? DUMMY_GAP : prev.name === null || nodes[v]!.name === null ? GAP_Y / 2 : GAP_Y;
      return prev.height / 2 + gap + nodes[v]!.height / 2;
    });
    const placed = nearestInOrder(wanted, weight, gaps);
    column.forEach((v, i) => (centre[v] = placed[i]!));
  };
  for (let round = 0; round < 6; round++) {
    const sides: ("up" | "down")[][] = round < 4 ? [["up"], ["down"]] : [["up", "down"], ["up", "down"]];
    for (let l = 1; l < layers.length; l++) place(layers[l]!, sides[0]!);
    for (let l = layers.length - 2; l >= 0; l--) place(layers[l]!, sides[1]!);
  }
  return centre;
}

/** The positions nearest to `wanted` (least weighted squares) that keep their order with at
 *  least `gaps[i]` between the i-th and the next: with the gaps taken off, an isotonic
 *  regression, solved by pooling adjacent violators. */
export function nearestInOrder(wanted: number[], weight: number[], gaps: number[]): number[] {
  const offset = [0];
  for (const g of gaps) offset.push(offset[offset.length - 1]! + g);
  const pools: { value: number; weight: number; count: number }[] = [];
  for (let i = 0; i < wanted.length; i++) {
    let pool = { value: wanted[i]! - offset[i]!, weight: weight[i]!, count: 1 };
    while (pools.length && pools[pools.length - 1]!.value >= pool.value) {
      const prev = pools.pop()!;
      const w = prev.weight + pool.weight;
      pool = { value: (prev.value * prev.weight + pool.value * pool.weight) / w, weight: w, count: prev.count + pool.count };
    }
    pools.push(pool);
  }
  const out: number[] = [];
  for (const p of pools) for (let k = 0; k < p.count; k++) out.push(p.value + offset[out.length]!);
  return out;
}

/** Packs the blocks in rows (tallest first) about as wide as the diagram should be; returns the
 *  width and the bottom of what it packed. */
function pack(blocks: Block[], origin: Point, at: Record<string, Point>): { width: number; bottom: number } {
  if (blocks.length === 0) return { width: 0, bottom: origin.y };
  const area = blocks.reduce((s, b) => s + (b.width + BLOCK_GAP) * (b.height + BLOCK_GAP), 0);
  const rowWidth = Math.max(...blocks.map((b) => b.width), Math.sqrt(area * ASPECT));
  const sorted = [...blocks].sort((a, b) => b.height - a.height);
  let x = origin.x;
  let y = origin.y;
  let rowHeight = 0;
  let width = 0;
  for (const b of sorted) {
    if (x > origin.x && x - origin.x + b.width > rowWidth) {
      x = origin.x;
      y += rowHeight + BLOCK_GAP;
      rowHeight = 0;
    }
    for (const [name, p] of Object.entries(b.at)) at[name] = { x: x + p.x, y: y + p.y };
    width = Math.max(width, x - origin.x + b.width);
    x += b.width + BLOCK_GAP;
    rowHeight = Math.max(rowHeight, b.height);
  }
  return { width, bottom: y + rowHeight };
}

/** The tables without any foreign key, in rows about as wide as the linked tables above them
 *  (or a square grid). */
function grid(alone: Box[], origin: Point, width: number, at: Record<string, Point>): void {
  if (alone.length === 0) return;
  const mean = alone.reduce((s, b) => s + b.width + GAP_X / 2, 0) / alone.length;
  const perRow = Math.max(1, Math.ceil(Math.sqrt(alone.length)), Math.floor(width / mean));
  let rowY = origin.y;
  for (let i = 0; i < alone.length; i += perRow) {
    const row = alone.slice(i, i + perRow);
    let rowX = origin.x;
    for (const b of row) {
      at[b.name] = { x: rowX, y: rowY };
      rowX += b.width + GAP_X / 2;
    }
    rowY += Math.max(...row.map((b) => b.height)) + GAP_Y;
  }
}

/** Where every box goes when the user has placed some (`fixed`): those stay where they are,
 *  the others are laid out with `autoLayout` below the lowest placed one, so a new table
 *  never lands on a placed table. With none placed, the whole diagram is laid out. */
export function placeTables(boxes: Box[], fixed: Record<string, Point>): Record<string, Point> {
  const placed = boxes.filter((b) => fixed[b.name]);
  if (placed.length === 0) return autoLayout(boxes);
  const bottom = Math.max(...placed.map((b) => fixed[b.name]!.y + b.height));
  const rest = boxes.filter((b) => !fixed[b.name]);
  const at = rest.length ? autoLayout(rest, { x: 20, y: bottom + GAP_Y }) : {};
  return { ...at, ...Object.fromEntries(placed.map((b) => [b.name, fixed[b.name]!])) };
}
