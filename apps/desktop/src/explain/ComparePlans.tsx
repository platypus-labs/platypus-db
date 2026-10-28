import type { Explain, PlanNode } from "../api";
import { t } from "../i18n";
import { alignPlans, change, type Measure, measureOf, own, stepName, total } from "./compare";

/** A change as a signed percentage, green when it went down, red when up. */
function Change({ value }: { value: number | null }) {
  if (value === null) return null;
  const pct = Math.round(value * 1000) / 10;
  const kind = Math.abs(pct) < 0.5 ? "same" : pct < 0 ? "better" : "worse";
  return <span className={`explain-change ${kind}`}>{kind === "same" ? "=" : `${pct > 0 ? "+" : ""}${pct}%`}</span>;
}

const name = (n: PlanNode | null) => (n === null ? "" : stepName(n));

/** Two plans of a statement side by side, before and after (an index, a rewrite): the
 *  whole plan's time or cost, then each step's own, and how it changed. */
export function ComparePlans({
  before,
  after,
  beforeName,
  afterName,
  num,
}: {
  before: Explain;
  after: Explain;
  beforeName: string;
  afterName: string;
  num: (v: number | null) => string;
}) {
  const measure: Measure = measureOf(before, after);
  const unit = measure === "time" ? " ms" : "";
  const pairs = alignPlans(before.root, after.root);
  const figure = (v: number | null) => (v === null ? "" : `${num(v)}${unit}`);
  const whole = [total(before, measure), total(after, measure)] as const;
  return (
    <div className="explain-compare">
      <div className="explain-compare-summary">
        <div>
          <span className="muted small">{t(measure === "time" ? "Total time" : "Total cost")}</span>
          <strong>
            {figure(whole[0])} → {figure(whole[1])}
          </strong>
          <Change value={change(whole[0], whole[1])} />
        </div>
        <div>
          <span className="muted small">{t("Rows")}</span>
          <strong>
            {num(before.root.actual_rows ?? before.root.rows)} → {num(after.root.actual_rows ?? after.root.rows)}
          </strong>
        </div>
        <div>
          <span className="muted small">{t("Steps")}</span>
          <strong>
            {pairs.filter((p) => p.before).length} → {pairs.filter((p) => p.after).length}
          </strong>
        </div>
        {measure === "cost" && (before.analyzed || after.analyzed) && (
          <span className="muted small">{t("One plan did not run: they compare on the planner's cost.")}</span>
        )}
      </div>
      <table className="meta-table explain-compare-table">
        <thead>
          <tr>
            <th>{beforeName}</th>
            <th>{afterName}</th>
            <th className="num">{t(measure === "time" ? "Own time before" : "Own cost before")}</th>
            <th className="num">{t(measure === "time" ? "Own time after" : "Own cost after")}</th>
            <th className="num">{t("Change")}</th>
          </tr>
        </thead>
        <tbody>
          {pairs.map((p, i) => {
            const status = !p.before ? "added" : !p.after ? "removed" : name(p.before) === name(p.after) ? "" : "changed";
            return (
              <tr key={i} className={status}>
                <td style={{ paddingLeft: p.depth * 16 + 6 }}>{name(p.before)}</td>
                <td style={{ paddingLeft: p.depth * 16 + 6 }}>{name(p.after)}</td>
                <td className="num">{figure(own(p.before, measure))}</td>
                <td className="num">{figure(own(p.after, measure))}</td>
                <td className="num">
                  {status === "added" ? (
                    <span className="explain-change neutral">{t("new")}</span>
                  ) : status === "removed" ? (
                    <span className="explain-change neutral">{t("gone")}</span>
                  ) : (
                    <Change value={change(own(p.before, measure), own(p.after, measure))} />
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
