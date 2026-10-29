import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, type ChangelogFormat, type SavedConnection, type SchemaMeta, type TableScript, toCommandError } from "../api";
import { workbench } from "../workbench";
import { t } from "../i18n";
import { commentHeader, compareTargets } from "./script";
import { translateWarning } from "./warnings";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  onClose: () => void;
}

/** Where the script goes: the SQL editor of the target, or a migration file for a tool. */
type Output = "editor" | ChangelogFormat;

/** `yyyyMMddHHmmss` in UTC: the version of a Flyway migration and the id of a Liquibase one. */
const timestamp = () => new Date().toISOString().replace(/\.\d+Z$/, "").replace(/[-:T]/g, "");

/** Compares a schema with another one of the same kind of database, here or on another
 *  connection, and opens on the target the script that makes it like this one: tables
 *  created, altered and dropped. The script is reviewed and run in the SQL editor. */
export function CompareDialog({ connection, database, schema, onClose }: Props) {
  const { targets, initial } = compareTargets(connection);
  const [target, setTarget] = useState<SavedConnection>(initial);
  const [schemas, setSchemas] = useState<SchemaMeta[]>([]);
  const [targetSchema, setTargetSchema] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [same, setSame] = useState(false);
  const [output, setOutput] = useState<Output>("editor");
  const sameConnection = target.id === connection.id;
  const targetDatabase = sameConnection ? database : null;

  useEffect(() => {
    // The schemas of a target left before they came must not replace the new target's.
    let current = true;
    setSchemas([]);
    // Nor its schema or error stay: Compare waits for a schema of this target.
    setTargetSchema("");
    setError(null);
    setSame(false);
    workbench()
      .sideSession(target, targetDatabase)
      .then(async (sid) => {
        if (sid === null || !current) return;
        const all = (await api.listSchemas(sid)).filter((s) => !sameConnection || s.name !== schema);
        if (!current) return;
        setSchemas(all);
        // On the same connection no schema is taken for the user: the current one would be
        // made like this one, its other tables dropped.
        setTargetSchema(sameConnection ? "" : ((all.find((s) => s.name === schema) ?? all.find((s) => s.current) ?? all[0])?.name ?? ""));
      })
      .catch((e) => current && setError(toCommandError(e).message));
    return () => {
      current = false;
    };
  }, [target.id]);

  /** Asks where to save the migration file and writes it; `false` when the dialog is cancelled. */
  async function writeChangelog(script: TableScript, format: ChangelogFormat): Promise<boolean> {
    const stamp = timestamp();
    // The description is English whatever the language: it names a file other tools read.
    const description = `make ${targetSchema} like ${schema}`;
    const defaultPath = format === "flyway" ? await api.flywayFileName(stamp, description) : `changelog-${stamp}.sql`;
    const path = await save({ defaultPath, filters: [{ name: "SQL", extensions: ["sql"] }] });
    if (typeof path !== "string") return false;
    await api.writeTextFile(path, await api.schemaChangelog(connection.driver, script, format, description, "platypusdb", stamp));
    return true;
  }

  async function compare() {
    setBusy(true);
    setError(null);
    setSame(false);
    try {
      const source = await workbench().sideSession(connection, database);
      const to = await workbench().sideSession(target, targetDatabase);
      if (source === null || to === null) return;
      const { script, sql } = await api.compareSchemas(source, schema, to, targetSchema);
      if (script.statements.length === 0) return setSame(true);
      if (output !== "editor") {
        if (await writeChangelog(script, output)) onClose();
        return;
      }
      const head = commentHeader(
        t("Makes {target} like {source}", { target: `${target.name} · ${targetSchema}`, source: `${connection.name} · ${schema}` }),
        script.warnings.map(translateWarning),
      );
      workbench().openTab("query", target.id, { sql: `${head}${sql}`, database: targetDatabase });
      onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="modal schema-compare" onKeyDown={(e) => e.key === "Escape" && !busy && onClose()}>
        <h2>{t("Compare {schema} with…", { schema })}</h2>
        <p className="muted small">
          {t("The script makes the target schema like {schema}: tables created, altered and dropped. It opens in the SQL editor of the target, to review before running.", { schema })}
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
            <select
              value={targetSchema}
              disabled={busy}
              onChange={(e) => {
                setTargetSchema(e.target.value);
                setSame(false);
              }}
            >
              {targetSchema === "" && schemas.length > 0 && (
                <option value="" disabled>
                  {t("Choose a schema")}
                </option>
              )}
              {schemas.map((s) => (
                <option key={s.name}>{s.name}</option>
              ))}
            </select>
          </label>
        </div>
        <label className="field">
          <span>{t("Output")}</span>
          <select value={output} disabled={busy} onChange={(e) => setOutput(e.target.value as Output)}>
            <option value="editor">{t("SQL editor")}</option>
            <option value="flyway">{t("Flyway migration…")}</option>
            <option value="liquibase">{t("Liquibase changelog…")}</option>
          </select>
        </label>
        {same && <div className="notice small">{t("The schemas have the same tables: there is nothing to change.")}</div>}
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" disabled={busy} onClick={onClose}>
            {t("Close")}
          </button>
          <button type="button" className="primary" disabled={busy || !targetSchema} onClick={() => void compare()}>
            {busy ? t("Comparing…") : t("Compare")}
          </button>
        </div>
      </div>
    </div>
  );
}
