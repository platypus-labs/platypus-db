import { type PointerEvent, useEffect, useMemo, useRef, useState } from "react";
import { api, type TableDesign, type SchemaTable, toCommandError } from "../api";
import { type ChartColors, useChartColors } from "../analysis/Chart";
import { truncate } from "../analysis/chartScale";
import { exportChart } from "../analysis/chartExport";
import { commentHeader } from "../schema/script";
import { translateWarning } from "../schema/warnings";
import type { TabBase, TabProps } from "../tabs/registry";
import { ask } from "../ui/ask";
import { Icon } from "../ui/Icon";
import { workbench } from "../workbench";
import { t, msg, plural } from "../i18n";
import { blankDesign } from "../table-editor/DesignForm";
import { openObjectTab } from "../objects/open";
import { DesignDialog, type DesignKind } from "./DesignDialog";
import {
  type Drawn,
  type ErdEdits,
  type TableState,
  dragKey,
  drawnTables,
  emptyEdits,
  pendingCount,
  withDesign,
  withDropped,
  withoutDesign,
  withoutDrop,
  withRenamedNew,
} from "./edits";
import { type Box, type Point, placeTables } from "./layout";
import { pathThrough, quickWire, routeWires } from "./route";
import { Minimap } from "./Minimap";
import { TablesDialog } from "./TablesDialog";
import { LoadingState } from "../ui/State";

export type { ErdEdits };

export interface ErdNote {
  id: number;
  x: number;
  y: number;
  text: string;
}

/** An ER diagram of a schema: every table, or those chosen, placed by the user or laid out
 *  from the foreign keys, with notes. All of it is kept with the tab. In edit mode the
 *  diagram is also where the schema is drawn: tables added, changed and dropped, foreign
 *  keys dragged, the edits kept with the tab until they become a script. */
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
  /** The edits made on the diagram; `null` when it is not in edit mode. */
  edits: ErdEdits | null;
}

declare module "../tabs/registry" {
  interface TabKinds {
    erd: ErdTabModel;
  }
}

/** Above this many tables, a diagram of a whole schema asks which ones to show first. */
export const ERD_LIMIT = 100;

const HEAD = 28;
const ROW = 22;
/** The room of the key mark (PK, FK) at the start of a column row. */
const MARK = 24;
const CHAR = 7;
const NOTE_WIDTH = 200;
const NOTE_CHARS = 28;
const PAD = 20;
/** The radius of the handle of a column row, the start of a foreign key dragged. */
const HANDLE = 5;

/** The label in the head of a table that edits changed. */
const TAGS: Record<TableState, string> = { plain: "", new: msg("new"), changed: msg("changed"), dropped: msg("dropped") };

/** The characters of a column row that the name has: the box less the key mark, the type and
 *  the margins. */
const nameChars = (width: number, type: string) => Math.max(4, Math.floor((width - MARK - 6 - 10 - 8 - type.length * 6.5) / CHAR));

const typeText = (type: string) => truncate(type, 24);

const boxWidth = (table: Drawn) => {
  const tag = table.state === "plain" ? 0 : t(TAGS[table.state]).length + 1;
  const longest = Math.max(table.name.length + 2 + tag, ...table.columns.map((c) => c.name.length + c.data_type.length + 3));
  return Math.min(340, Math.max(140, 16 + MARK + CHAR * longest));
};

/** Whether `fk` references a table of `schema`, the diagram's: a table of another schema is
 *  not drawn, even with the name of one here. */
const local = (fk: { ref_schema: string }, schema: string) => !fk.ref_schema || fk.ref_schema === schema;

const box = (table: Drawn, schema: string): Box => ({
  name: table.name,
  width: boxWidth(table),
  height: HEAD + ROW * table.columns.length + 6,
  refs: table.foreign_keys
    .filter((f) => local(f, schema))
    .map((f) => f.ref_table)
    .filter((r) => r !== table.name),
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

type Dragging =
  | { what: "table"; name: string; dx: number; dy: number }
  | { what: "note"; id: number; dx: number; dy: number }
  | { what: "link"; table: string; column: string };

/** A foreign key being dragged from a column row, to the pointer. */
interface Link {
  from: Point;
  to: Point;
}

/** The table form open over the diagram. */
interface Open {
  design: TableDesign;
  kind: DesignKind;
  changed: boolean;
}

export function ErdTab({ tab, connection, active, sideSession, onChange, onError }: TabProps<ErdTabModel>) {
  const [tables, setTables] = useState<SchemaTable[] | null>(null);
  const [all, setAll] = useState<string[] | null>(null);
  const [choosing, setChoosing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const [moved, setMoved] = useState<Record<string, Point>>({});
  const [movedNotes, setMovedNotes] = useState<Record<number, Point>>({});
  const [editing, setEditing] = useState<number | null>(null);
  // Not kept with the tab: a diagram always opens in its place among the others.
  const [full, setFull] = useState(false);
  const [open, setOpen] = useState<Open | null>(null);
  const [link, setLink] = useState<Link | null>(null);
  // What the last edit action could not do, until the next one.
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const drag = useRef<Dragging | null>(null);
  /** The routes of the last drawing with no table being dragged, by wire. */
  const routed = useRef(new Map<string, Point[]>());
  const lastPress = useRef<{ what: string; at: number } | null>(null);
  const svg = useRef<SVGSVGElement>(null);
  const canvas = useRef<HTMLDivElement>(null);
  // The tab as the last render saw it, for the actions that wait for the database.
  const latest = useRef(tab);
  latest.current = tab;
  const colors = useChartColors();
  const id = (what: string) => `erd-${tab.key}-${what}`;
  // A read-only connection has no edit mode, whatever the tab kept.
  const edits = connection.read_only ? null : tab.edits;

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

  useEffect(() => {
    if (!active) setFull(false);
  }, [active]);
  useEffect(() => {
    if (!full) return;
    // In the capture phase, before a dialog's own handler closes it: Escape in a dialog closes
    // only the dialog.
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(".modal-backdrop, .context-menu, .palette-backdrop")) return;
      setFull(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [full]);

  const drawn = useMemo(() => drawnTables(tables ?? [], edits), [tables, edits]);
  const boxes = useMemo(() => drawn.map((d) => box(d, tab.schema)), [drawn, tab.schema]);
  // The tables the user has placed stay; the others go below them, never on top of them.
  const placed = useMemo(() => ({ ...placeTables(boxes, tab.positions), ...moved }), [boxes, tab.positions, moved]);
  const pos = (name: string): Point => placed[name] ?? { x: PAD, y: PAD };
  const notes = useMemo(() => tab.notes.map((n) => ({ ...n, ...movedNotes[n.id] })), [tab.notes, movedNotes]);
  const unreadable = (tables ?? []).filter((t) => t.error !== null);
  // Every foreign key drawn, its line routed around the tables from where they are now.
  const keys = useMemo(() => {
    const byName = new Map(drawn.map((d) => [d.name, d]));
    const shown = drawn.flatMap((table) =>
      table.foreign_keys.flatMap((fk) => {
        const target = local(fk, tab.schema) ? byName.get(fk.ref_table) : undefined;
        // A table going to be dropped takes its keys with it.
        if (!target || table.state === "dropped" || target.state === "dropped") return [];
        return [{ table, fk, target }];
      }),
    );
    const rects = Object.fromEntries(boxes.map((b) => [b.name, { ...pos(b.name), width: b.width, height: b.height }]));
    const wires = shown.map(({ table, fk, target }) => ({ from: table.name, fromRow: rowY(table, fk.columns[0]), to: target.name, toRow: rowY(target, fk.ref_columns[0]) }));
    const id = (w: (typeof wires)[number], i: number) => `${w.from}.${shown[i]!.fk.columns.join(",")}>${w.to}`;
    // While a table is dragged, only the wires touching it are redrawn (cheaply); the others
    // keep the route they had, and everything is routed again on the drop.
    const dragged = Object.keys(moved);
    const kept = routed.current;
    const touches = (w: (typeof wires)[number]) => dragged.includes(w.from) || dragged.includes(w.to);
    if (dragged.length > 0 && wires.every((w, i) => touches(w) || kept.has(id(w, i)))) {
      return shown.map((k, i) => ({ ...k, points: touches(wires[i]!) ? quickWire(rects, wires[i]!) : kept.get(id(wires[i]!, i))! }));
    }
    const lines = routeWires(rects, wires);
    if (dragged.length === 0) routed.current = new Map(wires.map((w, i) => [id(w, i), lines[i]!]));
    return shown.map((k, i) => ({ ...k, points: lines[i]! }));
  }, [drawn, boxes, placed, moved, tab.schema]);
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
    // A line may go round below the lowest table or right of the rightmost one.
    for (const k of keys) {
      for (const p of k.points) {
        w = Math.max(w, p.x + PAD);
        h = Math.max(h, p.y + PAD);
      }
    }
    return { w, h };
  }, [boxes, placed, notes, keys]);

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
      if (edits) void editTable(name);
      else openTable(name);
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
  /** The press on the handle of a column row: a foreign key from it is dragged to a table. */
  const startLink = (e: PointerEvent, table: string, column: string, row: number) => {
    e.stopPropagation();
    if (e.button !== 0) return;
    const b = boxes.find((o) => o.name === table)!;
    const from = { x: pos(table).x + b.width, y: pos(table).y + HEAD + row * ROW + ROW / 2 };
    drag.current = { what: "link", table, column };
    setMessage(null);
    setLink({ from, to: at(e) });
    svg.current!.setPointerCapture(e.pointerId);
  };
  const move = (e: PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const p = at(e);
    if (d.what === "link") return setLink((l) => l && { ...l, to: p });
    const to = { x: Math.max(0, Math.round(p.x - d.dx)), y: Math.max(0, Math.round(p.y - d.dy)) };
    if (d.what === "table") setMoved((m) => ({ ...m, [d.name]: to }));
    else setMovedNotes((m) => ({ ...m, [d.id]: to }));
  };
  const drop = (e: PointerEvent) => {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    if (d.what === "link") {
      setLink(null);
      if (e.type === "pointerup") dropLink(d.table, d.column, at(e));
      return;
    }
    // Every table is kept where it is, not only the one moved: the others were laid out
    // automatically, and must not jump when the next table appears or is moved.
    if (d.what === "table" && moved[d.name]) onChange({ positions: { ...tab.positions, ...placed } });
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
    openObjectTab(connection.id, { database: tab.database, schema: tab.schema, currentSchema: tab.currentSchema, name, kind: "table", parent: null });
  const save = (format: "png" | "svg") => {
    if (!svg.current) return;
    void exportChart(svg.current, format, `${tab.schema}-diagram`).catch((e: unknown) =>
      onError(e instanceof Error ? e.message : toCommandError(e).message),
    );
  };
  /** Reads the tables again, every one of them staying where it is now: the tables laid out
   *  automatically are frozen first, so nothing jumps when one appears or goes. */
  const refresh = () => {
    if (boxes.length > 0) onChange({ positions: { ...tab.positions, ...placed } });
    setVersion((v) => v + 1);
  };
  const zoom = (by: number) => onChange({ zoom: Math.min(2, Math.max(0.25, Math.round((tab.zoom + by) * 100) / 100)) });
  const note = editing === null ? undefined : tab.notes.find((n) => n.id === editing);

  // The edit mode.

  const pending = edits ? pendingCount(edits) : 0;
  const pendingText = plural(pending, "{n} pending change", "{n} pending changes");

  /** Turns the edit mode on, or off after asking when edits would be lost. */
  async function toggleEdit() {
    setMessage(null);
    if (!edits) return onChange({ edits: emptyEdits() });
    if (pending > 0 && !(await ask(t("Leave the edit mode and discard {changes}?", { changes: pendingText }), { confirm: t("Discard"), danger: true }))) return;
    onChange({ edits: null });
  }

  async function discard() {
    setMessage(null);
    if (!(await ask(t("Discard {changes}?", { changes: pendingText }), { confirm: t("Discard"), danger: true }))) return;
    onChange({ edits: emptyEdits() });
  }

  /** The design of a table as the edits have it, else as the database does. */
  async function designOf(name: string): Promise<TableDesign | null> {
    const edited = latest.current.edits?.designs.find((d) => d.name === name);
    if (edited) return edited;
    const sid = await sideSession();
    return sid === null ? null : api.tableDesign(sid, tab.schema, name);
  }

  /** Opens the form of a table to change, or keeps a table going to be dropped after all. */
  async function editTable(name: string) {
    const table = drawn.find((d) => d.name === name);
    if (!table || !edits) return;
    setMessage(null);
    if (table.state === "dropped") {
      if (await ask(t("Keep {name} after all?", { name }), { confirm: t("Keep") })) onChange({ edits: withoutDrop(latest.current.edits ?? emptyEdits(), name) });
      return;
    }
    try {
      const design = await designOf(name);
      if (design) setOpen({ design, kind: table.state === "new" ? "new" : "existing", changed: table.state === "changed" });
    } catch (e) {
      setMessage(toCommandError(e).message);
    }
  }

  const addTable = () => {
    setMessage(null);
    setOpen({ design: blankDesign(), kind: "add", changed: false });
  };

  /** Saves the form: a table added goes below the others, and the place is kept. */
  function saveDesign(design: TableDesign) {
    const form = open!;
    const current = latest.current.edits ?? emptyEdits();
    setOpen(null);
    if (form.kind === "add") {
      return onChange({ edits: withDesign(current, design), positions: { ...tab.positions, ...placed, [design.name]: { x: PAD, y: size.h } } });
    }
    if (form.kind === "new" && design.name !== form.design.name) {
      const { [form.design.name]: old, ...others } = { ...tab.positions, ...placed };
      return onChange({ edits: withRenamedNew(current, form.design.name, design), positions: old ? { ...others, [design.name]: old } : others });
    }
    // Nothing changed in a table of the database: it stays as it is.
    if (form.kind === "existing" && !form.changed && JSON.stringify(design) === JSON.stringify(form.design)) return;
    onChange({ edits: withDesign(current, design) });
  }

  function dropOpen() {
    const form = open!;
    setOpen(null);
    onChange({ edits: withDropped(latest.current.edits ?? emptyEdits(), form.design.name) });
  }

  function revertOpen() {
    const form = open!;
    setOpen(null);
    const { [form.design.name]: _gone, ...others } = tab.positions;
    onChange({
      edits: withoutDesign(latest.current.edits ?? emptyEdits(), form.design.name, form.kind === "new"),
      ...(form.kind === "new" ? { positions: others } : {}),
    });
  }

  /** Ends a drag from a column row at `p`: onto another table it makes a foreign key. */
  function dropLink(table: string, column: string, p: Point) {
    const target = drawn.find((d, i) => {
      const where = pos(d.name);
      return d.name !== table && p.x >= where.x && p.x <= where.x + boxes[i]!.width && p.y >= where.y && p.y <= where.y + boxes[i]!.height;
    });
    if (target) void addKey(table, column, target);
  }

  /** Adds to the design of `table` the foreign key from `column` to the key of `target`. */
  async function addKey(table: string, column: string, target: Drawn) {
    setMessage(null);
    if (target.state === "dropped") return setMessage(t("{name} is going to be dropped.", { name: target.name }));
    try {
      const design = await designOf(table);
      if (!design) return;
      const made = dragKey(design, column, target, tab.schema);
      if ("problem" in made) {
        const problems = {
          no_key: t("{name} has no primary key to reference.", { name: target.name }),
          composite_key: t("The primary key of {name} has more than one column: add this foreign key in the form of the table.", { name: target.name }),
          exists: t("There is a foreign key from {column} to {name} already.", { column, name: target.name }),
        };
        return setMessage(problems[made.problem]);
      }
      const current = latest.current.edits ?? emptyEdits();
      onChange({ edits: withDesign(current, { ...design, foreign_keys: [...design.foreign_keys, made.key] }) });
    } catch (e) {
      setMessage(toCommandError(e).message);
    }
  }

  /** Opens the script of the edits in a SQL editor of the connection, to review and run. */
  async function generate() {
    if (!edits) return;
    setBusy(true);
    setMessage(null);
    try {
      const sid = await sideSession();
      if (sid === null) return;
      const { script, sql } = await api.erdScript(sid, tab.schema, edits.designs, edits.dropped);
      if (script.statements.length === 0) return setMessage(t("The edits change nothing in the database."));
      const head = commentHeader(t("Applies the edits of the ER diagram of {schema}", { schema: tab.schema }), script.warnings.map(translateWarning));
      workbench().openTab("query", connection.id, { sql: `${head}${sql}`, database: tab.database });
    } catch (e) {
      setMessage(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  const form = open
    ? {
        taken: drawn.map((d) => d.name).filter((n) => open.kind === "add" || n !== open.design.name),
        columns: Object.fromEntries(drawn.map((d) => [d.name, d.columns.map((c) => c.name)])),
      }
    : null;

  return (
    <div className={`object-tab erd${full ? " erd-full" : ""}${edits ? " erd-editing" : ""}`} style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <span className="object-icon" data-kind="schema">
          <Icon name="schema" size={16} />
        </span>
        <div className="object-title">
          <strong>{t("ER diagram of {schema}", { schema: tab.schema })}</strong>
          <span className="muted small">
            {tab.tables === null ? t("all tables") : plural(tab.tables.length, "{n} table", "{n} tables")}
          </span>
        </div>
        <span className="spacer" />
        {!connection.read_only && (
          <button type="button" aria-pressed={edits !== null} className={edits ? "primary" : ""} onClick={() => void toggleEdit()} title={t("Draw the schema and generate the script")}>
            <Icon name="edit" />
            {t("Edit")}
          </button>
        )}
        <button type="button" disabled={!all} onClick={() => setChoosing(true)}>
          <Icon name="table" />
          {t("Tables…")}
        </button>
        <button type="button" onClick={addNote}>
          <Icon name="plus" />
          {t("Add note")}
        </button>
        <button type="button" onClick={() => setFull(!full)} aria-pressed={full}>
          <Icon name={full ? "minimize" : "maximize"} />
          {full ? t("Exit full screen") : t("Full screen")}
        </button>
        <button className="square" type="button" onClick={refresh} aria-label={t("Refresh")} title={t("Refresh")}>
          <Icon name="refresh" />
        </button>
      </header>
      {edits && (
        <div className="erd-edit-bar">
          <strong className="erd-pending">{pendingText}</strong>
          <span className="muted small">{t("Double-click a table to change it; drag from the circle of a column onto a table to add a foreign key.")}</span>
          <span className="spacer" />
          <button type="button" onClick={addTable}>
            {t("Add table")}
          </button>
          <button type="button" className="primary" disabled={pending === 0 || busy} onClick={() => void generate()}>
            {t("Generate DDL")}
          </button>
          <button type="button" disabled={pending === 0} onClick={() => void discard()}>
            {t("Discard changes")}
          </button>
        </div>
      )}
      {message && <div className="notice small erd-message">{message}</div>}
      {error && <div className="notice error small">{error}</div>}
      {unreadable.length > 0 && (
        <div className="notice error small">
          {t("Could not read {tables}: {error}", { tables: unreadable.map((u) => u.name).join(", "), error: unreadable[0]!.error! })}
        </div>
      )}
      <div className="erd-stage">
        <div className="erd-float">
          <button className="square" type="button" onClick={() => zoom(-0.25)} aria-label={t("Zoom out")} title={t("Zoom out")}>
            <Icon name="minus" />
          </button>
          <span className="small muted erd-zoom">{Math.round(tab.zoom * 100)}%</span>
          <button className="square" type="button" onClick={() => zoom(0.25)} aria-label={t("Zoom in")} title={t("Zoom in")}>
            <Icon name="plus" />
          </button>
          <span className="erd-sep" />
          <button type="button" onClick={() => onChange({ positions: {} })} title={t("Place every table again from the foreign keys")}>
            <Icon name="schema" />
            {t("Auto layout")}
          </button>
          <span className="erd-sep" />
          <button type="button" onClick={() => save("svg")} title={t("Save the diagram as an SVG image")}>
            <Icon name="download" />
            SVG…
          </button>
          <button type="button" onClick={() => save("png")} title={t("Save the diagram as a PNG image")}>
            <Icon name="download" />
            PNG…
          </button>
        </div>
      <div className="erd-canvas" ref={canvas}>
        {tables === null && !error && <LoadingState className="erd-loading" title={t("Reading the tables…")} />}
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
          <rect className="erd-bg" width={size.w} height={size.h} fill={colors.background} />
          {keys.map(({ table, fk, target, points }) => (
            <path
              key={`${table.name}/${fk.name}`}
              d={pathThrough(points)}
              stroke={colors.muted}
              fill="none"
              data-fk={`${table.name}.${fk.columns.join(",")}->${target.name}`}
              markerStart={`url(#${id("many")})`}
              markerEnd={`url(#${id("one")})`}
            >
              <title>{`${fk.name}: ${table.name} (${fk.columns.join(", ")}) → ${fk.ref_table} (${fk.ref_columns.join(", ")})`}</title>
            </path>
          ))}
          {drawn.map((table, i) => (
            <TableBox
              key={table.name}
              table={table}
              box={boxes[i]!}
              at={pos(table.name)}
              colors={colors}
              editing={edits !== null}
              onPointerDown={(e) => startTable(e, table.name)}
              onLink={(e, column, row) => startLink(e, table.name, column, row)}
            />
          ))}
          {link && <path d={`M ${link.from.x} ${link.from.y} L ${link.to.x} ${link.to.y}`} stroke={colors.series[0]} strokeDasharray="4 3" fill="none" pointerEvents="none" />}
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
      <Minimap
        canvas={canvas}
        rects={boxes.map((b) => ({ name: b.name, ...pos(b.name), width: b.width, height: b.height }))}
        zoom={tab.zoom}
        version={size}
      />
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
      {open && form && (
        <DesignDialog
          driver={connection.driver}
          schema={tab.schema}
          design={open.design}
          kind={open.kind}
          changed={open.changed}
          taken={form.taken}
          columns={form.columns}
          onSave={saveDesign}
          onDrop={dropOpen}
          onRevert={revertOpen}
          onClose={() => setOpen(null)}
        />
      )}
    </div>
  );
}

interface TableBoxProps {
  table: Drawn;
  box: Box;
  at: Point;
  colors: ChartColors;
  /** Draws the handles of the column rows. */
  editing: boolean;
  onPointerDown: (e: PointerEvent) => void;
  onLink: (e: PointerEvent, column: string, row: number) => void;
}

function TableBox({ table, box: b, at, colors, editing, onPointerDown, onLink }: TableBoxProps) {
  const keys = new Set(table.foreign_keys.flatMap((f) => f.columns));
  const dropped = table.state === "dropped";
  // Tables edited stand out: created ones in green with a dashed border, changed ones in the
  // warning colour, dropped ones faded with their name struck through.
  const head = table.state === "new" ? colors.series[1] : table.state === "changed" ? colors.series[7] : colors.series[0];
  return (
    <g transform={`translate(${at.x},${at.y})`} className="erd-table" data-state={table.state} opacity={dropped ? 0.45 : 1} onPointerDown={onPointerDown}>
      <title>{table.comment ?? table.name}</title>
      <rect width={b.width} height={b.height} rx={4} fill={colors.background} stroke={table.state === "new" ? head : colors.grid} strokeDasharray={table.state === "new" ? "6 3" : undefined} />
      <rect className="erd-head" width={b.width} height={HEAD} rx={4} fill={head} fillOpacity={table.state === "plain" ? 0.14 : 0.4} />
      <path className="erd-rule" d={`M 0 ${HEAD} H ${b.width}`} stroke={colors.grid} />
      <text x={10} y={18} fill={colors.text} fontWeight={600} textDecoration={dropped ? "line-through" : undefined}>
        {truncate(table.name, Math.max(4, Math.floor((b.width - 20 - (table.state === "plain" ? 0 : (t(TAGS[table.state]).length + 1) * 6)) / CHAR)))}
      </text>
      {table.state !== "plain" && (
        <text x={b.width - 10} y={18} fill={colors.muted} fontSize={10} textAnchor="end" className="erd-tag">
          {t(TAGS[table.state])}
        </text>
      )}
      {table.columns.map((c, i) => (
        <g key={c.name} transform={`translate(0,${HEAD + i * ROW})`}>
          {c.primary_key && <KeyMark label="PK" fill={colors.series[0]!} />}
          {!c.primary_key && keys.has(c.name) && <KeyMark label="FK" fill={colors.series[4]!} />}
          <text x={MARK + 6} y={15} fill={c.primary_key ? colors.series[0] : colors.text} fontWeight={c.primary_key ? 600 : 400}>
            {truncate(c.name, nameChars(b.width, typeText(c.data_type)) - (c.nullable ? 0 : 2)) + (c.nullable ? "" : " *")}
          </text>
          <text x={b.width - (editing && !dropped ? 22 : 10)} y={15} fill={colors.muted} fontSize={11} textAnchor="end">
            {typeText(c.data_type)}
          </text>
          {editing && !dropped && c.name !== "" && (
            <circle
              className="erd-handle"
              data-table={table.name}
              data-column={c.name}
              cx={b.width - HANDLE - 3}
              cy={ROW / 2}
              r={HANDLE}
              fill={colors.background}
              stroke={colors.series[0]}
              strokeWidth={1.5}
              onPointerDown={(e) => onLink(e, c.name, i)}
            >
              <title>{t("Drag onto a table to add a foreign key")}</title>
            </circle>
          )}
        </g>
      ))}
    </g>
  );
}

/** The small PK or FK mark at the start of a column row. */
function KeyMark({ label, fill }: { label: string; fill: string }) {
  return (
    <g className="erd-key" pointerEvents="none">
      <rect x={6} y={4} width={MARK - 4} height={14} rx={3} fill={fill} fillOpacity={0.18} />
      <text x={6 + (MARK - 4) / 2} y={14} fill={fill} fontSize={9} fontWeight={600} textAnchor="middle">
        {label}
      </text>
    </g>
  );
}

/** Where the line of a foreign key meets `table`: the middle of the row of `column` (from the
 *  top of the box), or of its primary key when the column is not there. */
function rowY(table: Drawn, column?: string): number {
  let i = table.columns.findIndex((c) => c.name === column);
  if (i < 0) i = Math.max(0, table.columns.findIndex((c) => c.primary_key));
  return HEAD + i * ROW + ROW / 2;
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
