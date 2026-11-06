// A result's grid with what plugins add: computed columns and ways to draw a cell. Kept apart from
// the app around it, so that the backend comes in as an argument.
//
// The grid draws a frame by asking for each visible cell, so nothing here may call a module on
// that path. A cell is one map lookup: found, it is drawn; not found, it is noted and drawn as it
// is (or as loading), and a timer outside the frame sends what was noted, as one batch, one batch
// at a time per extension. A renderer's answers are kept by value, a column's by row, and scrolling
// back costs nothing. The newest cells noted go first: those are what is on screen now.
//
// A plugin reads a cell as the grid shows it: a masked one as dots, NULL as NULL. A column whose
// batch fails shows its error in its cells, and asks no more; a renderer's goes back to the plain
// value. Neither takes the grid with it.
import type { ColumnInfo, InstalledPlugin, PluginGridCell, PluginGridColumn, PluginGridRendered, PluginGridValue } from "../api";
import type { CellDecoration } from "../grid/DataGrid";
import type { Rows } from "../grid/RowCache";
import type { GridPluginInput, GridPluginView } from "./slots";

/** A batch at most: the backend refuses more than 1000. */
const BATCH = 200;
/** How long notes pile up before they are sent. */
const FLUSH_MS = 30;
/** The rows of a computed column, and the values of a renderer, kept per result. */
const MAX_CACHED = 20_000;
/** The longest value a plugin gets, as the backend's limit; a longer one is cut (a column) or
 *  drawn as it is (a renderer). */
export const MAX_VALUE = 16 << 10;

/** What a view needs of the app. */
export interface GridBackend {
  /** What a masked value shows. */
  mask: string;
  column(plugin: string, extension: string, columns: PluginGridColumn[], rows: (string | null)[][]): Promise<PluginGridValue[]>;
  render(plugin: string, extension: string, column: PluginGridColumn, values: string[]): Promise<PluginGridRendered[]>;
  /** A batch of this extension failed: at most once per plugin, the user hears of it. */
  failed(plugin: InstalledPlugin, title: string, reason: string): void;
  reason(error: unknown): string;
}

/** Whether a column of `dbType` is one of `types`; all are without any. A size is not the type. */
export function typeMatches(types: string[] | undefined, dbType: string): boolean {
  if (!types?.length) return true;
  const name = dbType.split("(")[0]!.trim().toLowerCase();
  return types.some((t) => t.toLowerCase() === name);
}

/** `title`, or with its plugin's name when a column of the grid already has that name: layouts go
 *  by name. */
function uniqueName(title: string, plugin: string, taken: Set<string>): string {
  let name = title;
  for (let n = 1; taken.has(name); n += 1) name = n === 1 ? `${title} (${plugin})` : `${title} (${plugin} ${n})`;
  taken.add(name);
  return name;
}

/** Notes of what to ask for, the latest first; ones past the batch are forgotten, as what is still
 *  on screen is noted again at the next frame. */
export class Notes<K> {
  private keys = new Set<K>();

  /** Called on the path of a frame: an insert in a set. */
  add(key: K) {
    this.keys.add(key);
  }

  get size() {
    return this.keys.size;
  }

  /** The latest `max` keys; the rest are dropped. */
  take(max: number): K[] {
    const all = [...this.keys];
    this.keys.clear();
    return all.slice(-max);
  }

  clear() {
    this.keys.clear();
  }
}

/** A map that forgets the oldest it was given past its size. */
function remember<K, V>(map: Map<K, V>, key: K, value: V) {
  map.set(key, value);
  if (map.size > MAX_CACHED) {
    const oldest = map.keys().next().value;
    if (oldest !== undefined) map.delete(oldest);
  }
}

const decoration = (cell: PluginGridCell): CellDecoration => ({
  ...(cell.text !== undefined && { text: cell.text }),
  ...(cell.color && { color: cell.color }),
  ...(cell.background && { background: cell.background }),
  ...(cell.hint && { hint: cell.hint }),
  ...(cell.strike && { strike: true }),
});

/** Sends what is noted, one batch at a time, away from the frame that noted it. */
class Sender<K> {
  readonly notes = new Notes<K>();
  private timer: ReturnType<typeof setTimeout> | undefined;
  private busy = false;
  private readonly send: (keys: K[]) => Promise<void>;

  constructor(send: (keys: K[]) => Promise<void>) {
    this.send = send;
  }

  note(key: K) {
    this.notes.add(key);
    if (this.timer === undefined && !this.busy) this.timer = setTimeout(this.flush, FLUSH_MS);
  }

  private flush = () => {
    this.timer = undefined;
    const keys = this.notes.take(BATCH);
    if (!keys.length) return;
    this.busy = true;
    void this.send(keys).finally(() => {
      this.busy = false;
      if (this.notes.size && this.timer === undefined) this.timer = setTimeout(this.flush, FLUSH_MS);
    });
  };

  stop() {
    clearTimeout(this.timer);
    this.timer = undefined;
    this.notes.clear();
  }
}

/** The grid of `input` with the extensions of `plugins`; `null` when none of them applies. */
export function createGridView(input: GridPluginInput, plugins: InstalledPlugin[], backend: GridBackend): GridPluginView | null {
  const { columns, shown, rows, masked, topRows, redraw } = input;
  const usable = plugins.filter((p) => p.enabled && p.approved.grid && p.manifest.grid);
  if (usable.length === 0) return null;
  const senders: { stop(): void }[] = [];
  const as = (c: number): PluginGridColumn => ({ name: columns[c]!.name, type: columns[c]!.db_type, masked: !!masked[c] });
  /** A value as the grid shows it. */
  const shownValue = (c: number, value: string | null) => (value !== null && masked[c] ? backend.mask : value);
  const matching = (types: string[] | undefined) => columns.flatMap((c, i) => (typeMatches(types, c.db_type) ? [i] : []));

  // Computed columns: by row.
  const taken = new Set(shown.map((c) => c.name));
  const computed: { info: ColumnInfo; get: (row: number) => string | null | undefined }[] = [];
  for (const plugin of usable) {
    for (const extension of plugin.manifest.grid!.columns ?? []) {
      const cols = matching(extension.types);
      if (cols.length === 0) continue;
      const values = new Map<number, string | null>();
      let failure: string | null = null;
      const sender = new Sender<number>(async (keys) => {
        // A row not loaded is noted again when it is drawn after it loads.
        const ready = keys.filter((row) => cols.every((c) => rows.get(row, c) !== undefined));
        if (!ready.length) return;
        const batch = ready.map((row) => cols.map((c) => shownValue(c, rows.get(row, c) as string | null)?.slice(0, MAX_VALUE) ?? null));
        try {
          const answer = await backend.column(plugin.manifest.id, extension.id, cols.map(as), batch);
          answer.forEach((a, i) => remember(values, ready[i]!, "error" in a ? `⚠ ${a.error}` : a.text));
        } catch (e) {
          failure = `⚠ ${backend.reason(e)}`;
          backend.failed(plugin, extension.title, backend.reason(e));
        }
        redraw();
      });
      senders.push(sender);
      computed.push({
        info: { name: uniqueName(extension.title, plugin.manifest.name, taken), db_type: plugin.manifest.name, numeric: false, binary: false },
        get: (row) => {
          if (failure) return failure;
          const hit = values.get(row);
          if (hit === undefined) sender.note(row);
          return hit;
        },
      });
    }
  }

  // Renderers: by value. The first plugin to claim a column draws it.
  const renderers: ((value: string) => CellDecoration | undefined)[] = [];
  for (const plugin of usable) {
    for (const extension of plugin.manifest.grid!.renderers ?? []) {
      for (const c of matching(extension.types)) {
        if (renderers[c]) continue;
        const cells = new Map<string, CellDecoration | null>();
        let failed = false;
        const sender = new Sender<string>(async (values) => {
          try {
            const answer = await backend.render(plugin.manifest.id, extension.id, as(c), values);
            answer.forEach((a, i) => {
              if ("error" in a) remember(cells, values[i]!, { hint: `⚠ ${a.error}` });
              else remember(cells, values[i]!, a.cell ? decoration(a.cell) : null);
            });
          } catch (e) {
            failed = true;
            backend.failed(plugin, extension.title, backend.reason(e));
          }
          redraw();
        });
        senders.push(sender);
        renderers[c] = (value) => {
          if (failed || value.length > MAX_VALUE) return undefined;
          const hit = cells.get(value);
          if (hit === undefined) sender.note(value);
          return hit ?? undefined;
        };
      }
    }
  }

  const first = shown.length;
  const allRows: Rows = computed.length
    ? {
        get: (row, col) => (col < first ? rows.get(row, col) : computed[col - first]?.get(row)),
        ensure: (from, to, available) => rows.ensure(from, to, available),
      }
    : rows;
  const decorate = renderers.length
    ? (row: number, col: number) => {
        const draw = renderers[col];
        if (!draw || row < topRows) return undefined;
        const value = rows.get(row - topRows, col);
        return value == null ? undefined : draw(masked[col] ? backend.mask : value);
      }
    : undefined;

  return {
    columns: computed.length ? [...shown, ...computed.map((c) => c.info)] : shown,
    rows: allRows,
    decorate,
    cell: (row, col) => {
      if (col >= columns.length || row < topRows) return null;
      const value = rows.get(row - topRows, col);
      return value === undefined ? null : { column: as(col), value: shownValue(col, value)?.slice(0, MAX_VALUE) ?? null };
    },
    dispose: () => senders.forEach((s) => s.stop()),
  };
}
