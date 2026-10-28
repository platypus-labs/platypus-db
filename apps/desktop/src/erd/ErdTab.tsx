import { type PointerEvent, useEffect, useMemo, useRef, useState } from "react";
import { api, type SchemaTable, toCommandError } from "../api";
import { type ChartColors, useChartColors } from "../analysis/Chart";
import { exportChart } from "../analysis/chartExport";
import type { TabBase, TabProps } from "../tabs/registry";
import { Icon } from "../ui/Icon";
import { workbench } from "../workbench";
import { t, plural } from "../i18n";
import { type Box, type Point, autoLayout } from "./layout";
import { TablesDialog } from "./TablesDialog";

export interface ErdNote {
  id: number;
  x: number;
  y: number;
  text: string;
}

/** An ER diagram of a schema: every table, or those chosen, placed by the user or laid out
 *  from the foreign keys, with notes. All of it is kept with the tab. */
export interface ErdTabModel extends TabBase {
  kind: "erd";
  schema: string;
  currentSchema: boolean;
  /** The tables shown; `null` for all of them. */
  tables: string[] | null;
  /** Where the user put tables; the others are laid out automatically. */
  positions: Record<string, Point>;
  notes: ErdNote[];
  zoom: number;
}

declare module "../tabs/registry" {
  interface TabKinds {
    erd: ErdTabModel;
  }
}

/** Above this many tables, a diagram of a whole schema asks which ones to show first. */
export const ERD_LIMIT = 100;

const HEAD = 26;
const ROW = 20;
const CHAR = 7;
const NOTE_WIDTH = 200;
const NOTE_CHARS = 28;
const PAD = 20;

const boxWidth = (t: SchemaTable) => {
  const longest = Math.max(t.name.length + 2, ...t.details.columns.map((c) => c.name.length + c.data_type.length + 3));
  return Math.min(340, Math.max(140, 16 + CHAR * longest));
};

/** Whether `fk` references a table of `schema`, the diagram's: a table of another schema is
 *  not drawn, even with the name of one here. */
const local = (fk: { ref_schema: string }, schema: string) => !fk.ref_schema || fk.ref_schema === schema;

const box = (t: SchemaTable, schema: string): Box => ({
  name: t.name,
  width: boxWidth(t),
  height: HEAD + ROW * t.details.columns.length + 6,
  refs: t.details.foreign_keys
    .filter((f) => local(f, schema))
    .map((f) => f.ref_table)
    .filter((r) => r !== t.name),
});

/** Two presses this close on the same thing are a double click. The pointer is captured on
 *  each press, so the browser's own dblclick goes to the canvas, not to the table. */
const DOUBLE_CLICK_MS = 500;

/** A note's text cut in lines of about `NOTE_CHARS` characters. */
function noteLines(text: string): string[] {
  return text.split("\n").flatMap((line) => {
    const out: string[] = [];
    let current = "";
    for (const word of line.split(" ")) {
      if (current && current.length + word.length >= NOTE_CHARS) {
        out.push(current);
        current = word;
      } else current = current ? `${current} ${word}` : word;
    }
    return [...out, current];
  });
}

const noteHeight = (n: ErdNote) => 16 + noteLines(n.text).length * 16;

type Dragging = { what: "table"; name: string; dx: number; dy: number } | { what: "note"; id: number; dx: number; dy: number };

export function ErdTab({ tab, connection, active, sideSession, onChange, onError }: TabProps<ErdTabModel>) {
  const [tables, setTables] = useState<SchemaTable[] | null>(null);
  const [all, setAll] = useState<string[] | null>(null);
  const [choosing, setChoosing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const [moved, setMoved] = useState<Record<string, Point>>({});
  const [movedNotes, setMovedNotes] = useState<Record<number, Point>>({});
  const [editing, setEditing] = useState<number | null>(null);
  const drag = useRef<Dragging | null>(null);
  const lastPress = useRef<{ what: string; at: number } | null>(null);
  const svg = useRef<SVGSVGElement>(null);
  const colors = useChartColors();
  const id = (what: string) => `erd-${tab.key}-${what}`;

  useEffect(() => {
    let live = true;
    setError(null);
    void (async () => {
      try {
        const sid = await sideSession();
        if (sid === null || !live) return;
        const objects = await api.listObjects(sid, tab.schema);
        // The tables without their partitions, as `platypus_schema::tables_of` lists them.
        const tableNames = new Set(objects.flatMap((o) => (o.kind === "table" ? [o.name] : [])));
        const names = objects.filter((o) => o.kind === "table" && !(o.parent !== null && tableNames.has(o.parent))).map((o) => o.name);
        if (!live) return;
        setAll(names);
        if (tab.tables === null && names.length > ERD_LIMIT) {
          setTables([]);
          setChoosing(true);
          return;
        }
        const read = await api.schemaTables(sid, tab.schema, tab.tables);
        if (live) setTables(read);
      } catch (e) {
        if (live) setError(toCommandError(e).message);
      }
    })();
    return () => {
      live = false;
    };
  }, [tab.schema, tab.tables?.join("\n"), version]);

  const boxes = useMemo(() => (tables ?? []).map((t) => box(t, tab.schema)), [tables, tab.schema]);
  const placed = useMemo(() => ({ ...autoLayout(boxes), ...tab.positions, ...moved }), [boxes, tab.positions, moved]);
  const pos = (name: string): Point => placed[name] ?? { x: PAD, y: PAD };
  const notes = useMemo(() => tab.notes.map((n) => ({ ...n, ...movedNotes[n.id] })), [tab.notes, movedNotes]);
  const unreadable = (tables ?? []).filter((t) => t.error !== null);
  const size = useMemo(() => {
    let w = 400;
    let h = 300;
    for (const b of boxes) {
      const p = pos(b.name);
      w = Math.max(w, p.x + b.width + PAD);
      h = Math.max(h, p.y + b.height + PAD);
    }
    for (const n of notes) {
      w = Math.max(w, n.x + NOTE_WIDTH + PAD);
      h = Math.max(h, n.y + noteHeight(n) + PAD);
    }
    return { w, h };
  }, [boxes, placed, notes]);

  const at = (e: PointerEvent): Point => {
    const rect = svg.current!.getBoundingClientRect();
    return { x: (e.clientX - rect.left) / tab.zoom, y: (e.clientY - rect.top) / tab.zoom };
  };
  /** Whether this press on `what` makes a double click with the one before. */
  const double = (e: PointerEvent, what: string) => {
    const last = lastPress.current;
    const yes = last !== null && last.what === what && e.timeStamp - last.at < DOUBLE_CLICK_MS;
    lastPress.current = yes ? null : { what, at: e.timeStamp };
    return yes;
  };
  const startTable = (e: PointerEvent, name: string) => {
    if (e.button !== 0) return;
    if (double(e, `table:${name}`)) {
      drag.current = null;
      openTable(name);
      return;
    }
    const p = at(e);
    drag.current = { what: "table", name, dx: p.x - pos(name).x, dy: p.y - pos(name).y };
    svg.current!.setPointerCapture(e.pointerId);
  };
  const startNote = (e: PointerEvent, n: ErdNote) => {
    if (e.button !== 0) return;
    if (double(e, `note:${n.id}`)) {
      drag.current = null;
      setEditing(n.id);
      return;
    }
    const p = at(e);
    drag.current = { what: "note", id: n.id, dx: p.x - n.x, dy: p.y - n.y };
    svg.current!.setPointerCapture(e.pointerId);
  };
  const move = (e: PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const p = at(e);
    const to = { x: Math.max(0, Math.round(p.x - d.dx)), y: Math.max(0, Math.round(p.y - d.dy)) };
    if (d.what === "table") setMoved((m) => ({ ...m, [d.name]: to }));
    else setMovedNotes((m) => ({ ...m, [d.id]: to }));
  };
  const drop = () => {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    if (d.what === "table" && moved[d.name]) onChange({ positions: { ...tab.positions, ...moved } });
    if (d.what === "note" && movedNotes[d.id]) onChange({ notes: tab.notes.map((n) => ({ ...n, ...movedNotes[n.id] })) });
    setMoved({});
    setMovedNotes({});
  };

  const addNote = () => {
    const next = Math.max(0, ...tab.notes.map((n) => n.id)) + 1;
    onChange({ notes: [...tab.notes, { id: next, x: PAD, y: size.h, text: t("Note") }] });
    setEditing(next);
  };
  const setNote = (noteId: number, text: string | null) =>
    onChange({ notes: text === null ? tab.notes.filter((n) => n.id !== noteId) : tab.notes.map((n) => (n.id === noteId ? { ...n, text } : n)) });
  const openTable = (name: string) =>
    workbench().openTab("object", connection.id, {
      database: tab.database,
      schema: tab.schema,
      currentSchema: tab.currentSchema,
      name,
      objectKind: "table",
      title: name,
    });
  const save = (format: "png" | "svg") => {
    if (!svg.current) return;
    void exportChart(svg.current, format, `${tab.schema}-diagram`).catch((e: unknown) =>
      onError(e instanceof Error ? e.message : toCommandError(e).message),
    );
  };
  const zoom = (by: number) => onChange({ zoom: Math.min(2, Math.max(0.25, Math.round((tab.zoom + by) * 100) / 100)) });
  const note = editing === null ? undefined : tab.notes.find((n) => n.id === editing);

  return (
    <div className="object-tab erd" style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <span className="object-icon" data-kind="schema">
          <Icon name="schema" size={16} />
        </span>
        <span>
          <strong>{t("ER diagram of {schema}", { schema: tab.schema })}</strong>{" "}
          <span className="muted small">
            {tab.tables === null ? t("all tables") : plural(tab.tables.length, "{n} table", "{n} tables")}
          </span>
        </span>
        <span className="spacer" />
        <button type="button" disabled={!all} onClick={() => setChoosing(true)}>
          {t("Tables…")}
        </button>
        <button type="button" onClick={addNote}>
          {t("Add note")}
        </button>
        <button type="button" onClick={() => onChange({ positions: {} })} title={t("Place every table again from the foreign keys")}>
          {t("Auto layout")}
        </button>
        <button className="square" type="button" onClick={() => zoom(-0.25)} aria-label={t("Zoom out")} title={t("Zoom out")}>
          −
        </button>
        <span className="small muted">{Math.round(tab.zoom * 100)}%</span>
        <button className="square" type="button" onClick={() => zoom(0.25)} aria-label={t("Zoom in")} title={t("Zoom in")}>
          +
        </button>
        <button type="button" onClick={() => save("svg")}>
          SVG…
        </button>
        <button type="button" onClick={() => save("png")}>
          PNG…
        </button>
        <button className="square" type="button" onClick={() => setVersion((v) => v + 1)} aria-label={t("Refresh")} title={t("Refresh")}>
          <Icon name="refresh" />
        </button>
      </header>
      {error && <div className="notice error small">{error}</div>}
      {unreadable.length > 0 && (
        <div className="notice error small">
          {t("Could not read {tables}: {error}", { tables: unreadable.map((u) => u.name).join(", "), error: unreadable[0]!.error! })}
        </div>
      )}
      <div className="erd-canvas">
        {tables === null && !error && <p className="muted small erd-loading">{t("Reading the tables…")}</p>}
        <svg
          ref={svg}
          width={size.w * tab.zoom}
          height={size.h * tab.zoom}
          viewBox={`0 0 ${size.w} ${size.h}`}
          fontFamily={colors.font}
          fontSize={12}
          onPointerMove={move}
          onPointerUp={drop}
          onPointerCancel={drop}
        >
          <defs>
            <marker id={id("one")} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="10" markerHeight="10" orient="auto-start-reverse">
              <path d="M 6 0 L 6 10" stroke={colors.muted} fill="none" />
            </marker>
            <marker id={id("many")} viewBox="0 0 10 10" refX="1" refY="5" markerWidth="10" markerHeight="10" orient="auto-start-reverse">
              <path d="M 10 5 L 0 0 M 10 5 L 0 10 M 10 5 L 0 5" stroke={colors.muted} fill="none" />
            </marker>
          </defs>
          <rect width={size.w} height={size.h} fill={colors.background} />
          {(tables ?? []).map((table, i) =>
            table.details.foreign_keys.map((fk) => {
              if (!local(fk, tab.schema)) return null;
              const j = tables!.findIndex((o) => o.name === fk.ref_table);
              if (j < 0) return null;
              const target = tables![j]!;
              const a = { ...pos(table.name), width: boxes[i]!.width };
              const b = { ...pos(target.name), width: boxes[j]!.width };
              return (
                <path
                  key={`${table.name}/${fk.name}`}
                  d={edgePath(table, a, target, b, fk.columns[0], fk.ref_columns[0])}
                  stroke={colors.muted}
                  fill="none"
                  markerStart={`url(#${id("many")})`}
                  markerEnd={`url(#${id("one")})`}
                >
                  <title>{`${fk.name}: ${table.name} (${fk.columns.join(", ")}) → ${fk.ref_table} (${fk.ref_columns.join(", ")})`}</title>
                </path>
              );
            }),
          )}
          {(tables ?? []).map((table, i) => (
            <TableBox key={table.name} table={table} box={boxes[i]!} at={pos(table.name)} colors={colors} onPointerDown={(e) => startTable(e, table.name)} />
          ))}
          {notes.map((n) => (
            <g key={n.id} transform={`translate(${n.x},${n.y})`} className="erd-note" onPointerDown={(e) => startNote(e, n)}>
              <rect width={NOTE_WIDTH} height={noteHeight(n)} rx={4} fill={colors.series[7]} fillOpacity={0.18} stroke={colors.series[7]} />
              {noteLines(n.text).map((line, i) => (
                <text key={i} x={8} y={20 + i * 16} fill={colors.text}>
                  {line}
                </text>
              ))}
            </g>
          ))}
        </svg>
      </div>
      {note && (
        <NoteDialog
          text={note.text}
          onSave={(text) => setNote(note.id, text)}
          onClose={() => setEditing(null)}
        />
      )}
      {choosing && all && (
        <TablesDialog
          all={all}
          chosen={tab.tables}
          // `null` would ask again for a schema too large to show whole without asking.
          onSave={(chosen) => onChange({ tables: chosen ?? (all.length > ERD_LIMIT ? all : null) })}
          onClose={() => setChoosing(false)}
        />
      )}
    </div>
  );
}

function TableBox({ table, box: b, at, colors, onPointerDown }: { table: SchemaTable; box: Box; at: Point; colors: ChartColors; onPointerDown: (e: PointerEvent) => void }) {
  const keys = new Set(table.details.foreign_keys.flatMap((f) => f.columns));
  return (
    <g transform={`translate(${at.x},${at.y})`} className="erd-table" onPointerDown={onPointerDown}>
      <title>{table.comment ?? table.name}</title>
      <rect width={b.width} height={b.height} rx={4} fill={colors.background} stroke={colors.grid} />
      <rect width={b.width} height={HEAD} rx={4} fill={colors.series[0]} fillOpacity={0.2} />
      <text x={8} y={17} fill={colors.text} fontWeight={600}>
        {table.name}
      </text>
      {table.details.columns.map((c, i) => (
        <g key={c.name} transform={`translate(0,${HEAD + i * ROW})`}>
          <text x={8} y={14} fill={c.primary_key ? colors.series[0] : colors.text} fontWeight={c.primary_key ? 600 : 400}>
            {`${keys.has(c.name) ? "→ " : ""}${c.name}${c.nullable ? "" : " *"}`}
          </text>
          <text x={b.width - 8} y={14} fill={colors.muted} textAnchor="end">
            {c.data_type.length > 24 ? `${c.data_type.slice(0, 23)}…` : c.data_type}
          </text>
        </g>
      ))}
    </g>
  );
}

/** The line of a foreign key, from the row of its column in `from` (its box at `a`) to the row
 *  of the referenced column in `to` (at `b`), leaving and entering the boxes on the sides facing
 *  each other. */
function edgePath(from: SchemaTable, a: Point & { width: number }, to: SchemaTable, b: Point & { width: number }, column?: string, refColumn?: string): string {
  const row = (t: SchemaTable, name?: string) => {
    let i = t.details.columns.findIndex((c) => c.name === name);
    if (i < 0) i = Math.max(0, t.details.columns.findIndex((c) => c.primary_key));
    return HEAD + i * ROW + ROW / 2;
  };
  const wa = a.width;
  const wb = b.width;
  const ya = a.y + row(from, column);
  const yb = b.y + row(to, refColumn);
  if (from === to) {
    const x = a.x + wa;
    return `M ${x} ${ya} C ${x + 40} ${ya}, ${x + 40} ${yb}, ${x} ${yb}`;
  }
  const right = b.x + wb / 2 > a.x + wa / 2;
  const xa = right ? a.x + wa : a.x;
  const xb = right ? b.x : b.x + wb;
  const bend = Math.max(30, Math.abs(xb - xa) / 2);
  const dir = right ? 1 : -1;
  return `M ${xa} ${ya} C ${xa + dir * bend} ${ya}, ${xb - dir * bend} ${yb}, ${xb} ${yb}`;
}

function NoteDialog({ text, onSave, onClose }: { text: string; onSave: (text: string | null) => void; onClose: () => void }) {
  const [value, setValue] = useState(text);
  const done = (result: string | null) => {
    onSave(result);
    onClose();
  };
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal small" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Note")}</h2>
        <textarea autoFocus rows={5} value={value} aria-label={t("Note")} onChange={(e) => setValue(e.target.value)} />
        <div className="actions">
          <button type="button" className="link" onClick={() => done(null)}>
            {t("Delete")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="button" className="primary" onClick={() => done(value)}>
            {t("Save")}
          </button>
        </div>
      </div>
    </div>
  );
}
