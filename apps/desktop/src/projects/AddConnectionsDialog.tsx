import { useState } from "react";
import { api, type Project, type SavedConnection, toCommandError } from "../api";
import { workbench } from "../workbench";
import { toggled } from "../sets";

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
      setError(toCommandError(e).message);
      // The ones added before the error are in the project already.
      if (projects) workbench().setProjects(projects);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal small add-connections" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>Add connections to {project.name}</h2>
        {outside.length === 0 ? (
          <p className="muted small">Every saved connection is in this project already.</p>
        ) : (
          <div className="import-list">
            {outside.map((c) => (
              <label key={c.id} className="check">
                <input type="checkbox" checked={picked.has(c.id)} onChange={() => setPicked((p) => toggled(p, c.id))} />
                {c.name}
              </label>
            ))}
          </div>
        )}
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="primary" disabled={picked.size === 0} onClick={() => void save()}>
            Add {picked.size > 0 ? picked.size : ""}
          </button>
        </div>
      </div>
    </div>
  );
}
