/** The differences between two runs of a statement: a summary, then the rows added (+),
 *  removed (−) and changed (~), the changed cells marked with their previous value. */
import type { Diff, DiffRow } from "../api";
import { fmt } from "../components/ResultPanel";
import { t } from "../i18n";
import type { TabBase, TabProps } from "../tabs/registry";
import { summary } from "./diffModel";

export interface DiffTabModel extends TabBase {
  kind: "diff";
  /** Not persisted: the rows compared are gone after a restart. */
  diff: Diff | null;
}

declare module "../tabs/registry" {
  interface TabKinds {
    diff: DiffTabModel;
  }
}

const SIGN: Record<DiffRow["change"], string> = { added: "+", removed: "−", changed: "~" };
/** Rows of a diff the backend sends at most. */
const MAX_ROWS = 10_000;

export function DiffTab({ tab, active }: TabProps<DiffTabModel>) {
  const diff = tab.diff;
  return (
    <div className="diff-tab" style={{ display: active ? "flex" : "none" }}>
      {diff && (
        <>
          <div className="diff-summary" role="status">
            <span className="diff-counts">{summary(diff)}</span>
            <span className="muted">
              {t(diff.keyed ? "Rows matched by the table's key" : "Rows matched by all their values: a changed row shows as removed and added.")}
            </span>
            {diff.truncated && <strong className="diff-truncated">{t("Only the first {n} differences are shown.", { n: fmt.format(MAX_ROWS) })}</strong>}
          </div>
          {!diff.complete && (
            <div className="notice warn diff-partial">
              {t(
                "Compared the first {before} and {after} loaded rows: rows found in only one run are counted only when the other run was loaded in full. Load all the rows of both runs for a complete comparison.",
                { before: fmt.format(diff.before_rows), after: fmt.format(diff.after_rows) },
              )}
            </div>
          )}
          {diff.rows.length === 0 ? (
            <div className="notice ok">{t(diff.complete ? "No differences." : "No differences in the rows compared.")}</div>
          ) : (
            <div className="diff-scroll">
              <table className="diff-table">
                <thead>
                  <tr>
                    <th aria-label={t("Change")} />
                    {diff.columns.map((c, i) => (
                      <th key={i}>{c}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {diff.rows.map((row, i) => (
                    <DiffLine key={i} row={row} />
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function DiffLine({ row }: { row: DiffRow }) {
  const before = new Map(row.before);
  return (
    <tr className={`diff-row ${row.change}`}>
      <td className="diff-sign">{SIGN[row.change]}</td>
      {row.values.map((value, c) => {
        const changed = before.has(c);
        const classes = [changed ? "diff-cell-changed" : "", value === null ? "null" : ""].filter(Boolean).join(" ");
        return (
          <td key={c} className={classes || undefined} title={changed ? t("Before: {value}", { value: before.get(c) ?? "NULL" }) : undefined}>
            {value ?? "NULL"}
          </td>
        );
      })}
    </tr>
  );
}
