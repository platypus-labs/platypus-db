/**
 * An agent's request to draw an ER diagram as SVG (`agent-diagram-request` events). The model
 * comes from the agent's server, which read the catalog; nothing is asked of the user, no
 * connection is opened and nothing is shown. This module and the drawing code behind it load at
 * the first request (see `bridge.ts`), so that the app does not carry them at each start.
 */
import { api } from "../api";
import { readColors } from "../analysis/Chart";
import { erdSvg } from "../erd/diagramSvg";
import type { Shape } from "../erd/geometry";

/** `platypus_mcp::DiagramRequest`. */
export interface DiagramRequest {
  id: string;
  schema: string;
  tables: (Omit<Shape, "foreign_keys"> & { foreign_keys: { name: string; columns: string[]; table: string; ref_columns: string[] }[] })[];
}

/** `platypus_mcp::DiagramAnswer`. */
export type DiagramAnswer = { answer: "svg"; svg: string } | { answer: "failed"; message: string };

/** The SVG for `request`, or why it could not be drawn. */
function drawDiagram(request: DiagramRequest): DiagramAnswer {
  try {
    // The model names the table a key goes to; the drawing wants it as the app's own tables have it.
    const tables: Shape[] = request.tables.map((t) => ({
      ...t,
      foreign_keys: t.foreign_keys.map((f) => ({ name: f.name, columns: f.columns, ref_table: f.table, ref_columns: f.ref_columns })),
    }));
    return { answer: "svg", svg: erdSvg(tables, request.schema, readColors()) };
  } catch (e) {
    return { answer: "failed", message: e instanceof Error ? e.message : String(e) };
  }
}

/** Draws `request` and answers the agent's server. */
export async function answerDiagram(request: DiagramRequest) {
  await api.answerAgentDiagram(request.id, drawDiagram(request));
}
