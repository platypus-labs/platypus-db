import { api, toCommandError } from "../api";
import { FailedBlocks } from "./fetchBackoff";

const BLOCK_ROWS = 500;
/** Blocks kept in memory (LRU); the backend holds the full result. */
const MAX_BLOCKS = 100;

interface Block {
  rows: number;
  columns: (string | null)[][];
}

/**
 * Client-side window over a query result. Rows are fetched in blocks as the grid scrolls;
 * a block fetched while the query was still running is refetched once more rows exist.
 * A block that fails to load is tried again later, with a growing wait, and the error shows.
 */
export class RowCache {
  private blocks = new Map<number, Block>();
  private pending = new Set<number>();
  private failed = new FailedBlocks();
  private retry: number | undefined;
  private disposed = false;
  /** Why the last fetch failed, while some block keeps failing. */
  error: string | null = null;

  constructor(
    private readonly jobId: number,
    private readonly onLoad: () => void,
  ) {}

  /** Blocks are being fetched. */
  get loading(): boolean {
    return this.pending.size > 0;
  }

  /** `undefined` = not loaded yet, `null` = SQL NULL. */
  get(row: number, col: number): string | null | undefined {
    const block = this.blocks.get(Math.floor(row / BLOCK_ROWS));
    if (!block) return undefined;
    const offset = row % BLOCK_ROWS;
    if (offset >= block.rows) return undefined;
    return block.columns[col]?.[offset];
  }

  /** Loads the blocks covering `first..=last`, given `available` rows on the backend. */
  ensure(first: number, last: number, available: number) {
    if (this.disposed || available === 0) return;
    const lastRow = Math.min(last, available - 1);
    const now = Date.now();
    for (let b = Math.floor(first / BLOCK_ROWS); b <= Math.floor(lastRow / BLOCK_ROWS); b++) {
      const block = this.blocks.get(b);
      const expected = Math.min(BLOCK_ROWS, available - b * BLOCK_ROWS);
      if (block) {
        // Refresh LRU position.
        this.blocks.delete(b);
        this.blocks.set(b, block);
        if (block.rows >= expected) continue;
      }
      if (!this.pending.has(b) && this.failed.ready(b, now)) void this.load(b);
    }
  }

  private async load(block: number) {
    this.pending.add(block);
    this.onLoad();
    try {
      const { rows, columns } = await api.fetchRows(this.jobId, block * BLOCK_ROWS, BLOCK_ROWS);
      if (this.disposed) return;
      this.failed.ok(block);
      if (!this.failed.any) this.error = null;
      if (rows > 0) {
        this.blocks.set(block, { rows, columns });
        while (this.blocks.size > MAX_BLOCKS) {
          const oldest = this.blocks.keys().next().value;
          if (oldest === undefined) break;
          this.blocks.delete(oldest);
        }
      }
    } catch (e) {
      if (this.disposed) return;
      this.error = toCommandError(e).message;
      const wait = this.failed.fail(block, Date.now());
      // A redraw then, which asks for the block again if it is still on screen.
      clearTimeout(this.retry);
      this.retry = window.setTimeout(() => !this.disposed && this.onLoad(), wait);
    } finally {
      this.pending.delete(block);
      if (!this.disposed) this.onLoad();
    }
  }

  dispose() {
    this.disposed = true;
    clearTimeout(this.retry);
    this.blocks.clear();
  }
}
