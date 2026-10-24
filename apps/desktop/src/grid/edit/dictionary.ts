/**
 * Dictionary data: a foreign key column shows the describing column of the row it points
 * to (e.g. a customer's name next to `customer_id`), and edits with a list of those rows.
 * Labels load in batches for the values the grid draws, on the navigator session.
 */
import { api, type ColumnMeta, type Lookup, type TableDetails, toCommandError } from "../../api";
import type { EditMeta } from "./meta";

/** Values looked up per request. */
const BATCH = 500;
/** Rows offered by the editor's list. */
export const OPTION_LIMIT = 1000;

/** Names that usually describe a row, best first. */
const LABEL_NAMES = ["name", "title", "label", "description", "descr", "code", "username", "email"];
const TEXT_TYPE = /char|text|string|enum|citext/i;

/** The column that best describes the rows of `details`, other than its key `key`. */
export function labelColumn(details: TableDetails, key: string): string | null {
  const candidates = details.columns.filter((c) => c.name !== key && !c.primary_key);
  const named = (c: ColumnMeta) => LABEL_NAMES.indexOf(c.name.toLowerCase());
  const byName = candidates
    .filter((c) => named(c) >= 0)
    .sort((a, b) => named(a) - named(b))[0];
  return (byName ?? candidates.find((c) => TEXT_TYPE.test(c.data_type)))?.name ?? null;
}

interface Column {
  lookup: Lookup;
  /** Labels by key; `null` when no row has that key. */
  labels: Map<string, string | null>;
  pending: Set<string>;
}

export class Dictionary {
  private columns = new Map<number, Column>();
  private flushing = false;
  private disposed = false;

  constructor(
    private readonly session: () => Promise<number | null>,
    /** Called when labels arrive, to redraw. */
    private readonly onLoad: () => void,
  ) {}

  /** Finds the dictionaries of `meta`'s single-column foreign keys. */
  async init(meta: EditMeta) {
    const sessionId = await this.session();
    if (sessionId === null) return;
    const byColumn = new Map(meta.columns.flatMap((c, i) => (c ? [[c.name, i] as const] : [])));
    await Promise.all(
      meta.details.foreign_keys.map(async (fk) => {
        const col = fk.columns.length === 1 ? byColumn.get(fk.columns[0]!) : undefined;
        const key = fk.ref_columns[0];
        if (col === undefined || key === undefined) return;
        try {
          const details = await api.tableDetails(sessionId, fk.ref_schema, fk.ref_table);
          const label = labelColumn(details, key);
          if (label === null || this.disposed) return;
          this.columns.set(col, {
            lookup: { schema: fk.ref_schema, table: fk.ref_table, key, label },
            labels: new Map(),
            pending: new Set(),
          });
        } catch (e) {
          // The table may not be readable; the column then works without labels.
          console.warn("dictionary", toCommandError(e).message);
        }
      }),
    );
    if (this.columns.size > 0) this.onLoad();
  }

  has(col: number): boolean {
    return this.columns.has(col);
  }

  get size(): number {
    return this.columns.size;
  }

  /** The label of `value` in column `col`: `undefined` while loading (and queues it). */
  label(col: number, value: string): string | null | undefined {
    const column = this.columns.get(col);
    if (!column) return null;
    const label = column.labels.get(value);
    if (label !== undefined) return label;
    if (!column.pending.has(value)) {
      column.pending.add(value);
      if (!this.flushing) {
        this.flushing = true;
        // After the frame that asked, so one request covers every visible cell.
        setTimeout(() => void this.flush(), 0);
      }
    }
    return undefined;
  }

  private async flush() {
    try {
      const sessionId = await this.session();
      for (const column of this.columns.values()) {
        if (sessionId === null) {
          for (const v of column.pending) column.labels.set(v, null);
          column.pending.clear();
        }
        while (column.pending.size > 0 && !this.disposed) {
          const values = [...column.pending].slice(0, BATCH);
          let rows: [string | null, string | null][] = [];
          try {
            rows = await api.lookupValues(sessionId!, column.lookup, values, BATCH);
          } catch (e) {
            console.warn("dictionary", toCommandError(e).message);
          }
          for (const v of values) {
            column.pending.delete(v);
            column.labels.set(v, null);
          }
          for (const [key, label] of rows) if (key !== null) column.labels.set(key, label);
        }
      }
    } finally {
      this.flushing = false;
    }
    if (this.disposed) return;
    this.onLoad();
    // Values the grid asked for while this batch was loading.
    if ([...this.columns.values()].some((c) => c.pending.size > 0)) {
      this.flushing = true;
      setTimeout(() => void this.flush(), 0);
    }
  }

  /** `[key, label]` rows for the editor's list, ordered by label. */
  async options(col: number): Promise<[string, string | null][]> {
    const column = this.columns.get(col);
    const sessionId = column ? await this.session() : null;
    if (!column || sessionId === null) return [];
    const rows = await api.lookupValues(sessionId, column.lookup, null, OPTION_LIMIT);
    const options: [string, string | null][] = [];
    for (const [key, label] of rows) {
      if (key === null) continue;
      column.labels.set(key, label);
      options.push([key, label]);
    }
    return options;
  }

  dispose() {
    this.disposed = true;
  }
}
