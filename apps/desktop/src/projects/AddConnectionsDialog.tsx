import { useState } from "react";
import { api, type Project, type SavedConnection, toCommandError } from "../api";
import { workbench } from "../workbench";
import { ConnectionPicker } from "./ConnectionPicker";
import { t } from "../i18n";
import { storeMessage } from "../storeErrors";

interface Props {
  project: Project;
  connections: SavedConnection[];
  onSaved: (projects: Project[]) => void;
  onClose: () => void;
}

/** Adds saved connections to a project: those in no project or only in others. */
export function AddConnectionsDialog({ project, connections, onSaved, onClose }: Props) {
  const outside = connections.filter((c) => !project.connections.includes(c.id));
  const [picked, setPicked] = useState(new Set<string>());
  const [error, setError] = useState<string | null>(null);

  async function save() {
    let projects: Project[] | null = null;
    try {
      for (const id of picked) projects = await api.setProjectMembership(id, project.id, true);
      onSaved(projects!);
    } catch (e) {
      setError(storeMessage(toCommandError(e).message));
      // The ones added before the error are in the project already.
      if (projects) workbench().setProjects(projects);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal small add-connections" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Add connections to {name}", { name: project.name })}</h2>
        {outside.length === 0 ? (
          <p className="muted small">{t("Every saved connection is in this project already.")}</p>
        ) : (
          <ConnectionPicker connections={outside} picked={picked} onChange={setPicked} />
        )}
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="button" className="primary" disabled={picked.size === 0} onClick={() => void save()}>
            {picked.size > 0 ? t("Add {n}", { n: picked.size }) : t("Add")}
          </button>
        </div>
      </div>
    </div>
  );
}
