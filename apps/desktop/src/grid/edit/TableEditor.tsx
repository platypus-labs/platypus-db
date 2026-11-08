import { Icon } from "../../ui/Icon";
import { type ReactNode, type RefObject, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ADBC_DRIVERS,
  api,
  type CellInput,
  type ColumnInfo,
  type Generated,
  type GeneratedColumn,
  isCassandra,
  isMongo,
  isRedis,
  isSearch,
  type SavedConnection,
  toCommandError,
} from "../../api";
import type { Cell, CellDecoration, GridApi } from "../DataGrid";
import type { RowCache } from "../RowCache";
import { CellEditor, type Move } from "./CellEditor";
import { Dictionary, foreignKeyColumns } from "./dictionary";
import { displayColumn, useCatalogVersion } from "../../catalog/catalog";
import { DisplayColumnDialog } from "../../catalog/DisplayColumnDialog";
import { workbench } from "../../workbench";
import { type EditMeta, type EditSource, type EditorKind, editMeta, editorKind, isJson, type ReadOnly } from "./meta";
import { plainNumber } from "../formats";
import { isOwnCopy, mapFields, parseClipboard } from "./paste";
import { needsProductionConfirm } from "../../guards/guards";
import { History } from "./history";
import { PreviewDialog } from "./PreviewDialog";
import * as edits from "./state";
import { applyProposal, neededColumns, type ProposalColumn, type ProposedEdits } from "./proposed";
import { skippedNotice } from "./proposedNotice";
import { VirtualKeyDialog } from "./VirtualKeyDialog";
import { type ColumnRule, columnRule, loadsFromFile } from "./massEdit";
import type { MassEditKind } from "./MassEditDialog";
import { type FileLoad, useSelectionEdits } from "./useSelectionEdits";
import "./actions";
import { t, plural, msg } from "../../i18n";
import { THEME_EVENT } from "../../theme";

/** A result the editor may change: the data viewer's, or a query's of one table. */
export interface TableEditTarget {
  connection: SavedConnection;
  /** The database of the table, when not the connection's own. */
  database?: string | null;
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
  /** Rows an agent proposed: applied to the result as pending changes once, when its rows
   *  and the table's keys are loaded; `done` runs then, so that a result loaded again does
   *  not apply them twice. */
  proposed?: { edits: ProposedEdits; agent: string; done: () => void };
}

/** The data editor as grid actions see it, in `source.edit`. */
export interface TableEdit {
  editable: boolean;
  /** The table's columns are still being read: editing is neither possible nor refused yet. */
  loading: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  /** Opens the inline editor; `typed` replaces the value with the key that started it. */
  editCell(cell: Cell, typed?: string): Promise<void>;
  /** Sets every selected cell. */
  setSelection(value: CellInput): void;
  /** What result column `col` takes; `undefined` when it is not one of the table's. */
  rule(col: number): ColumnRule | undefined;
  /** Opens the dialog of a mass edit of the selection. */
  massEdit(kind: MassEditKind): Promise<void>;
  /** Copies the first selected row's values into the selected cells below it. */
  fillDown(): Promise<void>;
  /** Loads a cell from a file the user picks. */
  loadFile(cell: Cell): Promise<FileLoad>;
  addRow(): void;
  duplicateSelection(): Promise<void>;
  deleteSelection(): void;
  /** Pastes the clipboard (cells split by tabs and line breaks) over the cells from the focused one,
   *  or with `asRows` as new rows. */
  paste(asRows: boolean): Promise<void>;
  undo(): void;
  /** Takes back the last undo. */
  redo(): void;
  /** A cell's change not saved yet, by grid row. */
  pending(row: number, col: number): CellInput | undefined;
  /** Saves the changes; on a production connection the SQL is shown first, unless
   *  `confirmed` (the preview's Save). */
  save(confirmed?: boolean): Promise<void>;
  preview(): Promise<void>;
  discard(): void;
  /** The table a single-column foreign key in result column `col` points to; `null` otherwise. */
  foreignKey(col: number): { schema: string; table: string } | null;
  /** Lets the user pick the column that describes the rows of `col`'s referenced table. */
  chooseDisplayColumn(col: number): void;
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
  /** Rows the result holds, and whether the query has returned all of them. */
  rowCount: number;
  settled: boolean;
  grid: RefObject<GridApi | null>;
  /** The element holding the grid: bounds the editors and receives the typed keys. */
  box: RefObject<HTMLDivElement | null>;
  /** At the end of the bar, also when the result is not a table's: the result's SQL field. */
  extra?: ReactNode;
}

/** What the editor adds to a result grid. */
export interface TableEditorHost {
  /** Given to grid actions as `source.edit`; `undefined` when the result is not a table's. */
  edit: TableEdit | undefined;
  /** The table behind the result and its column per result column, once loaded. */
  catalog: Pick<EditMeta, "table" | "columns" | "key"> | null;
  /** The edit bar above the grid. */
  bar: ReactNode;
  /** Dialogs, e.g. the SQL preview. */
  dialog: ReactNode;
  /** New rows, shown above the result's. */
  topRows: number;
  decorate?: (row: number, col: number) => CellDecoration | undefined;
  /** A cell's change not saved yet, by grid row. */
  pending: (row: number, col: number) => CellInput | undefined;
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

interface Opening {
  /** What the editor starts with: the keys typed, or `undefined` for the cell's value. */
  typed?: string;
}

// Translucent, so they read on light and dark grids alike.
const CHANGED_FALLBACK = "rgba(177, 151, 252, 0.3)";
// The canvas cannot use a CSS variable, so the theme's value is read again when the theme changes.
const readChanged = () =>
  getComputedStyle(document.documentElement).getPropertyValue("--modified-bg").trim() || CHANGED_FALLBACK;
let CHANGED = CHANGED_FALLBACK;
window.addEventListener(THEME_EVENT, () => {
  CHANGED = readChanged();
});
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

/** Rows read per request when a proposal's keys are matched against the result. */
const PROPOSAL_CHUNK = 2000;
/** Rows of the result a proposal is matched against at most: the viewer reads only the rows
 *  its updates and deletes name. */
const PROPOSAL_ROWS = 50_000;

/** The loaded values of result columns `cols` (in that order) of the first rows. */
async function proposalCells(jobId: number, rowCount: number, cols: number[]): Promise<(string | null)[][]> {
  const rows: (string | null)[][] = [];
  const total = Math.min(rowCount, PROPOSAL_ROWS);
  for (let at = 0; at < total && cols.length > 0; at += PROPOSAL_CHUNK) {
    rows.push(...(await api.rowValues(jobId, at, Math.min(PROPOSAL_CHUNK, total - at), cols)));
  }
  return rows;
}

const columnsOf = (meta: EditMeta) => meta.columns.flatMap((c, i) => (c ? [i] : []));

/** The columns of the source's table the database fills by itself; none when they cannot be
 *  read, which leaves them editable as before. */
async function generatedColumns(target: TableEditTarget, source: EditSource): Promise<GeneratedColumn[]> {
  if (source.table.kind !== "table") return [];
  try {
    const session = await target.session();
    return session === null ? [] : await api.generatedColumns(session, source.table.schema, source.table.name);
  } catch {
    return [];
  }
}

/** Columns the database fills that the editor never writes: computed ones, and identities
 *  that take no value. */
const fixedByDatabase = (generated: Generated | undefined) => generated === "computed" || generated === "identity";

/** Editing of a result of one table: inline editors, new and deleted rows, save. */
export function useTableEditor({ target, jobId, columns, cache, version, rowCount, settled, grid, box, extra }: Options): TableEditorHost {
  const [meta, setMeta] = useState<EditMeta | ReadOnly | null>(null);
  const [state, setState] = useState(edits.EMPTY);
  const current = useRef(state);
  const [history] = useState(() => new History<edits.EditState>(UNDO_LIMIT));
  const [editing, setEditing] = useState<Editing | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** What the proposed rows that could not be applied were, until dismissed. */
  const [notice, setNotice] = useState<string | null>(null);
  const [preview, setPreview] = useState<string[] | null>(null);
  const [keyDialog, setKeyDialog] = useState(false);
  const [, setLayout] = useState(0);
  const dictionary = useRef<Dictionary | null>(null);
  const [labels, setLabels] = useState(0);
  const latest = useRef(target);
  latest.current = target;
  /** An editor waiting for its row's values, and the keys typed meanwhile. */
  const opening = useRef<Opening | null>(null);

  const reason = !target
    ? msg("not a table")
    : target.connection.read_only
      ? msg("the connection is read-only")
      : target.connection.restrictions?.no_data_editing || ADBC_DRIVERS.includes(target.connection.driver)
        ? msg("the connection changes its rows only by SQL")
        : // Saving checks each change's row count in a transaction: ClickHouse has neither.
          target.connection.driver === "clickhouse"
          ? msg("ClickHouse tables are not edited in place")
          : target.readOnly;
  const virtualKeys = target?.connection.virtual_keys ?? [];
  const virtualSignature = JSON.stringify(virtualKeys);
  useEffect(() => {
    const current = latest.current;
    if (reason !== null || !current) return;
    let live = true;
    current.load().then(
      async (source) => {
        if (!live) return;
        if (!source) return setMeta({ reason: msg("the rows are not one table's"), hidden: true });
        const generated = await generatedColumns(current, source);
        if (live) setMeta(editMeta(source, columns, virtualKeys, generated));
      },
      (e) => live && setMeta({ reason: t("its keys could not be read ({message})", { message: toCommandError(e).message }) }),
    );
    return () => {
      live = false;
    };
    // `virtualKeys` changes with its signature.
  }, [columns, reason, target?.key, virtualSignature]);
  const keys = meta && "key" in meta ? meta : null;
  const readOnly = reason ?? (meta && "reason" in meta ? meta.reason : null);
  const editable = readOnly === null && keys !== null && !busy;
  const loading = readOnly === null && keys === null;
  const dirty = !edits.isEmpty(state);

  useEffect(() => latest.current?.onDirtyChange(dirty), [dirty]);
  useEffect(() => () => latest.current?.onDirtyChange(false), []);

  // Foreign keys show the rows they point to, by the column the user picked for each table.
  useCatalogVersion();
  const connectionId = target?.connection.id;
  const database = target?.database;
  const preferred = useCallback(
    (schema: string, table: string) => (connectionId ? displayColumn(connectionId, database, schema, table) : undefined),
    [connectionId, database],
  );
  // Changes when a pick changes, not with every note saved in the catalog.
  const picks = keys ? foreignKeyColumns(keys).map(({ fk }) => preferred(fk.ref_schema, fk.ref_table) ?? "").join("\u0000") : "";
  useEffect(() => {
    const t = latest.current;
    if (!keys || !t) return;
    const dict = new Dictionary(t.session, () => setLabels((n) => n + 1), preferred);
    dictionary.current = dict;
    void dict.init(keys);
    return () => {
      dict.dispose();
      dictionary.current = null;
    };
  }, [keys, preferred, picks]);

  /** Makes `next` the pending changes; the selection follows its rows when new rows come or
   *  go on top (an explicit `select` afterwards, e.g. of the added row, still wins). */
  function setEdits(next: edits.EditState) {
    const before = current.current.inserts.length;
    current.current = next;
    setState(next);
    grid.current?.topRowsChanged(before, next.inserts.length);
  }

  function apply(change: (s: edits.EditState) => edits.EditState) {
    const prev = current.current;
    const next = change(prev);
    if (next === prev) return;
    history.record(prev);
    setEdits(next);
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
  const writable = (col: number) => keys?.columns[col] !== undefined && !fixedByDatabase(keys.generated[col]);

  /** What each result column takes, for mass edits and loading files. */
  const driver = target?.connection.driver;
  const sqlite = driver === "sqlite" || driver === "libsql";
  const rules = useMemo(
    () =>
      columns.map((c, i) => {
        const columnMeta = keys?.columns[i];
        return columnMeta && !fixedByDatabase(keys?.generated[i]) ? columnRule(c, columnMeta, sqlite) : undefined;
      }),
    [columns, keys, sqlite],
  );
  // Stable across renders: the mass edit dialogs work out their preview again when these change.
  const ruleOf = useCallback((col: number) => rules[col], [rules]);
  const names = useMemo(() => columns.map((c) => c.name), [columns]);
  const selectionEdits = useSelectionEdits({
    grid,
    jobId,
    current,
    rules: ruleOf,
    names,
    apply,
    setError,
    fail,
  });

  // Rows an agent proposed become pending changes, once: through `apply`, so they undo, show
  // in the row colors and the SQL preview, save and discard like the user's own.
  const proposedOnce = useRef(false);
  const proposal = target?.proposed;
  useEffect(() => {
    if (!proposal || proposedOnce.current) return;
    const finish = (message: string | null) => {
      proposedOnce.current = true;
      setNotice(message);
      proposal.done();
    };
    if (readOnly !== null) return finish(t("The proposed changes were not applied: {reason}.", { reason: t(readOnly) }));
    const { updates, deletes } = proposal.edits;
    // Matching a key needs the result's rows, all of them.
    if (!keys || busy || (updates.length + deletes.length > 0 && !settled)) return;
    proposedOnce.current = true;
    const known: ProposalColumn[] = columns.map((c, i) => ({
      name: c.name,
      rule: rules[i],
      fixed: keys.columns[i] ? "database" : "not_table",
    }));
    const wanted = neededColumns(proposal.edits, known);
    void proposalCells(jobId, rowCount, wanted).then(
      (rows) => {
        const at = new Map(wanted.map((col, i) => [col, i]));
        const result = applyProposal(current.current, proposal.edits, known, rows.length, (row, col) => {
          const i = at.get(col);
          return i === undefined ? undefined : rows[row]?.[i];
        });
        if (result.applied > 0) apply(() => result.state);
        finish(result.skipped.length ? skippedNotice(result.skipped) : null);
      },
      (e: unknown) => {
        fail(e);
        finish(null);
      },
    );
  }, [proposal, keys, readOnly, busy, settled, rowCount]);

  async function editCell(cell: Cell, typed?: string) {
    if (loading) return setError(t("The table's columns are still loading"));
    if (!editable || !keys) return;
    if (!writable(cell.col)) {
      // A computed column is the grid's, not the result's.
      const column = (columns[cell.col] ?? grid.current?.columns()[cell.col])?.name ?? "?";
      setError(
        keys.columns[cell.col]
          ? t("{column} is filled in by the database: it cannot be edited", { column })
          : t("{column} is not a column of {table}: it cannot be edited", { column, table: keys.table.name }),
      );
      return;
    }
    const s = current.current;
    const ref = edits.rowRef(s, cell.row);
    if ("row" in ref && s.deletes.has(ref.row)) return;
    let original: string | null | undefined;
    if ("row" in ref) {
      // Keys typed while the row loads go into this editor, not into new ones.
      const start: Opening = { typed };
      opening.current = start;
      try {
        original = (await api.rowValues(jobId, ref.row, 1))[0]?.[cell.col];
      } catch (e) {
        return fail(e);
      } finally {
        if (opening.current === start) opening.current = null;
      }
      typed = start.typed;
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
    // A computed column (after the result's) takes no paste.
    if (focused.col >= columns.length) return;
    const start = Math.max(0, shown.indexOf(focused.col));
    const ownCopy = isOwnCopy(text);
    const { targets, rows } = mapFields(
      parseClipboard(text),
      columns.map((c) => c.name),
      shown.slice(start),
    );
    const fields = (row: (string | null)[]) =>
      row.flatMap((value, i) => {
        const col = targets[i] ?? -1;
        return col >= 0 && writable(col) ? [{ col, value }] : [];
      });
    // NULL comes from the grid's own copy only; other text has no NULL.
    const input = (value: string | null): CellInput => (value === null ? { kind: "null" } : { kind: "value", text: value });
    if (asRows) {
      // Empty fields of other text take the column's default.
      const added = rows.map(
        (row) =>
          new Map<number, CellInput>(
            fields(row)
              .filter((f) => f.value !== "" || ownCopy)
              .map((f) => [f.col, input(f.value)]),
          ),
      );
      apply((s) => edits.addRows(s, added));
      grid.current?.select({ row: 0, col: focused.col }, { row: added.length - 1, col: focused.col });
      return;
    }
    // Over the cells from the focused one; empty fields of other text are NULL.
    const total = grid.current?.rowCount() ?? 0;
    const cells: edits.CellChange[] = rows.slice(0, Math.max(0, total - focused.row)).flatMap((row, r) =>
      fields(row).map(({ col, value }) => ({
        gridRow: focused.row + r,
        col,
        value: value === "" && !ownCopy ? { kind: "null" } : input(value),
      })),
    );
    apply((s) => edits.setCells(s, cells));
  }

  function changes(meta: EditMeta) {
    return edits.toChanges(current.current, {
      table: { schema: meta.table.schema, name: meta.table.name },
      key: meta.key,
      unique_key: meta.uniqueKey,
      table_columns: columnsOf(meta),
      defaults: meta.columns.map((c) => c?.default ?? null),
      generated: meta.generated.map((g) => g ?? null),
    });
  }

  const edit: TableEdit = {
    editable,
    loading,
    dirty,
    canUndo: history.canUndo,
    canRedo: history.canRedo,
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
    rule: ruleOf,
    massEdit: async (kind) => {
      if (editable) await selectionEdits.massEdit(kind);
    },
    fillDown: async () => {
      if (editable) await selectionEdits.fillDown();
    },
    loadFile: async (cell) => (editable ? selectionEdits.loadFile(cell) : "failed"),
    addRow: () => {
      if (!editable) return;
      apply((s) => edits.addRows(s, [new Map()]));
      grid.current?.select({ row: 0, col: grid.current.focused()?.col ?? 0 });
    },
    duplicateSelection: async () => {
      if (!editable || !keys) return;
      const s = current.current;
      // Numbered columns (serials, identities, auto-increments, rowids) and key columns with
      // a default get a fresh value; computed columns are left out (not writable).
      const generated = keys.columns.flatMap((meta, c) =>
        keys.generated[c] === "numbered" || (keys.key.includes(c) && meta?.default != null) ? [c] : [],
      );
      try {
        const rows = await Promise.all(
          selectedRows().map(async (gridRow): Promise<edits.Cells> => {
            const ref = edits.rowRef(s, gridRow);
            let cells: Map<number, CellInput>;
            if ("insert" in ref) {
              cells = new Map(s.inserts[ref.insert]!);
            } else {
              const [values = []] = await api.rowValues(jobId, ref.row, 1);
              cells = new Map<number, CellInput>(
                values.flatMap((v, c) =>
                  writable(c) ? [[c, v === null ? { kind: "null" } : { kind: "value", text: v }] as const] : [],
                ),
              );
            }
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
    pending: (row, col) => edits.cellEdit(current.current, row, col),
    undo: () => {
      const prev = history.undo(current.current);
      if (prev) setEdits(prev);
    },
    redo: () => {
      const next = history.redo(current.current);
      if (next) setEdits(next);
    },
    save: async (confirmed = false) => {
      if (!editable || !keys || edits.isEmpty(current.current)) return;
      if (!confirmed && target && needsProductionConfirm(target.connection)) return edit.preview();
      setBusy(true);
      setPreview(null);
      try {
        const saved = await api.saveChanges(jobId, changes(keys));
        history.clear();
        setEdits(edits.EMPTY);
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
    foreignKey: (col) => {
      const fk = keys && foreignKeyColumns(keys).find((f) => f.col === col)?.fk;
      return fk ? { schema: fk.ref_schema, table: fk.ref_table } : null;
    },
    chooseDisplayColumn: (col) => {
      const ref = edit.foreignKey(col);
      const editTarget = latest.current;
      if (!ref || !editTarget) return;
      editTarget.session().then((sessionId) => {
        if (sessionId === null) return;
        workbench().openDialog((close) => (
          <DisplayColumnDialog
            connectionId={editTarget.connection.id}
            database={editTarget.database}
            schema={ref.schema}
            table={ref.table}
            sessionId={sessionId}
            onClose={close}
          />
        ));
      }, fail);
    },
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
    const starting = opening.current;
    if (starting) {
      // The editor is on its way: the key joins what it starts with.
      e.preventDefault();
      starting.typed = (starting.typed ?? "") + e.key;
      return;
    }
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
      emptyIsNull={rules[editing.cell.col]?.emptyIsNull ?? column.numeric}
      options={editing.options}
      onLoadFile={
        loadsFromFile(rules[editing.cell.col])
          ? (text) => {
              const was = editing;
              setEditing(null);
              void edit.loadFile(was.cell).then((load) => {
                // No file picked: the editor comes back with what was typed in it.
                if (load === "cancelled" && text !== (was.typed ?? was.initial ?? "")) {
                  setEditing((e) => e ?? { ...was, typed: text });
                }
              });
            }
          : undefined
      }
      onCommit={commit}
      onCancel={cancelEdit}
    />
  );

  const noKey = meta && "reason" in meta ? meta.noKey : undefined;
  const showBar = !target?.quiet || (meta !== null && !("hidden" in meta && meta.hidden));
  const keyTable = keys?.table ?? noKey?.table;

  const bar = (target || extra) && (
    <div className="edit-bar" hidden={!showBar && !extra}>
      {!target || !showBar ? null : readOnly !== null ? (
        <>
          <span className="muted small">{t("Read-only: {reason}.", { reason: t(readOnly) })}</span>
          {noKey && (
            <button onClick={() => setKeyDialog(true)} title={t("Columns that tell the rows apart")}>
              {t("Set a virtual key…")}
            </button>
          )}
        </>
      ) : meta === null ? (
        <span className="muted small">{t("Checking the table's keys…")}</span>
      ) : (
        <>
          <button className="square" onClick={edit.addRow} disabled={!editable} title={t("Add a row (Insert)")} aria-label={t("Add a row")}>
            <Icon name="plus" />
          </button>
          <button
            className="square"
            onClick={edit.deleteSelection}
            disabled={!editable}
            title={t("Delete the selected rows (Delete)")}
            aria-label={t("Delete the selected rows")}
          >
            <Icon name="delete" />
          </button>
          <span className="divider" />
          {dirty ? (
            <>
              <span className="edit-count">
                {plural(edits.changedRows(state), "{n} row changed", "{n} rows changed")}
                {edits.kindsChanged(state) > 1 &&
                  ` (${[
                    state.updates.size > 0 && t("{n} modified", { n: state.updates.size }),
                    state.inserts.length > 0 && t("{n} new", { n: state.inserts.length }),
                    state.deletes.size > 0 && t("{n} deleted", { n: state.deletes.size }),
                  ]
                    .filter(Boolean)
                    .join(", ")})`}
              </span>
              <button className="primary" onClick={() => void edit.save()} disabled={!editable} title={t("Save (Ctrl+S)")}>
                {t(busy ? "Saving…" : "Save")}
              </button>
              <button className="square" onClick={() => void edit.preview()} disabled={busy} title={t("Preview SQL")} aria-label={t("Preview SQL")}>
                <Icon name="sql" />
              </button>
              <span className="divider" />
              <button className="square" onClick={edit.undo} disabled={busy || !edit.canUndo} title={t("Undo (Ctrl+Z)")} aria-label={t("Undo")}>
                <Icon name="rollback" />
              </button>
              <button className="square" onClick={edit.discard} disabled={busy} title={t("Discard the changes")} aria-label={t("Discard")}>
                <Icon name="discard" />
              </button>
            </>
          ) : (
            <span className="muted small">{t("Double-click, F2 or type to edit a cell")}</span>
          )}
          {keys?.virtualKey && (
            <button className="link small" onClick={() => setKeyDialog(true)} title={t("Change the virtual key")}>
              {t("Virtual key: {columns}", { columns: keys.virtualKey.join(", ") })}
            </button>
          )}
        </>
      )}
      {extra && <span className="divider" />}
      {extra}
      {proposal && !error && (
        <span className="small edit-waiting" role="status">
          {t("Applying the changes proposed by {agent}…", { agent: proposal.agent })}
        </span>
      )}
      {notice && (
        <span className="small edit-error edit-notice" title={notice} role="status">
          {notice}
          <button className="link" onClick={() => setNotice(null)} title={t("Dismiss")} aria-label={t("Dismiss")}>
            <Icon name="close" />
          </button>
        </span>
      )}
      {error && (
        <span className="error-text small edit-error" title={error}>
          {error}
          <button className="link" onClick={() => setError(null)} title={t("Dismiss")} aria-label={t("Dismiss")}>
            <Icon name="close" />
          </button>
        </span>
      )}
    </div>
  );

  return {
    edit: target ? edit : undefined,
    catalog: keys ?? (meta && "catalog" in meta && meta.catalog ? { ...meta.catalog, key: [] } : null),
    bar,
    dialog: (
      <>
        {preview && (
          <PreviewDialog
            statements={preview}
            busy={busy}
            production={!!target && needsProductionConfirm(target.connection)}
            commands={
              target && isRedis(target.connection.driver)
                ? "multi"
                : target && isSearch(target.connection.driver)
                  ? "requests"
                  : target && isCassandra(target.connection.driver)
                    ? "statements"
                    : target && isMongo(target.connection.driver)
                      ? "mongosh"
                      : undefined
            }
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
        {selectionEdits.dialog}
      </>
    ),
    topRows: state.inserts.length,
    decorate: decorated ? decorate : undefined,
    pending: (row, col) => edits.cellEdit(state, row, col),
    overlay: overlay || null,
    onCellDoubleClick: editable ? (cell) => void editCell(cell) : undefined,
    onScroll,
    onKeyDown,
  };
}
