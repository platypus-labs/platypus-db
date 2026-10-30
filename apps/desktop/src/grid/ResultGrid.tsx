import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { api, type CellInput, type ColumnInfo, isCassandra, isNoSql } from "../api";
import { annotation, catalogEntry, catalogKey, useCatalogVersion } from "../catalog/catalog";
import { CalcPanel } from "../panels/CalcPanel";
import { CompareRows } from "../panels/CompareRows";
import { DocumentPanel } from "../panels/DocumentPanel";
import { GroupingPanel } from "../panels/GroupingPanel";
import { MapPanel } from "../panels/MapPanel";
import { MetadataPanel } from "../panels/MetadataPanel";
import { RecordView } from "../panels/RecordView";
import { ValuePanel, type ValueTarget } from "../panels/ValuePanel";
import { defineSetting, setSetting, useSetting } from "../settings";
import { MASK, SensitiveScope, useMask, useSensitive } from "../guards/mask";
import { PanelResizer, useStoredSize } from "../shell/PanelResizer";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { ColumnsDialog } from "./ColumnsDialog";
import { ComputedDialog } from "./ComputedDialog";
import { type ComputedColumn, compileAll, computedInfo, loadComputed, saveComputed } from "./computed";
import { FormattingDialog } from "./FormattingDialog";
import { plainNumber } from "./formats";
import { type FormatRule, type Ranges, formatCell, loadRules, parseNumber, resolveRules, saveRules, scaledColumns } from "./formatting";
import { type Cell, type CellDecoration, DataGrid, type GridApi, type GridSource } from "./DataGrid";
import { type TableEditTarget, useTableEditor } from "./edit/TableEditor";
import "./features/actions";
import "./features/columns";
import "./features/copy";
import "./features/filters";
import { useReferences } from "./features/references";
import { useGridSearch } from "./features/search";
import { useSelectionStats } from "./features/stats";
import { useColumnProfile } from "../analysis/ColumnProfile";
import { type ColumnLayout, EMPTY_LAYOUT, loadLayout, saveLayout } from "./layout";
import type { RowCache, Rows } from "./RowCache";

export const valuePanelSetting = defineSetting({
  key: "grid.value_panel",
  section: "Results",
  label: "Show the value panel next to results",
  description: "F7 shows or hides it from the grid.",
  default: false,
  control: { type: "boolean" },
});

/** Read-only grid features for actions, in `source.features`. */
export interface GridFeatures {
  /** Opens the search box (Ctrl+F). */
  find(): void;
  valuePanelOpen: boolean;
  toggleValuePanel(): void;
  openColumns(): void;
  /** Opens the computed columns. */
  openComputed(): void;
  recordView: boolean;
  toggleRecordView(): void;
  /** Opens the grouping panel on these result columns. */
  groupBy(cols: number[]): void;
  toggleMetadata(): void;
  toggleCalc(): void;
  toggleMap(): void;
  /** Shows two loaded rows side by side. */
  compareRows(rows: [number, number]): void;
  /** Opens the conditional formatting rules, a new one on `column`. */
  openFormatting(column: string | null): void;
}

declare module "./DataGrid" {
  interface GridSourceExtensions {
    features?: GridFeatures;
  }
}

type Decorate = (row: number, col: number) => CellDecoration | undefined;

/** Edits underneath, search highlights on top (their background wins). */
function combine(below: Decorate | undefined, above: Decorate | undefined): Decorate | undefined {
  if (!below || !above) return below ?? above;
  return (row, col) => {
    const top = above(row, col);
    if (!top) return below(row, col);
    const base = below(row, col);
    return base ? { ...base, background: top.background } : top;
  };
}

/** Formatting colors underneath the other decorations, which keep their own background. */
function underlay(colors: Decorate | undefined, above: Decorate | undefined): Decorate | undefined {
  if (!colors || !above) return colors ?? above;
  return (row, col) => {
    const top = above(row, col);
    const color = colors(row, col);
    if (!top || !color) return top ?? color;
    return { ...top, background: top.background ?? color.background };
  };
}

/** The values of `masked` columns drawn as dots, over every other decoration (an edit's text
 *  too); `value` is a cell's loaded text. */
function masking(below: Decorate | undefined, masked: boolean[], value: (row: number, col: number) => string | null | undefined): Decorate | undefined {
  if (!masked.some(Boolean)) return below;
  return (row, col) => {
    const base = below?.(row, col);
    if (!masked[col]) return base;
    const text = base?.text !== undefined ? base.text : value(row, col);
    return text ? { ...base, text: MASK } : base;
  };
}

/** The table whose columns' notes the header shows. */
export interface NotesScope {
  connectionId: string;
  database: string | null | undefined;
  schema: string;
  table: string;
}

interface Props {
  jobId: number;
  columns: ColumnInfo[];
  rowCount: number;
  cache: RowCache;
  version: number;
  sort?: { column: string; desc: boolean } | null;
  onHeaderClick?: (column: number) => void;
  /** Tooltip of the headers when they do not sort. */
  headerHint?: string;
  source?: GridSource | null;
  /** The table behind the result, to edit it and follow its foreign keys. */
  edit?: TableEditTarget | null;
  /** Set for a data viewer: the columns' local notes show in the header. */
  notes?: NotesScope | null;
  /** At the end of the bar above the grid: the result's SQL field. */
  toolbar?: ReactNode;
}

/** A result's grid with its features: editing, search, column layout, totals and panels. */
export function ResultGrid({ jobId, columns, rowCount, cache, version, sort, onHeaderClick, headerHint, source, edit, notes, toolbar }: Props) {
  const notesVersion = useCatalogVersion();
  const noteOf = (col: number) =>
    notes && columns[col]
      ? catalogEntry(notes.connectionId, catalogKey(notes.database, notes.schema, notes.table, columns[col].name))
      : undefined;
  const grid = useRef<GridApi>(null);
  const box = useRef<HTMLDivElement>(null);
  const editor = useTableEditor({ target: edit ?? null, jobId, columns, cache, version, grid, box, extra: toolbar });
  const search = useGridSearch(jobId, columns.length, editor.topRows, grid);
  const stats = useSelectionStats(jobId, grid);
  const profile = useColumnProfile(jobId, columns, rowCount);
  const valuePanelOpen = useSetting(valuePanelSetting);
  // MongoDB and Redis results show the focused row as a document tree in place of the value
  // panel, open from the start: nested fields and JSON values are only readable there.
  const documents = !!source && isNoSql(source.connection.driver) && !isCassandra(source.connection.driver);
  const [documentOpen, setDocumentOpen] = useState(true);
  const panelOpen = documents ? documentOpen : valuePanelOpen;
  const [focused, setFocused] = useState<Cell | null>(null);
  const [recordView, setRecordView] = useState(false);
  const [grouping, setGrouping] = useState<number[] | null>(null);
  const [columnsOpen, setColumnsOpen] = useState(false);
  const [metadataOpen, setMetadataOpen] = useState(false);
  const [calcOpen, setCalcOpen] = useState(false);
  const [mapOpen, setMapOpen] = useState(false);
  /** Bumped on every selection change while the calc or map panel is open. */
  const [selection, setSelection] = useState(0);
  const [compared, setCompared] = useState<[number, number] | null>(null);

  // Data viewer tables keep their column layout and formatting rules; other results start
  // from the query's.
  const layoutKey = source?.table ? `${source.connection.id}|${source.table}` : null;
  const [layout, setLayout] = useState<ColumnLayout>(() => (layoutKey ? loadLayout(layoutKey) : EMPTY_LAYOUT));
  useEffect(() => setLayout(layoutKey ? loadLayout(layoutKey) : EMPTY_LAYOUT), [layoutKey]);
  const changeLayout = (next: ColumnLayout) => {
    setLayout(next);
    if (layoutKey) saveLayout(layoutKey, next);
  };
  const formatting = useFormatting(layoutKey, jobId, columns, rowCount, editor.topRows, cache, editor.edit?.pending);
  const computed = useComputed(layoutKey, columns, cache, editor.topRows, editor.edit?.pending);
  // While the screen may be shared, the values of sensitive columns show as dots.
  const mask = useMask(notes);
  const sensitive = useSensitive(notes);
  const masked = useMemo(() => {
    const own = columns.map((c) => !!mask?.(c));
    // A computed column shows dots when it reads a masked one.
    return [...own, ...computed.uses.map((cols) => cols.some((c) => own[c]))];
  }, [columns, mask, computed.uses]);
  const topRows = editor.topRows;
  const decorate = useMemo(
    () =>
      masking(combine(underlay(formatting.decorate, editor.decorate), search.decorate), masked, (row, col) =>
        row < topRows ? "" : cache.get(row - topRows, col),
      ),
    [formatting.decorate, editor.decorate, search.decorate, masked, cache, topRows],
  );

  const dataRow = focused ? grid.current?.dataRow(focused.row) : null;
  const refs = useReferences({
    edit: edit ?? null,
    jobId,
    columns,
    row: dataRow ?? null,
    onError: (m) => workbench().showError(m),
  });

  const features: GridFeatures = {
    find: search.open,
    valuePanelOpen: panelOpen,
    toggleValuePanel: () => (documents ? setDocumentOpen(!documentOpen) : void setSetting(valuePanelSetting, !panelOpen)),
    openColumns: () => setColumnsOpen(true),
    openComputed: computed.open,
    recordView,
    toggleRecordView: () => {
      setRecordView(!recordView);
      if (recordView) grid.current?.focus();
    },
    groupBy: setGrouping,
    toggleMetadata: () => setMetadataOpen(!metadataOpen),
    toggleCalc: () => setCalcOpen(!calcOpen),
    toggleMap: () => setMapOpen(!mapOpen),
    compareRows: setCompared,
    openFormatting: formatting.open,
  };

  // The side panels' width, the same for all of them and kept for the next start.
  const [panelWidth, setPanelWidth] = useStoredSize("layout.side_panel_width", 340);
  const body = useRef<HTMLDivElement>(null);
  const sideOpen = panelOpen || metadataOpen || calcOpen || mapOpen || grouping !== null || refs.panel != null;

  const column = focused ? columns[focused.col] : undefined;
  const valueTarget: ValueTarget | null =
    focused && dataRow != null && column
      ? {
          jobId,
          row: dataRow,
          col: focused.col,
          column,
          isNull: grid.current?.value(focused.row, focused.col) === null,
          plainTextJson: source?.connection.text_json === "plain",
        }
      : null;
  const total = rowCount + editor.topRows;

  return (
    // The panels beside the grid mask with the marks of the viewer's table too.
    <SensitiveScope.Provider value={notes ?? null}>
      <div className="result-grid">
        {editor.bar}
        <div className="result-grid-body" ref={body} style={{ "--side-panel-width": `${panelWidth}px` } as React.CSSProperties}>
          <div className="result-grid-main" ref={box} onKeyDown={editor.onKeyDown}>
            <DataGrid
              ref={grid}
              columns={computed.columns}
              resultColumns={columns.length}
              headerTitle={(col) =>
                [
                  notes ? annotation(noteOf(col)) : undefined,
                  columns[col] && sensitive(columns[col]) ? t("Sensitive column") : undefined,
                  headerHint,
                ]
                  .filter(Boolean)
                  .join("\n") || undefined
              }
              headerVersion={notesVersion}
              headerMarked={notes ? (col) => { const e = noteOf(col); return !!(e?.note || e?.tags.length); } : undefined}
              onHeaderHover={(hover) => profile.onHeaderHover(hover && hover.col < columns.length ? hover : null)}
              rowCount={rowCount}
              topRows={editor.topRows}
              cache={computed.rows}
              version={version}
              sort={sort}
              onHeaderClick={onHeaderClick && ((col) => col < columns.length && onHeaderClick(col))}
              source={source ? { ...source, edit: editor.edit, features, references: refs.references } : null}
              decorate={decorate}
              overlay={
                <>
                  {editor.overlay}
                  {profile.popover}
                </>
              }
              onCellDoubleClick={editor.onCellDoubleClick && ((cell) => cell.col < columns.length && editor.onCellDoubleClick!(cell))}
              onScroll={editor.onScroll}
              onFocusChange={setFocused}
              onSelectionChange={() => {
                stats.onSelectionChange();
                if (calcOpen || mapOpen) setSelection((n) => n + 1);
              }}
              layout={layout}
              onLayoutChange={changeLayout}
            />
            {search.box}
            {recordView && total > 0 && (
              <RecordView
                jobId={jobId}
                columns={columns}
                shown={grid.current?.visibleColumns() ?? columns.map((_, i) => i)}
                row={Math.min(focused?.row ?? 0, total - 1)}
                rowCount={total}
                topRows={editor.topRows}
                version={version}
                pending={editor.pending}
                onMove={(row) => grid.current?.select({ row, col: focused?.col ?? 0 })}
                onClose={features.toggleRecordView}
              />
            )}
          </div>
          {sideOpen && (
            <PanelResizer
              storeKey="layout.side_panel_width"
              size={panelWidth}
              fallback={340}
              min={200}
              max={() => (body.current?.clientWidth ?? window.innerWidth) - 240}
              direction="column"
              after
              onDrag={(w) => body.current?.style.setProperty("--side-panel-width", `${w}px`)}
              onResize={setPanelWidth}
              label={t("Resize the side panels")}
            />
          )}
          {panelOpen &&
            (documents ? (
              <DocumentPanel
                jobId={jobId}
                columns={columns}
                row={dataRow ?? null}
                field={column?.name ?? null}
                version={version}
                onClose={features.toggleValuePanel}
              />
            ) : (
              <ValuePanel target={valueTarget} onClose={features.toggleValuePanel} />
            ))}
          {metadataOpen && (
            <MetadataPanel
              column={column}
              table={editor.catalog?.table ?? null}
              meta={focused ? editor.catalog?.columns[focused.col] : undefined}
              inKey={!!focused && !!editor.catalog?.key.includes(focused.col)}
              onClose={features.toggleMetadata}
            />
          )}
          {calcOpen && (
            <CalcPanel jobId={jobId} columns={columns} grid={grid} selection={selection} onClose={features.toggleCalc} />
          )}
          {mapOpen && (
            <MapPanel
              jobId={jobId}
              columns={columns}
              grid={grid}
              selection={selection}
              rows={rowCount}
              version={version}
              focusedRow={dataRow ?? null}
              onPick={(row, col) => grid.current?.select({ row: row + editor.topRows, col })}
              onClose={features.toggleMap}
            />
          )}
          {grouping && (
            <GroupingPanel
              jobId={jobId}
              cols={grouping}
              columns={columns}
              rows={rowCount}
              onClose={() => setGrouping(null)}
            />
          )}
          {refs.panel}
        </div>
        {stats.summary}
        {editor.dialog}
        {compared && <CompareRows jobId={jobId} columns={columns} rows={compared} onClose={() => setCompared(null)} />}
        {formatting.dialog}
        {computed.dialog}
        {columnsOpen && (
          <ColumnsDialog columns={computed.columns} layout={layout} onChange={changeLayout} onClose={() => setColumnsOpen(false)} />
        )}
      </div>
    </SensitiveScope.Provider>
  );
}

/** The computed columns of a result (see `computed.ts`), kept for a table: the grid shows
 *  them after the result's columns, with their values worked out from the rows loaded and the
 *  edits not saved yet. Numbers are read as the grid writes them (digit grouping). */
function useComputed(
  key: string | null,
  columns: ColumnInfo[],
  cache: RowCache,
  topRows: number,
  pending: ((row: number, col: number) => CellInput | undefined) | undefined,
) {
  // Read while painting: a ref, so that typing an edit does not rebuild the rows.
  const pendingRef = useRef(pending);
  pendingRef.current = pending;
  const [list, setList] = useState<ComputedColumn[]>(() => (key ? loadComputed(key) : []));
  useEffect(() => setList(key ? loadComputed(key) : []), [key]);
  const [open, setOpen] = useState(false);
  // Those that no longer compile (a column gone from the query) are left out of the grid.
  const usable = useMemo(
    () =>
      compileAll(list, columns).flatMap((c) =>
        c.evaluate ? [{ info: computedInfo(c.column), evaluate: c.evaluate, uses: c.evaluate.uses }] : [],
      ),
    [list, columns],
  );
  const all = useMemo(() => (usable.length ? [...columns, ...usable.map((c) => c.info)] : columns), [columns, usable]);
  const rows: Rows = useMemo(() => {
    if (!usable.length) return cache;
    const n = columns.length;
    const cell = (row: number, c: number) => {
      const edit = pendingRef.current?.(row + topRows, c);
      // A DEFAULT is not known until saved.
      const text = edit ? (edit.kind === "value" ? edit.text : edit.kind === "null" ? null : undefined) : cache.get(row, c);
      return text && columns[c]!.numeric ? plainNumber(text) : text;
    };
    return {
      get: (row, col) => (col < n ? cache.get(row, col) : usable[col - n]?.evaluate((c) => cell(row, c))),
      ensure: (first, last, available) => cache.ensure(first, last, available),
    };
  }, [cache, columns, usable, topRows]);

  const change = (next: ComputedColumn[]) => {
    setList(next);
    if (key) saveComputed(key, next);
  };

  return {
    columns: all,
    rows,
    /** The result columns each computed one reads. */
    uses: useMemo(() => usable.map((c) => c.uses), [usable]),
    open: () => setOpen(true),
    dialog: open && (
      <ComputedDialog columns={columns} computed={list} saved={key !== null} onChange={change} onClose={() => setOpen(false)} />
    ),
  };
}

/** How long the rows loaded stay the same before the color scales span them again: a result
 *  still fetching would otherwise profile its columns on every batch. */
const RANGES_DELAY_MS = 400;

/** The conditional formatting of a result: its rules, kept for a table, and the cells they
 *  color, after the edits not saved yet. Color scales span the numbers of the rows loaded. */
function useFormatting(
  key: string | null,
  jobId: number,
  columns: ColumnInfo[],
  rowCount: number,
  topRows: number,
  cache: RowCache,
  pending: ((row: number, col: number) => CellInput | undefined) | undefined,
) {
  // Read while painting: a ref, so that typing an edit does not rebuild the decoration.
  const pendingRef = useRef(pending);
  pendingRef.current = pending;
  const [rules, setRules] = useState<FormatRule[]>(() => (key ? loadRules(key) : []));
  useEffect(() => setRules(key ? loadRules(key) : []), [key]);
  const [dialog, setDialog] = useState<{ column: string | null } | null>(null);
  const [ranges, setRanges] = useState<Ranges>(() => new Map());
  const resolved = useMemo(() => resolveRules(rules, columns, plainNumber), [rules, columns]);
  const scaled = scaledColumns(resolved).join(",");

  useEffect(() => {
    const cols = scaled ? scaled.split(",").map(Number) : [];
    if (cols.length === 0 || rowCount === 0) return setRanges((r) => (r.size === 0 ? r : new Map()));
    let live = true;
    const timer = setTimeout(() => {
      // Each column on its own: one that fails (a closed result) leaves the others colored.
      void Promise.allSettled(cols.map((c) => api.columnProfile(jobId, c))).then((profiles) => {
        if (!live) return;
        const next: Ranges = new Map();
        const bound = (text: string | null) => (text === null ? null : parseNumber(plainNumber(text)));
        profiles.forEach((p, i) => {
          if (p.status !== "fulfilled") return;
          const low = bound(p.value.min);
          const high = bound(p.value.max);
          if (low !== null && high !== null) next.set(cols[i]!, [low, high]);
        });
        setRanges(next);
      });
    }, RANGES_DELAY_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [jobId, scaled, rowCount]);

  const decorate = useMemo(() => {
    if (resolved.length === 0) return undefined;
    const value = (row: number, c: number) => {
      const edit = pendingRef.current?.(row, c);
      if (edit?.kind === "value") return edit.text;
      if (edit?.kind === "null") return null;
      // A new row's DEFAULT is not known yet.
      if (edit || row < topRows) return undefined;
      return cache.get(row - topRows, c);
    };
    return (row: number, col: number) => formatCell(resolved, col, (c) => value(row, c), ranges);
  }, [resolved, ranges, topRows, cache]);

  const change = (next: FormatRule[]) => {
    setRules(next);
    if (key) saveRules(key, next);
  };

  return {
    decorate,
    open: (column: string | null) => setDialog({ column }),
    dialog: dialog && (
      <FormattingDialog
        columns={columns}
        rules={rules}
        column={dialog.column}
        saved={key !== null}
        onChange={change}
        onClose={() => setDialog(null)}
      />
    ),
  };
}
