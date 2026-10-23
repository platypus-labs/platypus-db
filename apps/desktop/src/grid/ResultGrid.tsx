import { useMemo, useRef, useState } from "react";
import type { ColumnInfo } from "../api";
import { ValuePanel, type ValueTarget } from "../panels/ValuePanel";
import { defineSetting, setSetting, useSetting } from "../settings";
import { type Cell, type CellDecoration, DataGrid, type GridApi, type GridSource } from "./DataGrid";
import type { RowCache } from "./RowCache";
import { type TableEditTarget, useTableEditor } from "./edit/TableEditor";
import "./features/actions";
import "./features/copy";
import "./features/filters";
import { useGridSearch } from "./features/search";

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

interface Props {
  jobId: number;
  columns: ColumnInfo[];
  rowCount: number;
  cache: RowCache;
  version: number;
  sort?: { column: string; desc: boolean } | null;
  onHeaderClick?: (column: number) => void;
  source?: GridSource | null;
  /** Data viewer: the table the rows can be edited in. */
  edit?: TableEditTarget | null;
}

/** A result's grid with its features: editing, search, the value panel. */
export function ResultGrid({ jobId, columns, rowCount, cache, version, sort, onHeaderClick, source, edit }: Props) {
  const grid = useRef<GridApi>(null);
  const box = useRef<HTMLDivElement>(null);
  const editor = useTableEditor({ target: edit ?? null, jobId, columns, cache, version, grid, box });
  const search = useGridSearch(jobId, columns.length, editor.topRows, grid);
  const panelOpen = useSetting(valuePanelSetting);
  const [focused, setFocused] = useState<Cell | null>(null);
  const decorate = useMemo(() => combine(editor.decorate, search.decorate), [editor.decorate, search.decorate]);

  const features: GridFeatures = {
    find: search.open,
    valuePanelOpen: panelOpen,
    toggleValuePanel: () => {
      if (!panelOpen) setFocused(grid.current?.focused() ?? null);
      void setSetting(valuePanelSetting, !panelOpen);
    },
  };

  const dataRow = focused ? grid.current?.dataRow(focused.row) : null;
  const column = focused ? columns[focused.col] : undefined;
  const valueTarget: ValueTarget | null =
    focused && dataRow != null && column
      ? { jobId, row: dataRow, col: focused.col, column, isNull: grid.current?.value(focused.row, focused.col) === null }
      : null;

  return (
    <div className="result-grid">
      {editor.bar}
      <div className="result-grid-body">
        <div className="result-grid-main" ref={box} onKeyDown={editor.onKeyDown}>
          <DataGrid
            ref={grid}
            columns={columns}
            rowCount={rowCount}
            topRows={editor.topRows}
            cache={cache}
            version={version}
            sort={sort}
            onHeaderClick={onHeaderClick}
            source={source ? { ...source, edit: editor.edit, features } : null}
            decorate={decorate}
            overlay={editor.overlay}
            onCellDoubleClick={editor.onCellDoubleClick}
            onScroll={editor.onScroll}
            onFocusChange={panelOpen ? setFocused : undefined}
          />
          {search.box}
        </div>
        {panelOpen && <ValuePanel target={valueTarget} onClose={features.toggleValuePanel} />}
      </div>
      {editor.dialog}
    </div>
  );
}
