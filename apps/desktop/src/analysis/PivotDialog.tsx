/** The form of a pivot: the fields down, the field across, the aggregate and its value, totals
 *  and sort. It only chooses; `ResultSql.tsx` makes the query of the choice and runs it. */
import { useState } from "react";
import type { ColumnInfo } from "../api";
import { msg, t } from "../i18n";
import { type PivotAggregate, type PivotSettings, type PivotSort, aggregatesFor, normalize } from "./pivot";

const AGGREGATE_LABELS: Record<PivotAggregate, string> = {
  count: msg("Count"),
  count_distinct: msg("Count distinct"),
  sum: msg("Sum"),
  avg: msg("Average"),
  min: msg("Minimum"),
  max: msg("Maximum"),
};

const SORT_LABELS: Record<PivotSort, string> = {
  keys: msg("Row fields, ascending"),
  keys_desc: msg("Row fields, descending"),
  value_desc: msg("Largest value first"),
  value_asc: msg("Smallest value first"),
};

const FIRST: PivotSettings = { rows: [], column: null, value: null, aggregate: "count", totals: false, sort: "keys" };

/** `settings` with the aggregate the value allows, else count. */
function fitted(settings: PivotSettings, columns: readonly ColumnInfo[]): PivotSettings {
  const value = columns.find((c) => c.name === settings.value) ?? null;
  return aggregatesFor(value).includes(settings.aggregate) ? settings : { ...settings, aggregate: "count" };
}

export function PivotDialog({
  columns,
  initial,
  onRun,
  onClose,
}: {
  columns: ColumnInfo[];
  /** The settings of the last pivot of this statement, if still valid. */
  initial: PivotSettings | null;
  onRun: (settings: PivotSettings) => void;
  onClose: () => void;
}) {
  const [settings, setSettings] = useState<PivotSettings>(() => (initial && normalize(initial, columns)) || FIRST);
  const set = (patch: Partial<PivotSettings>) => setSettings((s) => fitted({ ...s, ...patch }, columns));
  const value = columns.find((c) => c.name === settings.value) ?? null;
  // A sort by value needs a column to sort by: no column field, or the column of totals.
  const byValue = settings.column === null || settings.totals;
  const valid = settings.rows.length > 0;
  const run = () => valid && onRun(settings);

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className="modal pivot-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={t("Pivot")}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
          else if (e.key === "Enter" && e.target instanceof HTMLElement && e.target.tagName !== "BUTTON") run();
        }}
      >
        <h2>{t("Pivot")}</h2>
        <p className="muted small">
          {t("Groups the loaded rows by the row fields, with the values of the column field across, and writes the query in the SQL field, where it can be edited.")}
        </p>
        <fieldset aria-label={t("Row fields")}>
          <legend>{t("Row fields")}</legend>
          {columns.map((c, i) =>
            c.name === settings.column ? null : (
              <label key={i} className="check">
                <input
                  type="checkbox"
                  checked={settings.rows.includes(c.name)}
                  onChange={(e) => set({ rows: e.target.checked ? [...settings.rows, c.name] : settings.rows.filter((n) => n !== c.name) })}
                />
                {c.name}
              </label>
            ),
          )}
        </fieldset>
        <label>
          {t("Column field")}
          <select
            aria-label={t("Column field")}
            value={settings.column ?? ""}
            // A field is down or across, not both.
            onChange={(e) => set({ column: e.target.value || null, rows: settings.rows.filter((n) => n !== e.target.value) })}
          >
            <option value="">{t("(none)")}</option>
            {columns.map((c, i) => (
              <option key={i} value={c.name}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("Value")}
          <select aria-label={t("Value")} value={settings.value ?? ""} onChange={(e) => set({ value: e.target.value || null })}>
            <option value="">{t("(count the rows)")}</option>
            {columns.map((c, i) => (
              <option key={i} value={c.name}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("Aggregate")}
          <select aria-label={t("Aggregate")} disabled={!settings.value} title={settings.value ? undefined : t("Counting the rows needs no aggregate")} value={settings.aggregate} onChange={(e) => set({ aggregate: e.target.value as PivotAggregate })}>
            {aggregatesFor(value).map((a) => (
              <option key={a} value={a}>
                {t(AGGREGATE_LABELS[a])}
              </option>
            ))}
          </select>
        </label>
        <label className="check">
          <input type="checkbox" checked={settings.totals} onChange={(e) => set({ totals: e.target.checked })} />
          {t("Totals")}
        </label>
        <label>
          {t("Sort")}
          <select aria-label={t("Sort")} value={byValue ? settings.sort : settings.sort.startsWith("value") ? "keys" : settings.sort} onChange={(e) => set({ sort: e.target.value as PivotSort })}>
            {(Object.keys(SORT_LABELS) as PivotSort[]).map((k) => (
              <option key={k} value={k} disabled={k.startsWith("value") && !byValue}>
                {t(SORT_LABELS[k])}
              </option>
            ))}
          </select>
        </label>
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="button" className="primary" disabled={!valid} onClick={run}>
            {t("Pivot")}
          </button>
        </div>
      </div>
    </div>
  );
}
