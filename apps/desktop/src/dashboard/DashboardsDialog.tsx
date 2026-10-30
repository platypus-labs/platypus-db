import { useCallback, useEffect, useState } from "react";
import { api, type DashboardEntry, type SavedConnection, toCommandError } from "../api";
import { locale, t } from "../i18n";
import { ask } from "../ui/ask";
import { askName } from "../ui/NameDialog";
import { openSavedDashboard } from "./files";

interface Props {
  /** The connection a dashboard opens on when the one it names is gone. */
  connection: SavedConnection;
  project: string | null;
  onClose: () => void;
}

/** The saved dashboards of the project: open, rename and delete them. */
export function DashboardsDialog({ connection, project, onClose }: Props) {
  const [list, setList] = useState<DashboardEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fail = (e: unknown) => setError(toCommandError(e).message);

  const load = useCallback(() => {
    api.listDashboards(project).then(setList, fail);
  }, [project]);
  useEffect(load, [load]);

  async function open(name: string) {
    try {
      await openSavedDashboard(project, name, connection);
      onClose();
    } catch (e) {
      fail(e);
    }
  }

  async function remove(name: string) {
    if (!(await ask(t("Delete the dashboard {name}? The file is removed from disk.", { name }), { confirm: t("Delete"), danger: true }))) return;
    try {
      await api.deleteDashboard(project, name);
      setError(null);
      load();
    } catch (e) {
      fail(e);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal dashboards-dialog" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Dashboards")}</h2>
        {error && (
          <div className="notice error small" data-field="error">
            {error}
          </div>
        )}
        {list?.length === 0 && (
          <p className="muted small" data-field="empty">
            {t("No saved dashboards")}
          </p>
        )}
        <ul className="dashboards-list">
          {(list ?? []).map((d) => (
            <li key={d.name} data-dashboard={d.name}>
              <span className="dashboards-name">{d.name}</span>
              <span className="muted small">{new Date(d.modified).toLocaleString(locale())}</span>
              <span className="spacer" />
              <button type="button" className="primary" data-action="open" onClick={() => void open(d.name)}>
                {t("Open")}
              </button>
              <button
                type="button"
                data-action="rename"
                onClick={() =>
                  askName(t("Rename dashboard"), d.name, async (name) => {
                    await api.renameDashboard(project, d.name, name);
                    setError(null);
                    load();
                  })
                }
              >
                {t("Rename")}
              </button>
              <button type="button" className="danger" data-action="delete" onClick={() => void remove(d.name)}>
                {t("Delete")}
              </button>
            </li>
          ))}
        </ul>
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}
