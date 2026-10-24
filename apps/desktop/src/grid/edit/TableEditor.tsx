import { type ReactNode, type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { api, type CellInput, type ColumnInfo, type SavedConnection, toCommandError } from "../../api";
import type { Cell, CellDecoration, GridApi } from "../DataGrid";
import type { RowCache } from "../RowCache";
import { CellEditor, type Move } from "./CellEditor";
import { Dictionary } from "./dictionary";
import { type EditMeta, type EditSource, type EditorKind, editMeta, editorKind, isJson, type ReadOnly } from "./meta";
import { plainNumber } from "../formats";
import { mapFields, parseClipboard } from "./paste";
import { needsProductionConfirm } from "../../guards/guards";
import { PreviewDialog } from "./PreviewDialog";
import * as edits from "./state";
import { VirtualKeyDialog } from "./VirtualKeyDialog";
import "./actions";

/** A result the editor may change: the data viewer's, or a query's of one table. */
export interface TableEditTarget {
  connection: SavedConnection;
  /** Identifies the source, e.g. the query; the editor loads the table again when it changes. */
  key: string;
  /** Hides the edit bar until the result turns out to be editable: results in the SQL
   *  editor, most of which are not of one table. */
  quiet: boolean;
  /** Why this result cannot be edited even if the table can. */
  readOnly: string | null;
  /** The table behind the result; `null` when the result is not one table's rows. */
  load: () => Promise<EditSource | null>;
  /** The navigator session, for the tables foreign keys point to. */
  session: () => Promise<number | null>;
  /** Called after saving, to reload the data; `undo` puts the rows back. */
  onSaved: (undo: string) => void;
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
  /** Pastes the clipboard (spreadsheet cells or CSV) over the cells from the focused one,
   *  or with `asRows` as new rows. */
  paste(asRows: boolean): Promise<void>;
  undo(): void;
  /** Saves the changes; on a production connection the SQL is shown first, unless
   *  `confirmed` (the preview's Save). */
  save(confirmed?: boolean): Promise<void>;
  preview(): Promise<void>;
  discard(): void;
}

declare module "../DataGrid" {
  interface GridSourceExtensions {
    /** The data editor, on results of one table. */
    edit?: TableEdit;
  }
}

interface Options {
  /** `null` when the result is not a table's data. */
  target: TableEditTarget | null;
  jobId: number;
  columns: ColumnInfo[];
  /** The result's rows, for the labels of foreign keys. */
  cache: RowCache;
  /** Bumped whenever the cache receives rows. */
  version: number;
  grid: RefObject<GridApi | null>;
  /** The element holding the grid: bounds the editors and receives the typed keys. */
  box: RefObject<HTMLDivElement | null>;
}

/** What the editor adds to a result grid. */
export interface TableEditorHost {
  /** Given to grid actions as `source.edit`; `undefined` when the result is not a table's. */
  edit: TableEdit | undefined;
  /** The edit bar above the grid. */
  bar: ReactNode;
  /** Dialogs, e.g. the SQL preview. */
  dialog: ReactNode;
  /** New rows, shown above the result's. */
  topRows: number;
  decorate?: (row: number, col: number) => CellDecoration | undefined;
  overlay: ReactNode;
  onCellDoubleClick?: (cell: Cell) => void;
  onScroll: () => void;
  onKeyDown: (e: React.KeyboardEvent) => void;
}

interface Editing {
  cell: Cell;
  kind: EditorKind;
  initial: string | null;
  typed?: string;
  /** The loaded value, for result rows. */
  original?: string | null;
  /** Rows of the table a foreign key points to, once loaded. */
  options?: [string, string | null][];
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

const columnsOf = (meta: EditMeta) => meta.columns.flatMap((c, i) => (c ? [i] : []));

/** Editing of a result of one table: inline editors, new and deleted rows, save. */
export function useTableEditor({ target, jobId, columns, cache, version, grid, box }: Options): TableEditorHost {
  const [meta, setMeta] = useState<EditMeta | ReadOnly | null>(null);
  const [state, setState] = useState(edits.EMPTY);
  const current = useRef(state);
  const undoStack = useRef<edits.EditState[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<string[] | null>(null);
  const [keyDialog, setKeyDialog] = useState(false);
  const [, setLayout] = useState(0);
  const dictionary = useRef<Dictionary | null>(null);
  const [labels, setLabels] = useState(0);
  const latest = useRef(target);
  latest.current = target;

  const reason = !target
    ? "not a table"
    : target.connection.read_only
      ? "the connection is read-only"
      : target.readOnly;
  const virtualKeys = target?.connection.virtual_keys ?? [];
  const virtualSignature = JSON.stringify(virtualKeys);
  useEffect(() => {
    const t = latest.current;
    if (reason !== null || !t) return;
    let live = true;
    t.load().then(
      (source) => {
        if (!live) return;
        setMeta(source ? editMeta(source, columns, virtualKeys) : { reason: "the rows are not one table's", hidden: true });
      },
      (e) => live && setMeta({ reason: `its keys could not be read (${toCommandError(e).message})` }),
    );
    return () => {
      live = false;
    };
    // `virtualKeys` changes with its signature.
  }, [columns, reason, target?.key, virtualSignature]);
  const keys = meta && "key" in meta ? meta : null;
  const readOnly = reason ?? (meta && "reason" in meta ? meta.reason : null);
  const editable = readOnly === null && keys !== null && !busy;
  const dirty = !edits.isEmpty(state);

  useEffect(() => latest.current?.onDirtyChange(dirty), [dirty]);
  useEffect(() => () => latest.current?.onDirtyChange(false), []);

  // Foreign keys show the rows they point to.
  useEffect(() => {
    const t = latest.current;
    if (!keys || !t) return;
    const dict = new Dictionary(t.session, () => setLabels((n) => n + 1));
    dictionary.current = dict;
    void dict.init(keys);
    return () => {
      dict.dispose();
      dictionary.current = null;
    };
  }, [keys]);

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

  /** Whether a result column is one of the table's, so it can be written. */
  const writable = (col: number) => keys?.columns[col] !== undefined;

  async function editCell(cell: Cell, typed?: string) {
    if (!editable || !keys) return;
    if (!writable(cell.col)) {
      setError(`${columns[cell.col]?.name ?? "This column"} is not a column of ${keys.table.name}: it cannot be edited`);
      return;
    }
    const s = current.current;
    const ref = edits.rowRef(s, cell.row);
    if ("row" in ref && s.deletes.has(ref.row)) return;
    let original: string | null | undefined;
    if ("row" in ref) {
      try {
        original = (await api.rowValues(jobId, ref.row, 1))[0]?.[cell.col];
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
    const dict = dictionary.current;
    const lookup = dict?.has(cell.col) ?? false;
    const kind = editorKind(column, keys.columns[cell.col], typed ?? initial, lookup);
    setEditing({ cell, initial, typed, original, kind });
    if (lookup && dict) {
      dict.options(cell.col).then(
        (options) => setEditing((e) => (e?.cell === cell ? { ...e, options } : e)),
        fail,
      );
    }
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

  async function paste(asRows: boolean) {
    if (!editable || !keys) return;
    let text: string;
    try {
      text = await api.readClipboard();
    } catch (e) {
      return fail(e);
    }
    const focused = grid.current?.focused() ?? { row: 0, col: 0 };
    const shown = grid.current?.visibleColumns() ?? columns.map((_, i) => i);
    const { targets, rows } = mapFields(
      parseClipboard(text),
      columns.map((c) => c.name),
      shown.slice(Math.max(0, shown.indexOf(focused.col))),
    );
    const fields = (row: string[]) =>
      row.flatMap((value, i) => {
        const col = targets[i] ?? -1;
        return col >= 0 && writable(col) ? [{ col, value }] : [];
      });
    if (asRows) {
      // Empty fields take the column's default.
      const added = rows.map(
        (row) =>
          new Map<number, CellInput>(
            fields(row)
              .filter((f) => f.value !== "")
              .map((f) => [f.col, { kind: "value", text: f.value }]),
          ),
      );
      apply((s) => edits.addRows(s, added));
      grid.current?.select({ row: 0, col: focused.col }, { row: added.length - 1, col: focused.col });
      return;
    }
    // Over the cells from the focused one; empty fields are NULL.
    const total = grid.current?.rowCount() ?? 0;
    const cells: edits.CellChange[] = rows.slice(0, Math.max(0, total - focused.row)).flatMap((row, r) =>
      fields(row).map(({ col, value }) => ({
        gridRow: focused.row + r,
        col,
        value: value === "" ? { kind: "null" } : { kind: "value", text: value },
      })),
    );
    apply((s) => edits.setCells(s, cells));
  }

  function changes(meta: EditMeta) {
    return edits.toChanges(current.current, {
      table: { schema: meta.table.schema, name: meta.table.name },
      key: meta.key,
      table_columns: columnsOf(meta),
      defaults: meta.columns.map((c) => c?.default ?? null),
    });
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
        for (const c of sel.cols) if (writable(c)) cells.push({ gridRow: r, col: c, value });
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
            const [values = []] = await api.rowValues(jobId, ref.row, 1);
            const cells = new Map<number, CellInput>(
              values.flatMap((v, c) =>
                writable(c) ? [[c, v === null ? { kind: "null" } : { kind: "value", text: v }] as const] : [],
              ),
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
    paste,
    undo: () => {
      const prev = undoStack.current.pop();
      if (!prev) return;
      current.current = prev;
      setState(prev);
    },
    save: async (confirmed = false) => {
      if (!editable || !keys || edits.isEmpty(current.current)) return;
      if (!confirmed && target && needsProductionConfirm(target.connection)) return edit.preview();
      setBusy(true);
      setPreview(null);
      try {
        const saved = await api.saveChanges(jobId, changes(keys));
        undoStack.current = [];
        current.current = edits.EMPTY;
        setState(edits.EMPTY);
        latest.current?.onSaved(saved.undo);
      } catch (e) {
        fail(e);
      } finally {
        setBusy(false);
      }
    },
    preview: async () => {
      if (!keys || edits.isEmpty(current.current)) return;
      try {
        setPreview(await api.previewChanges(jobId, changes(keys)));
      } catch (e) {
        fail(e);
      }
    },
    discard: () => apply(() => edits.EMPTY),
  };

  const decorate = useCallback(
    (row: number, col: number): CellDecoration | undefined => {
      const n = state.inserts.length;
      const dict = dictionary.current;
      let d: CellDecoration | undefined;
      let value: string | null | undefined;
      if (row < n) {
        const input = state.inserts[row]!.get(col);
        d = input ? decoration(input, INSERTED) : INSERTED_DEFAULT;
        value = input?.kind === "value" ? input.text : null;
      } else if (state.deletes.has(row - n)) {
        return DELETED;
      } else {
        const input = state.updates.get(row - n)?.get(col);
        d = input && decoration(input, CHANGED);
        // Loaded numbers may be shown grouped; keys are looked up as plain numbers.
        const loaded = cache.get(row - n, col);
        value = input
          ? input.kind === "value"
            ? input.text
            : null
          : loaded != null && columns[col]?.numeric
            ? plainNumber(loaded)
            : loaded;
      }
      if (!dict?.has(col) || value == null) return d;
      const label = dict.label(col, value);
      return label ? { ...d, hint: label } : d;
    },
    // `labels` and `version` change what the dictionary and the cache hold.
    [state, labels, version, cache, columns],
  );
  const decorated = dirty || (dictionary.current?.size ?? 0) > 0;

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
      type={columnMeta?.data_type ?? column.db_type}
      fields={columnMeta?.fields ?? []}
      nullable={columnMeta?.nullable ?? true}
      json={isJson(column, columnMeta)}
      numeric={column.numeric}
      options={editing.options}
      onCommit={commit}
      onCancel={cancelEdit}
    />
  );

  const noKey = meta && "reason" in meta ? meta.noKey : undefined;
  const showBar = !target?.quiet || (meta !== null && !("hidden" in meta && meta.hidden));
  const keyTable = keys?.table ?? noKey?.table;

  const bar = target && (
    <div className="edit-bar" hidden={!showBar}>
      {readOnly !== null ? (
        <>
          <span className="muted small">Read-only: {readOnly}.</span>
          {noKey && (
            <button onClick={() => setKeyDialog(true)} title="Columns that tell the rows apart">
              Set a virtual key…
            </button>
          )}
        </>
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
          {keys?.virtualKey && (
            <button className="link small" onClick={() => setKeyDialog(true)} title="Change the virtual key">
              Virtual key: {keys.virtualKey.join(", ")}
            </button>
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
  );

  return {
    edit: target ? edit : undefined,
    bar,
    dialog: (
      <>
        {preview && (
          <PreviewDialog
            statements={preview}
            busy={busy}
            production={!!target && needsProductionConfirm(target.connection)}
            onSave={() => void edit.save(true)}
            onClose={() => setPreview(null)}
          />
        )}
        {keyDialog && keyTable && target && (
          <VirtualKeyDialog
            connection={target.connection}
            table={keyTable}
            columns={keys ? keys.details.columns.map((c) => c.name) : (noKey?.columns ?? [])}
            current={keys?.virtualKey ?? null}
            onClose={() => setKeyDialog(false)}
          />
        )}
      </>
    ),
    topRows: state.inserts.length,
    decorate: decorated ? decorate : undefined,
    overlay: overlay || null,
    onCellDoubleClick: editable ? (cell) => void editCell(cell) : undefined,
    onScroll,
    onKeyDown,
  };
}
