import { useEffect, useMemo, useRef, useState } from "react";
import type { ColumnInfo } from "../api";
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
import { workbench } from "../workbench";
import { ColumnsDialog } from "./ColumnsDialog";
import { type Cell, type CellDecoration, DataGrid, type GridApi, type GridSource } from "./DataGrid";
import { type TableEditTarget, useTableEditor } from "./edit/TableEditor";
import "./features/actions";
import "./features/columns";
import "./features/copy";
import "./features/filters";
import { useReferences } from "./features/references";
import { useGridSearch } from "./features/search";
import { useSelectionStats } from "./features/stats";
import { type ColumnLayout, EMPTY_LAYOUT, loadLayout, saveLayout } from "./layout";
import type { RowCache } from "./RowCache";

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
  recordView: boolean;
  toggleRecordView(): void;
  /** Opens the grouping panel on these result columns. */
  groupBy(cols: number[]): void;
  toggleMetadata(): void;
  toggleCalc(): void;
  toggleMap(): void;
  /** Shows two loaded rows side by side. */
  compareRows(rows: [number, number]): void;
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
  source?: GridSource | null;
  /** The table behind the result, to edit it and follow its foreign keys. */
  edit?: TableEditTarget | null;
  /** Set for a data viewer: the columns' local notes show in the header. */
  notes?: NotesScope | null;
}

/** A result's grid with its features: editing, search, column layout, totals and panels. */
export function ResultGrid({ jobId, columns, rowCount, cache, version, sort, onHeaderClick, source, edit, notes }: Props) {
  useCatalogVersion();
  const noteOf = (col: number) =>
    notes && columns[col]
      ? catalogEntry(notes.connectionId, catalogKey(notes.database, notes.schema, notes.table, columns[col].name))
      : undefined;
  const grid = useRef<GridApi>(null);
  const box = useRef<HTMLDivElement>(null);
  const editor = useTableEditor({ target: edit ?? null, jobId, columns, cache, version, grid, box });
  const search = useGridSearch(jobId, columns.length, editor.topRows, grid);
  const stats = useSelectionStats(jobId, grid);
  const valuePanelOpen = useSetting(valuePanelSetting);
  // MongoDB results show the focused row's document in place of the value panel, open
  // from the start: nested fields are only readable there.
  const mongo = source?.connection.driver === "mongo";
  const [documentOpen, setDocumentOpen] = useState(true);
  const panelOpen = mongo ? documentOpen : valuePanelOpen;
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
  const decorate = useMemo(() => combine(editor.decorate, search.decorate), [editor.decorate, search.decorate]);

  // Data viewer tables keep their column layout; other results start from the query's.
  const layoutKey = source?.table ? `${source.connection.id}|${source.table}` : null;
  const [layout, setLayout] = useState<ColumnLayout>(() => (layoutKey ? loadLayout(layoutKey) : EMPTY_LAYOUT));
  useEffect(() => setLayout(layoutKey ? loadLayout(layoutKey) : EMPTY_LAYOUT), [layoutKey]);
  const changeLayout = (next: ColumnLayout) => {
    setLayout(next);
    if (layoutKey) saveLayout(layoutKey, next);
  };

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
    toggleValuePanel: () => (mongo ? setDocumentOpen(!documentOpen) : void setSetting(valuePanelSetting, !panelOpen)),
    openColumns: () => setColumnsOpen(true),
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
  };

  const column = focused ? columns[focused.col] : undefined;
  const valueTarget: ValueTarget | null =
    focused && dataRow != null && column
      ? { jobId, row: dataRow, col: focused.col, column, isNull: grid.current?.value(focused.row, focused.col) === null }
      : null;
  const total = rowCount + editor.topRows;

  return (
    <div className="result-grid">
      {editor.bar}
      <div className="result-grid-body">
        <div className="result-grid-main" ref={box} onKeyDown={editor.onKeyDown}>
          <DataGrid
            ref={grid}
            columns={columns}
            headerTitle={notes ? (col) => annotation(noteOf(col)) : undefined}
            headerMarked={notes ? (col) => { const e = noteOf(col); return !!(e?.note || e?.tags.length); } : undefined}
            rowCount={rowCount}
            topRows={editor.topRows}
            cache={cache}
            version={version}
            sort={sort}
            onHeaderClick={onHeaderClick}
            source={source ? { ...source, edit: editor.edit, features, references: refs.references } : null}
            decorate={decorate}
            overlay={editor.overlay}
            onCellDoubleClick={editor.onCellDoubleClick}
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
              onMove={(row) => grid.current?.select({ row, col: focused?.col ?? 0 })}
              onClose={features.toggleRecordView}
            />
          )}
        </div>
        {panelOpen &&
          (mongo ? (
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
      {columnsOpen && (
        <ColumnsDialog columns={columns} layout={layout} onChange={changeLayout} onClose={() => setColumnsOpen(false)} />
      )}
    </div>
  );
}
