/**
 * Reads an SQL editor tab for `context.ts`: its text and selection from the editor, its rows
 * from the job the grid shows.
 */
import { api, type SavedConnection } from "../api";
import type { Result } from "../components/ResultPanel";
import type { SqlEditorHandle } from "../editor/SqlEditor";
import { effectiveAgentAccess } from "./access";
import { resultRows, selectionText, RESULT_ROWS, type ContextResult, type ContextTable, type TabSnapshot } from "./context";

/** The first rows of the result on screen; `null` when it has none to show (or they were
 *  released meanwhile). */
async function shownRows(result: Result | undefined): Promise<ContextResult | null> {
  if (result?.status.kind !== "rows" || !result.columns || result.jobId === null) return null;
  const { finished, complete } = result.status;
  try {
    const page = await api.fetchRows(result.jobId, 0, RESULT_ROWS);
    return resultRows(
      result.columns.map((c) => c.name),
      page.columns,
      result.rows,
      !finished || !complete,
    );
  } catch {
    return null;
  }
}

export interface TabParts {
  connection: SavedConnection;
  title: string | null;
  editor: SqlEditorHandle | null;
  /** The result the grid shows. */
  result: Result | undefined;
  /** The table of a data viewer tab. */
  table?: ContextTable;
}

export async function tabSnapshot({ connection, title, editor, result, table }: TabParts): Promise<TabSnapshot> {
  const closedToAgents = effectiveAgentAccess(connection) === "off";
  const text = editor?.getText() ?? "";
  const { from, to } = editor?.getSelection() ?? { from: 0, to: 0 };
  return {
    connection: connection.name,
    connectionId: connection.id,
    title,
    editor: text,
    selection: selectionText(text, from, to),
    // Nothing is read for a tab that cannot be shared.
    result: closedToAgents ? null : await shownRows(result),
    closedToAgents,
    ...(table ? { table } : {}),
  };
}
