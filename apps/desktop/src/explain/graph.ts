import type { PlanNode } from "../api";

/** A step placed in the graph: its box's top left corner, and its path in the tree. */
export interface Placed {
  node: PlanNode;
  path: string;
  x: number;
  y: number;
}

export interface Layout {
  boxes: Placed[];
  /** From a step to each of its children, by path. */
  edges: [string, string][];
  width: number;
  height: number;
}

export const BOX = { width: 210, height: 74, gapX: 22, gapY: 42 };

/** The plan as a top-down tree: each leaf in its own column, each step centred over its
 *  children, a row per depth. */
export function layoutPlan(root: PlanNode): Layout {
  const boxes: Placed[] = [];
  const edges: [string, string][] = [];
  let column = 0;
  let depthMax = 0;
  const place = (node: PlanNode, path: string, depth: number): number => {
    depthMax = Math.max(depthMax, depth);
    let x: number;
    if (node.children.length === 0) {
      x = column * (BOX.width + BOX.gapX);
      column++;
    } else {
      const xs = node.children.map((c, i) => {
        const child = `${path}.${i}`;
        edges.push([path, child]);
        return place(c, child, depth + 1);
      });
      x = (xs[0]! + xs[xs.length - 1]!) / 2;
    }
    boxes.push({ node, path, x, y: depth * (BOX.height + BOX.gapY) });
    return x;
  };
  place(root, "0", 0);
  return {
    boxes,
    edges,
    width: column * (BOX.width + BOX.gapX) - BOX.gapX,
    height: (depthMax + 1) * (BOX.height + BOX.gapY) - BOX.gapY,
  };
}
