import { useEffect, useState } from "react";
import { api, type Project, toCommandError } from "../api";
import { t } from "../i18n";
import { storeMessage } from "../storeErrors";

/** The colors offered for a project; any other can be picked too. */
export const PROJECT_COLORS = ["#6c63ff", "#d08f20", "#3d7bf7", "#a46be0", "#2f9e6e", "#e05a5a", "#1aa3b8", "#8a93a3"];

interface Props {
  /** The project to change; `null` makes a new one. */
  project: Project | null;
  onSaved: (projects: Project[], saved: Project) => void;
  onClose: () => void;
}

/** Name, color and initials of a project. The initials follow the name until changed. */
export function ProjectDialog({ project, onSaved, onClose }: Props) {
  const [name, setName] = useState(project?.name ?? "");
  const [color, setColor] = useState(project?.color ?? PROJECT_COLORS[0]!);
  const [initials, setInitials] = useState(project?.initials ?? "");
  const [ownInitials, setOwnInitials] = useState(project !== null);
  const [error, setError] = useState<string | null>(null);

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
      id: project?.id ?? crypto.randomUUID(),
      name,
      color,
      initials,
      connections: project?.connections ?? [],
    };
    try {
      const all = await api.saveProject(saved);
      onSaved(all, all.find((p) => p.id === saved.id) ?? saved);
    } catch (err) {
      setError(storeMessage(toCommandError(err).message));
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="modal small project-dialog" onSubmit={save} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t(project ? "Edit project" : "New project")}</h2>
        <div className="project-dialog-preview" style={{ background: color }} aria-hidden>
          {initials}
        </div>
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
          <input
            className="project-initials"
            value={initials}
            maxLength={2}
            onChange={(e) => {
              setInitials(e.target.value);
              setOwnInitials(e.target.value !== "");
            }}
          />
        </label>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
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
