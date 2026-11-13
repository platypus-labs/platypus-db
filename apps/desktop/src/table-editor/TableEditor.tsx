import { useEffect, useState } from "react";
import { api, type TableDesign, type TableScript, toCommandError } from "../api";
import { catalogKey, renameCatalog } from "../catalog/catalog";
import { tableCatalogMoves } from "../catalog/renames";
import { renameBookmarks } from "../navigator/prefs";
import { requestNavigatorRefresh } from "../navigatorEvents";
import type { TabBase, TabProps } from "../tabs/registry";
import { Icon, objectIcon } from "../ui/Icon";
import { workbench } from "../workbench";
import { t } from "../i18n";
import { translateWarning } from "../schema/warnings";
import { DesignForm, blankDesign, type TableEditorSection } from "./DesignForm";
import { LoadingState } from "../ui/State";

export type { TableEditorSection };

/** The graphical table editor: a new table, or an existing one altered. */
interface TableEditorTabModel extends TabBase {
  kind: "table-editor";
  schema: string;
  /** The table edited; `null` for a new one until it is saved. */
  table: string | null;
  section?: TableEditorSection;
}

declare module "../tabs/registry" {
  interface TabKinds {
    "table-editor": TableEditorTabModel;
  }
}

type Preview = { script: TableScript; sql: string } | { error: string };

export function TableEditor({ tab, connection, active, sideSession, catalog, onNeedCatalog, onChange }: TabProps<TableEditorTabModel>) {
  const [original, setOriginal] = useState<TableDesign | null>(null);
  const [design, setDesign] = useState<TableDesign | null>(tab.table === null ? blankDesign : null);
  const [error, setError] = useState<string | null>(null);
  // The script of the design it was made for: Save waits until it matches the design shown.
  const [preview, setPreview] = useState<{ of: TableDesign; preview: Preview } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [running, setRunning] = useState(false);
  const [version, setVersion] = useState(0);
  const section = tab.section ?? "columns";

  useEffect(() => {
    if (!catalog) onNeedCatalog(false);
  }, []);

  useEffect(() => {
    if (tab.table === null) return;
    let cancelled = false;
    void (async () => {
      setError(null);
      const sid = await sideSession();
      if (cancelled) return;
      // The password prompt was cancelled or the connection failed: say so, not "Loading…".
      if (sid === null) return setError(t("Not connected."));
      try {
        const loaded = await api.tableDesign(sid, tab.schema, tab.table!);
        if (cancelled) return;
        setOriginal(loaded);
        setDesign(loaded);
      } catch (e) {
        if (!cancelled) setError(toCommandError(e).message);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [tab.table, version]);

  const dirty = design !== null && JSON.stringify(design) !== JSON.stringify(original ?? blankDesign());
  useEffect(() => {
    if (Boolean(tab.unsaved) !== dirty) onChange({ unsaved: dirty });
  }, [dirty]);

  useEffect(() => {
    if (!design) return;
    let current = true;
    const timer = setTimeout(() => {
      api.tableScript(connection.driver, tab.schema, original, design).then(
        (p) => current && setPreview({ of: design, preview: p }),
        (e: unknown) => current && setPreview({ of: design, preview: { error: toCommandError(e).message } }),
      );
    }, 250);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [design, original]);

  if (!design) {
    return (
      <div className="object-tab" style={{ display: active ? "flex" : "none" }}>
        <div className="object-body">{error ? <div className="notice error">{error}</div> : <LoadingState compact title={t("Loading…")} />}</div>
      </div>
    );
  }

  const fresh = preview?.of === design ? preview.preview : null;
  const ready = fresh && !("error" in fresh) && fresh.script.statements.length > 0 ? fresh : null;
  async function apply() {
    if (!ready || !design) return;
    setRunning(true);
    setError(null);
    try {
      const sid = await sideSession();
      if (sid === null) return;
      await api.applyTableScript(sid, ready.script);
      if (original) {
        if (original.name !== design.name) {
          const where = { connectionId: connection.id, database: tab.database ?? null, schema: tab.schema, kind: "table" as const };
          renameBookmarks(where, original.name, design.name);
        }
        try {
          for (const m of tableCatalogMoves(original, design)) {
            await renameCatalog(
              connection.id,
              catalogKey(tab.database, tab.schema, m.from.object, m.from.column),
              catalogKey(tab.database, tab.schema, m.to.object, m.to.column),
            );
          }
        } catch (e) {
          workbench().showError(toCommandError(e).message);
        }
      }
      setConfirming(false);
      requestNavigatorRefresh({ connectionId: connection.id, database: tab.database, schema: tab.schema });
      onNeedCatalog(true);
      if (design.name !== tab.table) onChange({ table: design.name, title: design.name });
      else setVersion((v) => v + 1);
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="object-tab table-editor" style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <span className="object-icon" data-kind="table">
          <Icon name={objectIcon("table")} size={16} />
        </span>
        <label className="table-name">
          <input value={design.name} placeholder={t("Table name")} aria-label={t("Table name")} autoFocus={tab.table === null} onChange={(e) => setDesign((d) => d && { ...d, name: e.target.value })} />
          <span className="muted small">
            {t("in {schema}", { schema: tab.schema })}
            {tab.database ? ` · ${tab.database}` : ""}
          </span>
        </label>
        <span className="spacer" />
        <button className="square" disabled={!dirty} onClick={() => setDesign(original ?? blankDesign())} title={t("Discard the changes")} aria-label={t("Discard")}>
          <Icon name="revert" />
        </button>
        <button
          className="primary"
          disabled={!ready || connection.read_only}
          title={connection.read_only ? t("{name} is read-only", { name: connection.name }) : dirty ? undefined : t("No changes to save")}
          onClick={() => setConfirming(true)}
        >
          {t(tab.table === null ? "Create…" : "Save…")}
        </button>
      </header>
      <DesignForm
        driver={connection.driver}
        design={design}
        onChange={(change) => setDesign((d) => d && change(d))}
        schema={tab.schema}
        catalog={catalog?.schemas}
        section={section}
        onSection={(next) => onChange({ section: next })}
        sql={<ScriptView preview={preview?.preview ?? null} />}
        notice={error && <div className="notice error">{error}</div>}
        idPrefix={`table-editor-${tab.key}`}
      />
      {confirming && ready && (
        <div className="modal-backdrop" onMouseDown={() => !running && setConfirming(false)}>
          <form
            className="modal sql-action"
            onMouseDown={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.key === "Escape" && !running && setConfirming(false)}
            onSubmit={(e) => {
              e.preventDefault();
              void apply();
            }}
          >
            <h2>{tab.table === null ? t("Create {name}", { name: design.name }) : t("Alter {name}", { name: tab.table })}</h2>
            <ScriptView preview={ready} />
            {error && <div className="notice error small">{error}</div>}
            <div className="actions">
              <button
                type="button"
                className="link"
                onClick={() => {
                  workbench().openTab("query", connection.id, { sql: ready.sql, database: tab.database });
                  setConfirming(false);
                }}
              >
                {t("Open in editor")}
              </button>
              <span className="spacer" />
              <button type="button" disabled={running} onClick={() => setConfirming(false)}>
                {t("Cancel")}
              </button>
              <button type="submit" className="primary" autoFocus disabled={running}>
                {t(running ? "Running…" : tab.table === null ? "Create" : "Apply")}
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}

function ScriptView({ preview }: { preview: Preview | null }) {
  if (!preview) return <div className="muted small">…</div>;
  if ("error" in preview) return <div className="notice error small">{preview.error}</div>;
  return (
    <>
      {preview.script.warnings.map((w) => (
        <div key={w} className="notice warn small">
          {translateWarning(w)}
        </div>
      ))}
      <pre className="sql-preview">{preview.sql || t("No changes.")}</pre>
    </>
  );
}
