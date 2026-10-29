import { useEffect, useState } from "react";
import { api, type DataDiff, type RowChange, type SavedConnection, type SchemaMeta, toCommandError } from "../api";
import { workbench } from "../workbench";
import { t } from "../i18n";
import { commentHeader, compareTargets } from "./script";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  table: string;
  onClose: () => void;
}

/** A value as the samples show it: NULL apart from the empty string. */
const cell = (v: string | null) => (v === null ? "NULL" : v);

/** The kind of a sampled difference, as seen from the source. */
const KINDS = {
  added: () => t("Only in source"),
  changed: () => t("Changed"),
  removed: () => t("Only in target"),
};

/** One sampled difference: its key and, for a changed row, each cell as `column: target → source`. */
function Sample({ row }: { row: RowChange }) {
  return (
    <li>
      <div className="lint-head">
        <span className="lint-kind">{KINDS[row.change]()}</span>
        <code>{row.key.map(cell).join(", ")}</code>
      </div>
      {row.cells.map((c) => (
        <div key={c.column} className="small">
          <code>
            {c.column}: {cell(c.target)} → {cell(c.source)}
          </code>
        </div>
      ))}
    </li>
  );
}

/** Compares a table with another one of the same kind of database, here or on another
 *  connection, matching rows by key, and opens on the target the script that makes it like
 *  this one: rows inserted, updated and, when asked, deleted. The script is reviewed and run
 *  in the SQL editor. */
export function DataCompareDialog({ connection, database, schema, table, onClose }: Props) {
  const { targets, initial } = compareTargets(connection);
  const [target, setTarget] = useState<SavedConnection>(initial);
  const [schemas, setSchemas] = useState<SchemaMeta[]>([]);
  const [targetSchema, setTargetSchema] = useState("");
  const [tables, setTables] = useState<string[]>([]);
  const [targetTable, setTargetTable] = useState("");
  const [filter, setFilter] = useState("");
  const [deleteMissing, setDeleteMissing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [diff, setDiff] = useState<DataDiff | null>(null);
  const sameConnection = target.id === connection.id;
  const targetDatabase = sameConnection ? database : null;

  useEffect(() => {
    // The schemas of a target left before they came must not replace the new target's.
    let current = true;
    setSchemas([]);
    // Nor its schema, table, result or error stay: Compare waits for a table of this target.
    setTargetSchema("");
    setTables([]);
    setTargetTable("");
    setError(null);
    setDiff(null);
    workbench()
      .sideSession(target, targetDatabase)
      .then(async (sid) => {
        if (sid === null || !current) return;
        const all = await api.listSchemas(sid);
        if (!current) return;
        setSchemas(all);
        setTargetSchema((all.find((s) => s.name === schema) ?? all.find((s) => s.current) ?? all[0])?.name ?? "");
      })
      .catch((e) => current && setError(toCommandError(e).message));
    return () => {
      current = false;
    };
  }, [target.id]);

  useEffect(() => {
    // The tables of a schema left before they came must not replace the new schema's.
    let current = true;
    setTables([]);
    setTargetTable("");
    setDiff(null);
    if (targetSchema === "") return;
    workbench()
      .sideSession(target, targetDatabase)
      .then(async (sid) => {
        if (sid === null || !current) return;
        const all = (await api.listObjects(sid, targetSchema))
          .filter((o) => o.kind === "table")
          .map((o) => o.name)
          // A table is not compared with itself.
          .filter((n) => !(sameConnection && targetSchema === schema && n === table));
        if (!current) return;
        setTables(all);
        setTargetTable(all.includes(table) ? table : "");
      })
      .catch((e) => current && setError(toCommandError(e).message));
    return () => {
      current = false;
    };
  }, [target.id, targetSchema]);

  async function compare() {
    setBusy(true);
    setError(null);
    setDiff(null);
    try {
      const source = await workbench().sideSession(connection, database);
      const to = await workbench().sideSession(target, targetDatabase);
      if (source === null || to === null) return;
      const options = { filter: filter.trim() === "" ? null : filter.trim(), delete_missing: deleteMissing };
      setDiff(await api.compareTableData(source, { schema, name: table }, to, { schema: targetSchema, name: targetTable }, options));
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  function openScript(result: DataDiff) {
    const head = commentHeader(
      t("Makes {target} like {source}", { target: `${target.name} · ${targetSchema}.${targetTable}`, source: `${connection.name} · ${schema}.${table}` }),
      result.warnings,
    );
    const sql = `${result.statements.join(";\n")};\n`;
    workbench().openTab("query", target.id, { sql: `${head}${sql}`, database: targetDatabase });
    onClose();
  }

  const total = diff === null ? 0 : diff.added + diff.changed + diff.removed;

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="modal schema-lint data-compare" onKeyDown={(e) => e.key === "Escape" && !busy && onClose()}>
        <h2>{t("Compare the data of {table} with…", { table })}</h2>
        <p className="muted small">
          {t("Rows are matched by the key of {table}. The script makes the target table like it: rows inserted, updated and, if asked, deleted. It opens in the SQL editor of the target, to review before running.", { table })}
        </p>
        <div className="row">
          <label className="field grow">
            <span>{t("Connection")}</span>
            <select value={target.id} disabled={busy} onChange={(e) => setTarget(targets.find((c) => c.id === e.target.value) ?? connection)}>
              {targets.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>{t("Schema")}</span>
            <select value={targetSchema} disabled={busy} onChange={(e) => setTargetSchema(e.target.value)}>
              {schemas.map((s) => (
                <option key={s.name}>{s.name}</option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>{t("Table")}</span>
            <select
              value={targetTable}
              disabled={busy}
              onChange={(e) => {
                setTargetTable(e.target.value);
                setDiff(null);
              }}
            >
              {targetTable === "" && tables.length > 0 && (
                <option value="" disabled>
                  {t("Choose a table")}
                </option>
              )}
              {tables.map((n) => (
                <option key={n}>{n}</option>
              ))}
            </select>
          </label>
        </div>
        <label className="field">
          <span>{t("Filter (WHERE)")}</span>
          <input
            value={filter}
            disabled={busy}
            spellCheck={false}
            onChange={(e) => {
              setFilter(e.target.value);
              setDiff(null);
            }}
          />
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={deleteMissing}
            disabled={busy}
            onChange={(e) => {
              setDeleteMissing(e.target.checked);
              setDiff(null);
            }}
          />
          <span>{t("Delete rows missing from the source")}</span>
        </label>
        {diff !== null && (
          <>
            <div className="small data-diff-counts">
              <span>{t("Same")}: {diff.same}</span>
              {" · "}
              <span>{t("Only in source → inserted")}: {diff.added}</span>
              {" · "}
              <span>{t("Changed → updated")}: {diff.changed}</span>
              {" · "}
              <span>{deleteMissing ? t("Only in target → deleted") : t("Only in target → kept")}: {diff.removed}</span>
            </div>
            {diff.warnings.map((w) => (
              <div key={w} className="notice small">
                {w}
              </div>
            ))}
            {total === 0 && <div className="notice small">{t("The tables have the same rows")}</div>}
            {diff.samples.length > 0 && (
              <ul className="lint-findings">
                {diff.samples.map((row, i) => (
                  <Sample key={i} row={row} />
                ))}
              </ul>
            )}
            {total > diff.samples.length && (
              <div className="muted small">{t("First {shown} of {total} differences", { shown: diff.samples.length, total })}</div>
            )}
          </>
        )}
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" disabled={busy} onClick={onClose}>
            {t("Close")}
          </button>
          {diff !== null && diff.statements.length > 0 && (
            <button type="button" disabled={busy} onClick={() => openScript(diff)}>
              {t("Open script")}
            </button>
          )}
          <button type="button" className="primary" disabled={busy || !targetTable} onClick={() => void compare()}>
            {busy ? t("Comparing…") : t("Compare")}
          </button>
        </div>
      </div>
    </div>
  );
}
