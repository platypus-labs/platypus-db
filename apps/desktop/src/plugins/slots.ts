// Where what plugins add plugs into the app's own screens. The module that fills these loads only
// when a plugin has something to add: without any, the app pays for this file and nothing else.
import { type ComponentType, useEffect, useMemo, useSyncExternalStore } from "react";
import type { ColumnInfo, SavedConnection } from "../api";
import type { CellDecoration } from "../grid/DataGrid";
import type { Rows } from "../grid/RowCache";

/** What a result gives its grid's plugins. */
export interface GridPluginInput {
  /** The result's columns. */
  columns: ColumnInfo[];
  /** These and the user's computed columns, after which the plugins' go. */
  shown: ColumnInfo[];
  rows: Rows;
  /** Whether a result column's values are sensitive: a plugin reads them as dots, whether or not
   *  the screen masks them now. */
  sensitive: (column: ColumnInfo) => boolean;
  topRows: number;
  redraw: () => void;
}

/** A result's grid with its plugins. */
export interface GridPluginView {
  columns: ColumnInfo[];
  rows: Rows;
  decorate: ((row: number, col: number) => CellDecoration | undefined) | undefined;
  /** A cell as the grid shows it, for a plugin's command. */
  cell(row: number, col: number): { column: { name: string; type: string; masked: boolean }; value: string | null } | null;
  dispose(): void;
}

declare module "../grid/DataGrid" {
  interface GridSourceExtensions {
    pluginGrid?: GridPluginView | null;
    /** Whether a column is sensitive, whether or not the screen masks it: what a plugin may read. */
    sensitive?: (column: ColumnInfo) => boolean;
  }
}

interface PluginSlots {
  panels: ComponentType<{ connection: SavedConnection | null }> | null;
  grid: ((input: GridPluginInput) => GridPluginView | null) | null;
}

let slots: PluginSlots = { panels: null, grid: null };
const listeners = new Set<() => void>();

export function setPluginSlots(next: PluginSlots) {
  slots = next;
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The plugins' side panels, for the shell; `null` without any. */
export function usePluginPanels() {
  return useSyncExternalStore(subscribe, () => slots.panels);
}

/** A result's grid with its plugins, for as long as the result stays the same; `null` without
 *  any, and the grid then has nothing more to go through. */
export function useGridPlugins(input: GridPluginInput): GridPluginView | null {
  const attach = useSyncExternalStore(subscribe, () => slots.grid);
  const { columns, shown, rows, sensitive, topRows } = input;
  const view = useMemo(() => attach?.(input) ?? null, [attach, columns, shown, rows, sensitive, topRows]);
  useEffect(() => () => view?.dispose(), [view]);
  return view;
}
