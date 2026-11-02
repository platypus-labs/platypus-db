/**
 * Shared team projects: "Share project…" writes a project to a folder (a team's repository or
 * a shared drive) as `platypus-project.toml` and `scripts/`, and "Open shared project…" links
 * such a folder as a project. Neither carries a password; each user's own are asked at the
 * first connect and kept in their keychain.
 */
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { api, type Project, toCommandError } from "../api";
import { t } from "../i18n";
import { storeMessage } from "../storeErrors";
import { answerFor } from "../testAnswers";
import { Busy } from "../ui/State";
import { workbench } from "../workbench";

/** The messages of the backend's shared-project errors are already readable. */
const text = (e: unknown) => storeMessage(toCommandError(e).message);

/** Asks for a folder; `null` when the dialog is cancelled. */
export async function pickFolder(title: string, defaultPath?: string): Promise<string | null> {
  const picked = await answerFor("folder", () => open({ directory: true, multiple: false, title, defaultPath }));
  return typeof picked === "string" ? picked : null;
}

/** Asks for the folder of the project to open, and opens it as a project. */
export async function openSharedProject() {
  const folder = await pickFolder(t("Choose the shared project folder"));
  if (!folder) return;
  try {
    const opened = await api.openSharedProject(folder);
    workbench().setWorkspace(opened.workspace);
    workbench().switchProject(opened.project);
    workbench().notify(t("Shared project opened. Each connection asks for your own password the first time."));
  } catch (e) {
    workbench().showError(text(e));
  }
}

function ShareDialog({ project, onClose }: { project: Project; onClose: () => void }) {
  const [folder, setFolder] = useState("");
  const [shareUser, setShareUser] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function browse() {
    const picked = await pickFolder(t("Choose the folder to share the project in"));
    if (picked) setFolder(picked);
  }

  async function share(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      workbench().setWorkspace(await api.shareProject(project.id, folder, shareUser));
      onClose();
      workbench().notify(t("{name} is shared. Commit the folder to your repository or leave it on the shared drive.", { name: project.name }));
    } catch (err) {
      setError(text(err));
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="modal share-project-dialog" onSubmit={share} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Share project")}</h2>
        <p className="muted small">
          {t("Writes the project's connections and scripts to a folder, where a team can keep them in a repository or on a shared drive. No passwords, keys or key files are written: each person enters their own the first time they connect.")}
        </p>
        <label className="field">
          <span>{t("Folder")}</span>
          <div className="row tight">
            <input data-field="share-folder" autoFocus value={folder} placeholder={t("Folder in the repository or on the drive")} onChange={(e) => setFolder(e.target.value)} />
            <button type="button" data-action="share-browse" onClick={() => void browse()}>
              {t("Browse…")}
            </button>
          </div>
        </label>
        <div className="field">
          <label className="check">
            <input data-field="share-user" type="checkbox" checked={shareUser} onChange={(e) => setShareUser(e.target.checked)} />
            <span>{t("Include the user names of the connections")}</span>
          </label>
          <span className="muted small">{t("User names can identify people: leave this off if the folder is public.")}</span>
        </div>
        {error && (
          <div className="notice error small" data-field="share-error">
            {error}
          </div>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" data-action="share-submit" aria-busy={busy || undefined} disabled={busy || folder.trim() === ""}>
            {busy ? <Busy label={t("Sharing…")} /> : t("Share")}
          </button>
        </div>
      </form>
    </div>
  );
}

/** Opens the dialog that shares `project`. */
export function shareProject(project: Project) {
  workbench().openDialog((close) => <ShareDialog project={project} onClose={close} />);
}
