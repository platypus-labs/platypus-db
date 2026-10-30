import { useEffect, useRef, useState } from "react";
import { api, type Project, type SavedConnection, toCommandError } from "../api";
import { ConnectionPicker } from "./ConnectionPicker";
import { t } from "../i18n";
import { storeMessage } from "../storeErrors";
import { workbench } from "../workbench";
import { openSharedProject } from "./shared";

/** The colors offered for a project; any other can be picked too. */
export const PROJECT_COLORS = ["#6c63ff", "#d08f20", "#3d7bf7", "#a46be0", "#2f9e6e", "#e05a5a", "#1aa3b8", "#8a93a3"];

interface Props {
  /** The project to change; `null` makes a new one. */
  project: Project | null;
  onSaved: (projects: Project[], saved: Project) => void;
  onClose: () => void;
}

/** Name, color, initials and folder of a project. The initials follow the name until changed. */
export function ProjectDialog({ project, onSaved, onClose }: Props) {
  const [name, setName] = useState(project?.name ?? "");
  const [color, setColor] = useState(project?.color ?? PROJECT_COLORS[0]!);
  const [initials, setInitials] = useState(project?.initials ?? "");
  const [ownInitials, setOwnInitials] = useState(project !== null);
  const [error, setError] = useState<string | null>(null);
  const [all, setAll] = useState<SavedConnection[]>([]);
  const folders = workbench().projectFolders();
  const currentFolder = folders.find((f) => project && f.projects.includes(project.id))?.id ?? "";
  const [folder, setFolder] = useState(currentFolder);
  // A new project gets its id once, so saving again after an error does not make a second one.
  const newId = useRef(crypto.randomUUID());
  const [picked, setPicked] = useState(new Set(project?.connections ?? []));

  useEffect(() => {
    api.listConnections().then(setAll, () => {});
  }, []);

  // Computed in Rust, so the store and the dialog never disagree.
  useEffect(() => {
    if (ownInitials) return;
    let live = true;
    api.projectInitials(name).then((i) => live && setInitials(i), () => {});
    return () => {
      live = false;
    };
  }, [name, ownInitials]);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    const saved: Project = {
      id: project?.id ?? newId.current,
      name,
      color,
      initials,
      // The ones kept keep their order; the newly picked follow.
      connections: [...(project?.connections ?? []).filter((id) => picked.has(id)), ...[...picked].filter((id) => !project?.connections.includes(id))],
    };
    let all: Project[];
    try {
      all = await api.saveProject(saved);
    } catch (err) {
      setError(storeMessage(toCommandError(err).message));
      return;
    }
    // The rail's folders are read in the same call as the drag to a folder. A folder that cannot
    // be set does not lose the project, which is saved by now.
    if (folder !== currentFolder) {
      try {
        const w = await api.setProjectFolder(saved.id, folder || null);
        workbench().setWorkspace(w);
        all = w.projects;
      } catch (err) {
        workbench().showError(storeMessage(toCommandError(err).message));
      }
    }
    onSaved(all, all.find((p) => p.id === saved.id) ?? saved);
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="modal small project-dialog" onSubmit={save} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t(project ? "Edit project" : "New project")}</h2>
        <label className="field">
          <span>{t("Name")}</span>
          <input
            autoFocus
            required
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setError(null);
            }}
            placeholder={t("Client, product or team")}
          />
        </label>
        <label className="field">
          <span>{t("Color")}</span>
          <div className="swatches">
            {PROJECT_COLORS.map((c) => (
              <button
                type="button"
                key={c}
                className={`swatch${c === color ? " selected" : ""}`}
                style={{ background: c }}
                title={c}
                onClick={() => setColor(c)}
              />
            ))}
            <input type="color" value={color} onChange={(e) => setColor(e.target.value)} title={t("Another color")} aria-label={t("Another color")} />
          </div>
        </label>
        <label className="field">
          <span>{t("Initials — on the project's square")}</span>
          <div className="project-dialog-icon">
            <div className="project-dialog-preview" style={{ background: color }} aria-hidden>
              {initials}
            </div>
            <input
              className="project-initials"
              value={initials}
              maxLength={2}
              onChange={(e) => {
                setInitials(e.target.value);
                setOwnInitials(e.target.value !== "");
              }}
            />
          </div>
        </label>
        {folders.length > 0 && (
          <label className="field">
            <span>{t("Folder")}</span>
            <select value={folder} onChange={(e) => setFolder(e.target.value)} data-field="folder">
              <option value="">{t("None (top level)")}</option>
              {folders.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.name}
                </option>
              ))}
            </select>
          </label>
        )}
        {all.length > 0 && (
          <div className="field">
            <span>{t("Connections")}</span>
            <ConnectionPicker connections={all} picked={picked} onChange={setPicked} />
          </div>
        )}
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          {project === null && (
            <button
              type="button"
              data-action="open-shared"
              onClick={() => {
                onClose();
                void openSharedProject();
              }}
            >
              {t("Open shared project…")}
            </button>
          )}
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary">
            {t("Save")}
          </button>
        </div>
      </form>
    </div>
  );
}
