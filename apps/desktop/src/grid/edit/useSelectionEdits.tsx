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
  const [shown, setShown] = useState<{ kind: MassEditKind; cells: SelectedCell[] } | null>(null);

  /** The selected cells with what they hold now; the loaded values in one request. */
  async function selected(): Promise<SelectedCell[]> {
    const sel = grid.current?.selection();
    if (!sel) return [];
    const n = current.current.inserts.length;
    const first = Math.max(sel.r0, n) - n;
    const count = sel.r1 - n - first + 1;
    const loaded = count > 0 ? await api.rowValues(jobId, first, count) : [];
    return selectionCells(current.current, sel, loaded, first);
  }

  function applyOutcome(outcome: Outcome) {
    if (outcome.changes.length > 0) apply((s) => edits.setCells(s, outcome.changes));
    const report = rejectedReport(outcome.rejected, names);
    if (report) setError(report);
  }

  async function massEdit(kind: MassEditKind) {
    try {
      setShown({ kind, cells: await selected() });
    } catch (e) {
      fail(e);
    }
  }

  async function fill() {
    try {
      applyOutcome(fillDown(await selected(), rules));
    } catch (e) {
      fail(e);
    }
  }

  /** The file's contents, or `null` when the user gave up or it cannot be read. */
  async function readFile(col: number, binary: boolean): Promise<string | null> {
    const path = await answerFor("file", () => open({ multiple: false, directory: false, title: t("Load {column} from a file", { column: names[col] ?? "" }) }));
    if (typeof path !== "string") return null;
    const size = await api.fileSize(path);
    if (size > LARGE_FILE) {
      const mb = Math.round(size / (1024 * 1024)).toLocaleString();
      const message = t("The file is {size} MB: loading it takes memory, and saving sends all of it to the database. Load it?", { size: mb });
      if (!(await ask(message, { confirm: t("Load") }))) return null;
    }
    try {
      return await api.readCellFile(path, binary);
    } catch (e) {
      if (toCommandError(e).kind !== "invalid_utf8") throw e;
      setError(t("The file is not UTF-8 text: it cannot be loaded into a text column."));
      return null;
    }
  }

  /** Loads a cell from a file: bytes into a binary column, UTF-8 text into a text one. */
  async function loadFile(cell: Cell) {
    const rule = rules(cell.col);
    if (!rule || !loadsFromFile(rule)) return;
    try {
      const text = await readFile(cell.col, rule.kind === "binary");
      if (text === null) return;
      const value = { kind: "value", text } as const;
      const problem = checkValue(rule, value);
      if (problem) return setError(rejectedReport([{ col: cell.col, problem, cells: 1 }], names));
      const s = current.current;
      const ref = edits.rowRef(s, cell.row);
      if ("row" in ref && s.deletes.has(ref.row)) return;
      const original = "row" in ref ? (await api.rowValues(jobId, ref.row, 1))[0]?.[cell.col] : undefined;
      apply((latest) => edits.setCells(latest, [{ gridRow: cell.row, col: cell.col, value, original }]));
      grid.current?.focus();
    } catch (e) {
      fail(e);
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
        applyOutcome(outcome);
        grid.current?.focus();
      }}
      onClose={() => setShown(null)}
    />
  );

  return { massEdit, fillDown: fill, loadFile, dialog };
}
