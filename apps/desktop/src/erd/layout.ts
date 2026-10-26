/** Where the tables of an ER diagram go when the user has not placed them: in columns by
 *  foreign key depth (a table right of the tables it references), and the tables without any
 *  foreign key in a grid below. */

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

export function autoLayout(boxes: Box[], origin: Point = { x: 20, y: 20 }): Record<string, Point> {
  const byName = new Map(boxes.map((b) => [b.name, b]));
  const linked = new Set<string>();
  for (const b of boxes) {
    for (const r of b.refs) {
      if (!byName.has(r)) continue;
      linked.add(b.name);
      linked.add(r);
    }
  }
  // Depth: one more than the deepest table referenced; a cycle stops at the table it reaches again.
  const depth = new Map<string, number>();
  const visiting = new Set<string>();
  const depthOf = (name: string): number => {
    const known = depth.get(name);
    if (known !== undefined) return known;
    if (visiting.has(name)) return 0;
    visiting.add(name);
    const refs = byName.get(name)!.refs.filter((r) => byName.has(r));
    const d = refs.length ? 1 + Math.max(...refs.map(depthOf)) : 0;
    visiting.delete(name);
    depth.set(name, d);
    return d;
  };
  const columns: Box[][] = [];
  for (const b of boxes) if (linked.has(b.name)) (columns[depthOf(b.name)] ??= []).push(b);

  const at: Record<string, Point> = {};
  let x = origin.x;
  let bottom = origin.y;
  for (const column of columns) {
    if (!column) continue;
    // Next to the tables they reference, as far as the order of a column allows.
    const centre = (b: Box) => {
      const ys = b.refs.flatMap((r) => (at[r] ? [at[r].y] : []));
      return ys.length ? ys.reduce((s, y) => s + y, 0) / ys.length : Infinity;
    };
    const ordered = column.map((b) => ({ b, c: centre(b) })).sort((p, q) => p.c - q.c);
    let y = origin.y;
    for (const { b } of ordered) {
      at[b.name] = { x, y };
      y += b.height + GAP_Y;
    }
    bottom = Math.max(bottom, y);
    x += Math.max(...column.map((b) => b.width)) + GAP_X;
  }

  const alone = boxes.filter((b) => !linked.has(b.name));
  const perRow = Math.max(1, Math.ceil(Math.sqrt(alone.length)));
  let rowY = linked.size ? bottom + GAP_Y : origin.y;
  for (let i = 0; i < alone.length; i += perRow) {
    const row = alone.slice(i, i + perRow);
    let rowX = origin.x;
    for (const b of row) {
      at[b.name] = { x: rowX, y: rowY };
      rowX += b.width + GAP_X / 2;
    }
    rowY += Math.max(...row.map((b) => b.height)) + GAP_Y;
  }
  return at;
}
