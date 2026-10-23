import { useCallback, useEffect, useRef, useState } from "react";
import { api, type CellInput, type ColumnInfo, type SavedConnection, type TableDetails, toCommandError } from "../../api";
import { type Cell, type CellDecoration, DataGrid, type GridApi, type GridSource } from "../DataGrid";
import type { RowCache } from "../RowCache";
import { CellEditor, type Move } from "./CellEditor";
import { type EditMeta, type EditorKind, editMeta, editorKind, isJson, type TableRef } from "./meta";
import { PreviewDialog } from "./PreviewDialog";
import * as edits from "./state";
import "./actions";

/** A data viewer result the editor may change. */
export interface TableEditTarget {
  connection: SavedConnection;
  table: TableRef;
  /** Why this result cannot be edited even if the table can, e.g. the query was changed. */
  readOnly: string | null;
  /** The table's columns and keys, cached by the tab. */
  loadDetails: () => Promise<TableDetails>;
  /** Called after saving, to reload the data. */
  onSaved: () => void;
  onDirtyChange: (dirty: boolean) => void;
}

/** The data editor as grid actions see it, in `source.edit`. */
export interface TableEdit {
  editable: boolean;
  dirty: boolean;
  canUndo: boolean;
  /** Opens the inline editor; `typed` replaces the value with the key that started it. */
  editCell(cell: Cell, typed?: string): Promise<void>;
  /** Sets every selected cell. */
  setSelection(value: CellInput): void;
  addRow(): void;
  duplicateSelection(): Promise<void>;
  deleteSelection(): void;
  undo(): void;
  save(): Promise<void>;
  preview(): Promise<void>;
  discard(): void;
}

declare module "../DataGrid" {
  interface GridSourceExtensions {
    /** The data editor, on data viewer results. */
    edit?: TableEdit;
  }
}

interface Props {
  target: TableEditTarget;
  jobId: number;
  columns: ColumnInfo[];
  rowCount: number;
  cache: RowCache;
  version: number;
  sort?: { column: string; desc: boolean } | null;
  onHeaderClick?: (column: number) => void;
  source?: GridSource | null;
}

interface Editing {
  cell: Cell;
  kind: EditorKind;
  initial: string | null;
  typed?: string;
  /** The loaded value, for result rows. */
  original?: string | null;
}

// Translucent, so they read on light and dark grids alike.
const CHANGED = "rgba(250, 176, 5, 0.24)";
const INSERTED = "rgba(64, 192, 87, 0.2)";
const MUTED = "#8b919a";
const DELETED: CellDecoration = { background: "rgba(250, 82, 82, 0.2)", color: MUTED, strike: true };
const INSERTED_DEFAULT: CellDecoration = { background: INSERTED, text: "DEFAULT", color: MUTED };
const UNDO_LIMIT = 200;
/** As in `display.rs`: the grid shows values on one line, cut. */
const MAX_CELL_CHARS = 500;

/** One-line text of pending values, computed once per value: decorations run every frame. */
const shownText = new WeakMap<CellInput, string | null>();

function decoration(value: CellInput, background: string): CellDecoration {
  let text = shownText.get(value);
  if (text === undefined) {
    if (value.kind === "null") text = null;
    else if (value.kind === "default") text = "DEFAULT";
    else {
      const cut = value.text.length > MAX_CELL_CHARS ? `${value.text.slice(0, MAX_CELL_CHARS)}…` : value.text;
      text = cut.replace(/[\n\r\t]/g, " ");
    }
    shownText.set(value, text);
  }
  return { background, text, color: value.kind === "default" ? MUTED : undefined };
}

/** The data viewer's grid with editing: inline editors, new and deleted rows, save. */
export function TableEditor({ target, jobId, columns, rowCount, cache, version, sort, onHeaderClick, source }: Props) {
  const grid = useRef<GridApi>(null);
  const box = useRef<HTMLDivElement>(null);
  const [meta, setMeta] = useState<EditMeta | string | null>(null);
  const [state, setState] = useState(edits.EMPTY);
  const current = useRef(state);
  const undoStack = useRef<edits.EditState[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<string[] | null>(null);
  const [, setLayout] = useState(0);
  const latest = useRef(target);
  latest.current = target;

  const reason = target.connection.read_only ? "the connection is read-only" : target.readOnly;
  useEffect(() => {
    if (reason !== null) return;
    let live = true;
    latest.current.loadDetails().then(
      (details) => live && setMeta(editMeta(latest.current.table, columns, details)),
      (e) => live && setMeta(`its keys could not be read (${toCommandError(e).message})`),
    );
    return () => {
      live = false;
    };
  }, [columns, reason]);
  const keys = typeof meta === "object" ? meta : null;
  const readOnly = reason ?? (typeof meta === "string" ? meta : null);
  const editable = readOnly === null && keys !== null && !busy;
  const dirty = !edits.isEmpty(state);

  useEffect(() => latest.current.onDirtyChange(dirty), [dirty]);
  useEffect(() => () => latest.current.onDirtyChange(false), []);

  function apply(change: (s: edits.EditState) => edits.EditState) {
    const prev = current.current;
    const next = change(prev);
    if (next === prev) return;
    undoStack.current.push(prev);
    if (undoStack.current.length > UNDO_LIMIT) undoStack.current.shift();
    current.current = next;
    setState(next);
    setError(null);
  }

  const fail = (e: unknown) => setError(toCommandError(e).message);

  /** Grid rows of the selection, or of the focused cell. */
  function selectedRows(): number[] {
    const sel = grid.current?.selection();
    if (!sel) return [];
    return Array.from({ length: sel.r1 - sel.r0 + 1 }, (_, i) => sel.r0 + i);
  }

  async function editCell(cell: Cell, typed?: string) {
    if (!editable || !keys) return;
    const s = current.current;
    const ref = edits.rowRef(s, cell.row);
    if ("row" in ref && s.deletes.has(ref.row)) return;
    let original: string | null | undefined;
    if ("row" in ref) {
      try {
        original = (await api.rowValues(jobId, ref.row))[cell.col];
      } catch (e) {
        return fail(e);
      }
    }
    const pending = edits.cellEdit(s, cell.row, cell.col);
    const initial = pending
      ? pending.kind === "value"
        ? pending.text
        : pending.kind === "null"
          ? null
          : ""
      : (original ?? null);
    grid.current?.select(cell);
    const column = columns[cell.col]!;
    setEditing({ cell, initial, typed, original, kind: editorKind(column, keys.columns[cell.col], typed ?? initial) });
  }

  function commit(value: CellInput, move: Move) {
    const e = editing;
    if (!e) return;
    apply((s) => edits.setCells(s, [{ gridRow: e.cell.row, col: e.cell.col, value, original: e.original }]));
    setEditing(null);
    const g = grid.current;
    if (!g) return;
    const { row, col } = e.cell;
    if (move) {
      const next = {
        down: { row: row + 1, col },
        up: { row: row - 1, col },
        right: { row, col: col + 1 },
        left: { row, col: col - 1 },
      }[move];
      g.select(next);
    }
    g.focus();
  }

  function cancelEdit() {
    setEditing(null);
    grid.current?.focus();
  }

  const edit: TableEdit = {
    editable,
    dirty,
    canUndo: undoStack.current.length > 0,
    editCell,
    setSelection: (value) => {
      const sel = grid.current?.selection();
      if (!editable || !sel) return;
      const cells: edits.CellChange[] = [];
      for (let r = sel.r0; r <= sel.r1; r++) {
        for (let c = sel.c0; c <= sel.c1; c++) cells.push({ gridRow: r, col: c, value });
      }
      apply((s) => edits.setCells(s, cells));
    },
    addRow: () => {
      if (!editable) return;
      apply((s) => edits.addRows(s, [new Map()]));
      grid.current?.select({ row: 0, col: grid.current.focused()?.col ?? 0 });
    },
    duplicateSelection: async () => {
      if (!editable || !keys) return;
      const s = current.current;
      // Key columns with a default (serials, identities) get a fresh value.
      const generated = keys.key.filter((c) => keys.columns[c]?.default != null);
      try {
        const rows = await Promise.all(
          selectedRows().map(async (gridRow): Promise<edits.Cells> => {
            const ref = edits.rowRef(s, gridRow);
            if ("insert" in ref) return s.inserts[ref.insert]!;
            const values = await api.rowValues(jobId, ref.row);
            const cells = new Map<number, CellInput>(
              values.map((v, c) => [c, v === null ? { kind: "null" } : { kind: "value", text: v }]),
            );
            for (const c of generated) cells.set(c, { kind: "default" });
            return cells;
          }),
        );
        apply((latestState) => edits.addRows(latestState, rows));
      } catch (e) {
        fail(e);
      }
    },
    deleteSelection: () => {
      if (editable) apply((s) => edits.deleteRows(s, selectedRows()));
    },
    undo: () => {
      const prev = undoStack.current.pop();
      if (!prev) return;
      current.current = prev;
      setState(prev);
    },
    save: async () => {
      if (!editable || !keys || edits.isEmpty(current.current)) return;
      const t = latest.current;
      const changes = edits.toChanges(current.current, {
        table: { schema: t.table.schema, name: t.table.name },
        key: keys.key,
        defaults: keys.columns.map((c) => c?.default ?? null),
      });
      setBusy(true);
      setPreview(null);
      try {
        await api.saveChanges(jobId, changes);
        undoStack.current = [];
        current.current = edits.EMPTY;
        setState(edits.EMPTY);
        t.onSaved();
      } catch (e) {
        fail(e);
      } finally {
        setBusy(false);
      }
    },
    preview: async () => {
      if (!keys || edits.isEmpty(current.current)) return;
      const t = latest.current;
      try {
        setPreview(
          await api.previewChanges(
            jobId,
            edits.toChanges(current.current, {
              table: { schema: t.table.schema, name: t.table.name },
              key: keys.key,
              defaults: keys.columns.map((c) => c?.default ?? null),
            }),
          ),
        );
      } catch (e) {
        fail(e);
      }
    },
    discard: () => apply(() => edits.EMPTY),
  };

  const decorate = useCallback(
    (row: number, col: number): CellDecoration | undefined => {
      const n = state.inserts.length;
      if (row < n) {
        const value = state.inserts[row]!.get(col);
        return value ? decoration(value, INSERTED) : INSERTED_DEFAULT;
      }
      if (state.deletes.has(row - n)) return DELETED;
      const value = state.updates.get(row - n)?.get(col);
      return value && decoration(value, CHANGED);
    },
    [state],
  );

  /** Typing on a cell starts editing it with that key. */
  function onKeyDown(e: React.KeyboardEvent) {
    if (!editable || editing || e.ctrlKey || e.metaKey || e.altKey || e.key.length !== 1) return;
    if (!(e.target as HTMLElement).classList.contains("grid-scroll")) return;
    const focused = grid.current?.focused();
    if (!focused) return;
    e.preventDefault();
    void editCell(focused, e.key);
  }

  /** Scrolling commits the open editor (through its blur) and moves it with the cell. */
  function onScroll() {
    if (!editing) return;
    const active = document.activeElement;
    if (active instanceof HTMLElement && box.current?.contains(active)) active.blur();
    setLayout((n) => n + 1);
  }

  const rect = editing && grid.current?.cellRect(editing.cell.row, editing.cell.col);
  const column = editing ? columns[editing.cell.col] : undefined;
  const columnMeta = editing ? keys?.columns[editing.cell.col] : undefined;
  const overlay = editing && rect && column && (
    <CellEditor
      key={`${editing.cell.row}:${editing.cell.col}`}
      rect={rect}
      bounds={{ width: box.current?.clientWidth ?? 0, height: box.current?.clientHeight ?? 0 }}
      kind={editing.kind}
      initial={editing.initial}
      typed={editing.typed}
      enumValues={columnMeta?.enum_values ?? []}
      nullable={columnMeta?.nullable ?? true}
      json={isJson(column, columnMeta)}
      numeric={column.numeric}
      onCommit={commit}
      onCancel={cancelEdit}
    />
  );

  return (
    <div className="table-editor">
      <div className="edit-bar">
        {readOnly !== null ? (
          <span className="muted small">Read-only: {readOnly}.</span>
        ) : meta === null ? (
          <span className="muted small">Checking the table's keys…</span>
        ) : (
          <>
            <button onClick={edit.addRow} disabled={!editable} title="Add a row (Insert)">
              + Row
            </button>
            <button onClick={edit.deleteSelection} disabled={!editable} title="Delete the selected rows (Delete)">
              − Row
            </button>
            {dirty ? (
              <>
                <span className="edit-count">
                  {edits.changedRows(state)} {edits.changedRows(state) === 1 ? "row" : "rows"} changed
                </span>
                <button className="primary" onClick={() => void edit.save()} disabled={!editable} title="Save (Ctrl+S)">
                  {busy ? "Saving…" : "Save"}
                </button>
                <button onClick={() => void edit.preview()} disabled={busy}>
                  Preview SQL
                </button>
                <button onClick={edit.undo} disabled={busy || !edit.canUndo} title="Undo (Ctrl+Z)">
                  Undo
                </button>
                <button onClick={edit.discard} disabled={busy}>
                  Discard
                </button>
              </>
            ) : (
              <span className="muted small">Double-click, F2 or type to edit a cell</span>
            )}
          </>
        )}
        {error && (
          <span className="error-text small edit-error" title={error}>
            {error}
            <button className="link" onClick={() => setError(null)} title="Dismiss">
              ×
            </button>
          </span>
        )}
      </div>
      <div className="table-editor-grid" ref={box} onKeyDown={onKeyDown}>
        <DataGrid
          ref={grid}
          columns={columns}
          rowCount={rowCount}
          topRows={state.inserts.length}
          cache={cache}
          version={version}
          sort={sort}
          onHeaderClick={onHeaderClick}
          source={source ? { ...source, edit } : null}
          decorate={dirty ? decorate : undefined}
          overlay={overlay || null}
          onCellDoubleClick={editable ? (cell) => void editCell(cell) : undefined}
          onScroll={onScroll}
        />
      </div>
      {preview && (
        <PreviewDialog
          statements={preview}
          busy={busy}
          onSave={() => void edit.save()}
          onClose={() => setPreview(null)}
        />
      )}
    </div>
  );
}
