/**
 * Terminal mode of the SQL editor: results as text, one block per statement, kept across
 * runs like a console, instead of the grid.
 */
import { useEffect, useRef } from "react";
import { api, type ColumnInfo } from "../api";
import { type ResultStatus, describe, fmt, formatMs } from "../components/ResultPanel";

/** Rows a statement shows in terminal mode; the rest is not fetched. */
export const TERMINAL_ROWS = 500;

export interface TerminalBlock {
  id: number;
  sql: string;
  text: string;
  failed: boolean;
}

const CELL = 80;

function cell(v: string | null) {
  const text = v === null ? "NULL" : v.replace(/\r?\n/g, "↵");
  return text.length > CELL ? `${text.slice(0, CELL - 1)}…` : text;
}

/** Rows as a psql-style table, with a line counting them. */
export function textTable(columns: string[], rows: (string | null)[][], more: boolean): string {
  const cells = rows.map((r) => r.map(cell));
  const widths = columns.map((c, i) => Math.max(c.length, ...cells.map((r) => r[i]!.length)));
  const line = (values: string[]) => ` ${values.map((v, i) => v.padEnd(widths[i]!)).join(" | ")}`.trimEnd();
  const rule = widths.map((w) => "-".repeat(w + 2)).join("+");
  const count = `(${fmt.format(rows.length)} ${rows.length === 1 ? "row" : "rows"}${more ? ", more not shown" : ""})`;
  return [line(columns), rule, ...cells.map(line), count].join("\n");
}

/** The text of a finished statement: its rows, rows affected, or its error. */
export async function blockText(
  status: ResultStatus,
  jobId: number | null,
  columns: ColumnInfo[] | null,
  rows: number,
): Promise<string> {
  switch (status.kind) {
    case "rows": {
      if (jobId === null || !columns) return "";
      const values = await api.rowValues(jobId, 0, Math.min(rows, TERMINAL_ROWS));
      const more = !status.complete || !status.finished;
      return `${textTable(
        columns.map((c) => c.name),
        values,
        more,
      )}\nTime: ${formatMs(status.elapsedMs)}`;
    }
    case "affected":
      return `${fmt.format(status.rows)} row(s) affected\nTime: ${formatMs(status.elapsedMs)}`;
    case "error": {
      const e = status.error;
      return [`ERROR: ${e.code ? `${e.code}: ` : ""}${e.message}`, e.detail && `DETAIL: ${e.detail}`, e.hint && `HINT: ${e.hint}`]
        .filter(Boolean)
        .join("\n");
    }
    case "message":
      return status.text;
    default:
      return describe({ id: 0, sql: "", offset: 0, jobId, columns, rows, status, limit: null });
  }
}

export function TerminalView({ blocks, onClear }: { blocks: TerminalBlock[]; onClear: () => void }) {
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [blocks.length]);
  return (
    <div className="terminal-view">
      <div className="terminal-bar">
        <span className="muted small">Terminal output</span>
        <span className="spacer" />
        <button className="link" onClick={onClear} disabled={blocks.length === 0}>
          Clear
        </button>
      </div>
      <pre className="terminal-output" tabIndex={0}>
        {blocks.map((b) => (
          <div key={b.id} className={`terminal-block${b.failed ? " failed" : ""}`}>
            <span className="terminal-sql">{b.sql.replace(/^/gm, "> ")}</span>
            {"\n"}
            {b.text}
            {"\n\n"}
          </div>
        ))}
        <div ref={end} />
      </pre>
    </div>
  );
}
