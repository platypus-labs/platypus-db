// The data editor's edits beyond one typed cell: mass edits of the selection and a cell
// loaded from a file. Both end as pending changes through `apply`, like typed values: shown
// highlighted, in the SQL preview, saved or undone with the rest.
import { open } from "@tauri-apps/plugin-dialog";
import { type MutableRefObject, type RefObject, useState } from "react";
import { api, toCommandError } from "../../api";
import { t } from "../../i18n";
import { answerFor } from "../../testAnswers";
import { ask } from "../../ui/ask";
import type { Cell, GridApi } from "../DataGrid";
import { MASS_EDIT_DIALOGS, type MassEditKind, rejectedReport } from "./MassEditDialog";
import { type Outcome, type Rules, type SelectedCell, checkValue, fillDown, loadsFromFile, selectionCells } from "./massEdit";
import * as edits from "./state";

/** Files above this size are loaded only once the user confirms. */
const LARGE_FILE = 50 * 1024 * 1024;
/** Files above this size are not loaded (`MAX_CELL_FILE` in `commands/grid.rs`). */
const MAX_FILE = 200 * 1024 * 1024;

const megabytes = (size: number) => Math.round(size / (1024 * 1024)).toLocaleString();

/** How loading a cell from a file ended: `cancelled` when the user gave up before anything
 *  was read, `failed` when it was reported in the edit bar. */
export type FileLoad = "loaded" | "cancelled" | "failed";

/** Selected cells, with the pending changes they were read from: their grid rows are those
 *  of `base`. */
interface Selected {
  cells: SelectedCell[];
  base: edits.EditState;
}

interface Options {
  grid: RefObject<GridApi | null>;
  jobId: number;
  /** The pending changes as they are now. */
  current: MutableRefObject<edits.EditState>;
  rules: Rules;
  /** Result column names, by index. */
  names: readonly string[];
  apply: (change: (s: edits.EditState) => edits.EditState) => void;
  setError: (message: string | null) => void;
  fail: (e: unknown) => void;
}

export function useSelectionEdits({ grid, jobId, current, rules, names, apply, setError, fail }: Options) {
  const [shown, setShown] = useState<(Selected & { kind: MassEditKind }) | null>(null);

  /** The selected cells with what they hold now; the loaded values of the selected columns
   *  in one request. */
  async function selected(): Promise<Selected> {
    const base = current.current;
    const sel = grid.current?.selection();
    if (!sel) return { cells: [], base };
    const n = base.inserts.length;
    const first = Math.max(sel.r0, n) - n;
    const count = sel.r1 - n - first + 1;
    const loaded = count > 0 && sel.cols.length > 0 ? await api.rowValues(jobId, first, count, sel.cols) : [];
    return { cells: selectionCells(base, sel, loaded, first), base };
  }

  /** Applies changes worked out against `base` to the rows they are at now; `false` when
   *  some of those rows went away meanwhile, and their changes with them. */
  function applyChanges(base: edits.EditState, changes: edits.CellChange[]): boolean {
    let kept = changes.length;
    apply((latest) => {
      const moved = edits.rebase(base, latest, changes);
      kept = moved.length;
      return edits.setCells(latest, moved);
    });
    return kept === changes.length;
  }

  function applyOutcome(outcome: Outcome, base: edits.EditState) {
    const complete = outcome.changes.length === 0 || applyChanges(base, outcome.changes);
    const report = [
      rejectedReport(outcome.rejected, names),
      complete ? null : t("Some rows changed meanwhile: their cells were left as they are."),
    ].filter(Boolean);
    if (report.length) setError(report.join(" "));
  }

  async function massEdit(kind: MassEditKind) {
    try {
      setShown({ kind, ...(await selected()) });
    } catch (e) {
      fail(e);
    }
  }

  async function fill() {
    try {
      const { cells, base } = await selected();
      applyOutcome(fillDown(cells, rules), base);
    } catch (e) {
      fail(e);
    }
  }

  /** The file's contents; `cancelled` when the user gave up, `failed` when it cannot be
   *  loaded (reported). */
  async function readFile(col: number, binary: boolean): Promise<{ text: string } | Exclude<FileLoad, "loaded">> {
    const path = await answerFor("file", () => open({ multiple: false, directory: false, title: t("Load {column} from a file", { column: names[col] ?? "" }) }));
    if (typeof path !== "string") return "cancelled";
    const size = await api.fileSize(path);
    if (size > MAX_FILE) {
      setError(t("The file is {size} MB: a cell can load files up to {limit} MB.", { size: megabytes(size), limit: megabytes(MAX_FILE) }));
      return "failed";
    }
    if (size > LARGE_FILE) {
      const message = t("The file is {size} MB: loading it takes memory, and saving sends all of it to the database. Load it?", { size: megabytes(size) });
      if (!(await ask(message, { confirm: t("Load") }))) return "cancelled";
    }
    try {
      return { text: await api.readCellFile(path, binary) };
    } catch (e) {
      if (toCommandError(e).kind !== "invalid_utf8") throw e;
      setError(t("The file is not UTF-8 text: it cannot be loaded into a text column."));
      return "failed";
    }
  }

  /** Loads a cell from a file: bytes into a binary column, UTF-8 text into a text one. The
   *  grid stays editable while the file is read, so the row is found again afterwards. */
  async function loadFile(cell: Cell): Promise<FileLoad> {
    const rule = rules(cell.col);
    if (!rule || !loadsFromFile(rule)) return "failed";
    const base = current.current;
    const ref = edits.rowRef(base, cell.row);
    if ("row" in ref && base.deletes.has(ref.row)) return "failed";
    try {
      const read = await readFile(cell.col, rule.kind === "binary");
      if (typeof read === "string") return read;
      const value = { kind: "value", text: read.text } as const;
      const problem = checkValue(rule, value);
      if (problem) {
        setError(rejectedReport([{ col: cell.col, problem, cells: 1 }], names));
        return "failed";
      }
      const original = "row" in ref ? (await api.rowValues(jobId, ref.row, 1, [cell.col]))[0]?.[0] : undefined;
      if (!applyChanges(base, [{ gridRow: cell.row, col: cell.col, value, original }])) {
        setError(t("The row changed while the file was read: the file was not loaded."));
        return "failed";
      }
      // Back to the grid, unless something else took the focus while the file was read:
      // a dialog opened meanwhile would no longer get its keys, Escape included.
      if (document.activeElement === null || document.activeElement === document.body) grid.current?.focus();
      return "loaded";
    } catch (e) {
      fail(e);
      return "failed";
    }
  }

  const Dialog = shown && MASS_EDIT_DIALOGS[shown.kind];
  const dialog = shown && Dialog && (
    <Dialog
      cells={shown.cells}
      rules={rules}
      names={names}
      onApply={(outcome) => {
        setShown(null);
        applyOutcome(outcome, shown.base);
        grid.current?.focus();
      }}
      onClose={() => setShown(null)}
    />
  );

  return { massEdit, fillDown: fill, loadFile, dialog };
}
