// Saving the files that cells hold: "Save PDF…" for one cell, "Save N files to a folder…" for a
// selection down one column. Whether a cell holds a file is told by the backend from its
// bytes, in any type of column; it is asked once, when the menu opens (`prepareCellFiles`), so
// the items are there from the first frame and do not move under the mouse.
import { save } from "@tauri-apps/plugin-dialog";
import { registerAction } from "../../actions";
import { api, type CellFile, type ColumnInfo, keepsDataIn } from "../../api";
import { msg, t } from "../../i18n";
import { pickFolder } from "../../projects/shared";
import { workbench } from "../../workbench";
import type { GridActionContext, GridApi } from "../DataGrid";
import { defaultFileName, lookupFiles } from "../fileType";

/** Rows written by each call to the backend, so that progress shows between them. */
const CHUNK = 25;

interface Prepared {
  jobId: number;
  col: number;
  /** The selected grid rows. */
  r0: number;
  r1: number;
  /** The result row of `r0`. */
  first: number;
  /** `null` when the selection is too long to look for files in. */
  files: (CellFile | null)[] | null;
  /** Whether a column's values show masked. */
  masked: (col: number) => boolean;
}

let prepared: Prepared | null = null;

/** Asks which selected cells hold a file, before the grid's menu opens. Nothing is offered for a
 *  connection that keeps its data in, a masked column, or a selection over several columns. */
export async function prepareCellFiles(grid: GridApi, jobId: number | null, masked: (col: number) => boolean, restricted: boolean): Promise<void> {
  prepared = null;
  const sel = grid.selection();
  if (restricted || jobId === null || !sel || sel.cols.length !== 1) return;
  const col = sel.cols[0]!;
  const last = grid.dataRow(sel.r1);
  if (masked(col) || last === null) return;
  const first = grid.dataRow(sel.r0) ?? 0;
  // Too long a selection is not read: the menu says why it offers no folder.
  const files = lookupFiles(last - first + 1) ? await api.columnFiles(jobId, col, first, last - first + 1) : null;
  prepared = { jobId, col, r0: sel.r0, r1: sel.r1, first, files, masked };
}

/** What the menu was prepared for, if it is still the selection. */
function current({ grid, source }: GridActionContext): Prepared | null {
  const sel = grid.selection();
  if (!prepared || !sel || !source || source.jobId !== prepared.jobId || keepsDataIn(source.connection)) return null;
  return sel.r0 === prepared.r0 && sel.r1 === prepared.r1 && sel.cols.length === 1 && sel.cols[0] === prepared.col ? prepared : null;
}

/** The file of the one selected cell. */
function single(ctx: GridActionContext): CellFile | null {
  const p = current(ctx);
  return p && p.r0 === p.r1 ? (p.files?.[0] ?? null) : null;
}

/** The number of files in a selection of several cells. */
function many(ctx: GridActionContext): number {
  const p = current(ctx);
  return p && p.r1 > p.r0 ? (p.files?.filter(Boolean).length ?? 0) : 0;
}

/** Whether the selection is one column of several rows, too long to look for files in. */
function tooLong(ctx: GridActionContext): boolean {
  const p = current(ctx);
  return p !== null && p.r1 > p.r0 && p.files === null;
}

/** The result column that names the files: `id`, else the first one ending in `_id`. */
export function keyColumn(columns: Pick<ColumnInfo, "name">[], except: number): number | null {
  const at = (test: (name: string) => boolean) => columns.findIndex((c, i) => i !== except && test(c.name));
  const found = [at((n) => /^id$/i.test(n)), at((n) => /[_ ]id$/i.test(n))].find((i) => i >= 0);
  return found ?? null;
}

registerAction({
  id: "grid.save_file",
  scope: "grid",
  title: "Save {type}…",
  titleParams: (ctx) => ({ type: single(ctx)?.name ?? "" }),
  group: "copy",
  order: 18,
  when: (ctx) => single(ctx) !== null,
  run: async (ctx) => {
    const file = single(ctx)!;
    const { jobId, col, first } = current(ctx)!;
    const name = ctx.grid.columns()[col]!;
    const path = await save({ defaultPath: defaultFileName(name.name, file, true) });
    if (!path) return;
    await api.saveCell(jobId, first, col, path, file.coded);
    workbench().notify(t("Saved {path}", { path }));
  },
});

registerAction({
  id: "grid.save_files",
  scope: "grid",
  title: "Save {count} files to a folder…",
  titleParams: (ctx) => ({ count: many(ctx) }),
  group: "copy",
  order: 18.5,
  when: (ctx) => many(ctx) >= 2,
  run: async (ctx) => {
    const { jobId, col, first, r1 } = current(ctx)!;
    const folder = await pickFolder(t("Choose the folder for the files"));
    if (!folder) return;
    const total = ctx.grid.dataRow(r1)! - first + 1;
    // File names show in the folder: a masked column does not name them.
    const key = keyColumn(ctx.grid.columns(), col);
    const nameColumn = key !== null && current(ctx)?.masked(key) ? null : key;
    let written = 0;
    let failed = 0;
    let firstError = "";
    for (let done = 0; done < total; done += CHUNK) {
      workbench().notify(t("Saving files… {done} of {total} rows", { done, total }));
      const saved = await api.saveCellFiles(jobId, col, first + done, Math.min(CHUNK, total - done), folder, nameColumn);
      written += saved.written;
      failed += saved.failed;
      firstError ||= saved.errors[0] ?? "";
    }
    if (failed > 0) {
      workbench().showError(t("Saved {written} files to {folder}; {failed} failed: {error}", { written, folder, failed, error: firstError }));
    } else {
      workbench().notify(t("Saved {written} files to {folder}", { written, folder }));
    }
  },
});

// Shown greyed out with the reason, so that a long selection does not just lose the item.
registerAction({
  id: "grid.save_files_limit",
  scope: "grid",
  title: "Save files to a folder…",
  group: "copy",
  order: 18.5,
  when: (ctx) => tooLong(ctx),
  disabled: () => msg("Files are only looked for in a selection of up to 20,000 rows"),
  run: () => {},
});
