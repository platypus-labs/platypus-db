import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, type ConnectionFolder, isEmbedded, type Project, type SavedConnection, toCommandError } from "../api";
import { connectionColor, typeBadge } from "../connections";
import { answerFor } from "../testAnswers";
import { Icon } from "../ui/Icon";
import { DialogHeader } from "../ui/DialogHeader";
import { Tick } from "./ImportDialog";
import { t } from "../i18n";

interface Props {
  connections: SavedConnection[];
  projects: Project[];
  /** Connections ticked when the dialog opens. */
  preselect: string[];
  onExported: (count: number) => void;
  onClose: () => void;
}

const MIN_PASSWORD = 8;

/** Writes the ticked connections to a `.platypusconn` file, in clear or sealed with a password
 *  (which alone may carry the saved passwords). */
export function ExportConnectionsDialog({ connections, projects, preselect, onExported, onClose }: Props) {
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set(preselect));
  const [keepProjects, setKeepProjects] = useState(true);
  const [encrypt, setEncrypt] = useState(false);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [credentials, setCredentials] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const byId = new Map(connections.map((c) => [c.id, c]));
  const inProject = new Set(projects.flatMap((p) => p.connections));
  const loose = connections.filter((c) => !inProject.has(c.id));
  const all = connections.map((c) => c.id);
  const selected = all.filter((id) => ticked.has(id));
  const count = selected.length;

  const tooShort = encrypt && password.length > 0 && password.length < MIN_PASSWORD;
  const mismatch = encrypt && confirm.length > 0 && confirm !== password;
  const passwordOk = !encrypt || (password.length >= MIN_PASSWORD && confirm === password);

  async function submit() {
    if (busy || count === 0 || !passwordOk) return;
    setBusy(true);
    setError(null);
    try {
      const path = await answerFor("file", () =>
        save({ defaultPath: "connections.platypusconn", filters: [{ name: "PlatypusDB", extensions: ["platypusconn"] }] }),
      );
      if (!path) {
        setBusy(false);
        return;
      }
      await api.exportConnections(
        { connections: selected, projects: keepProjects, password: encrypt ? password : null, credentials: encrypt && credentials },
        path,
      );
      onExported(count);
    } catch (e) {
      setError(toCommandError(e).message);
      setBusy(false);
    }
  }

  const rows = (ids: string[], depth: number) =>
    ids.flatMap((id) => byId.get(id) ?? []).map((c) => <ConnectionRow key={c.id} c={c} depth={depth} ticked={ticked} onTicked={setTicked} />);

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide import-dialog export-connections" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <DialogHeader title={t("Export connections")} onClose={onClose} />
        <label className="check export-all">
          <Tick ids={all} ticked={ticked} onTicked={setTicked} />
          {t("Select all")}
        </label>
        <div className="import-tree" role="tree">
          {projects.map((p) => {
            const ids = p.connections.filter((id) => byId.has(id));
            return ids.length === 0 ? null : (
              <div key={p.id} role="group">
                <div className="import-row project" role="treeitem">
                  <Tick ids={ids} ticked={ticked} onTicked={setTicked} />
                  <Icon name="database" />
                  <strong>{p.name}</strong>
                </div>
                <FolderRows project={p} parent={null} depth={1} ticked={ticked} onTicked={setTicked} byId={byId} />
              </div>
            );
          })}
          {loose.length > 0 && (
            <div role="group">
              <div className="import-row project" role="treeitem">
                <Tick ids={loose.map((c) => c.id)} ticked={ticked} onTicked={setTicked} />
                <Icon name="database" />
                <strong>{t("No project")}</strong>
              </div>
              {rows(
                loose.map((c) => c.id),
                1,
              )}
            </div>
          )}
        </div>
        <div className="export-options">
          <label className="check">
            <input type="checkbox" checked={keepProjects} onChange={(e) => setKeepProjects(e.target.checked)} />
            {t("Keep projects and folders")}
          </label>
          <label className="check">
            <input type="checkbox" checked={encrypt} onChange={(e) => setEncrypt(e.target.checked)} />
            {t("Encrypt with a password")}
          </label>
          {encrypt && (
            <div className="export-password">
              <input type="password" placeholder={t("Password")} aria-label={t("Password")} value={password} onChange={(e) => setPassword(e.target.value)} />
              <input type="password" placeholder={t("Confirm password")} aria-label={t("Confirm password")} value={confirm} onChange={(e) => setConfirm(e.target.value)} />
            </div>
          )}
          {tooShort && <div className="notice error small export-password-error">{t("Use at least 8 characters.")}</div>}
          {mismatch && !tooShort && <div className="notice error small export-password-error">{t("The passwords do not match.")}</div>}
          <label className="check import-passwords">
            <input type="checkbox" checked={encrypt && credentials} disabled={!encrypt} onChange={(e) => setCredentials(e.target.checked)} />
            <span>
              {t("Include saved passwords")}
              <span className="muted small import-passwords-hint">{t("Without a password on the file, no password is ever written.")}</span>
            </span>
          </label>
        </div>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="button" className="primary" disabled={count === 0 || !passwordOk || busy} onClick={() => void submit()}>
            {count > 0 ? t("Export {n}", { n: count }) : t("Export")}
          </button>
        </div>
      </div>
    </div>
  );
}

function ConnectionRow({ c, depth, ticked, onTicked }: { c: SavedConnection; depth: number; ticked: ReadonlySet<string>; onTicked: (next: Set<string>) => void }) {
  const badge = typeBadge(c);
  return (
    <label className="import-row connection" role="treeitem" style={{ paddingLeft: depth * 18 }}>
      <Tick ids={[c.id]} ticked={ticked} onTicked={onTicked} />
      <span className="dot" style={{ background: connectionColor(c) ?? "var(--border)" }} />
      <span className="import-name">{c.name}</span>
      {badge && <span className={`type-badge ${badge.toLowerCase()}`}>{badge}</span>}
      <span className="muted small import-target">{isEmbedded(c.driver) ? c.database : `${c.host}:${c.port ?? ""}/${c.database}`}</span>
    </label>
  );
}

/** The connection ids placed directly in `folder` (`null` for the root) of a project. */
function idsIn(project: Project, folder: ConnectionFolder | null): string[] {
  return project.connections.filter((id) => (project.placement?.[id] ?? null) === (folder?.id ?? null));
}

/** The ids under `folder`, subfolders included. */
function idsUnder(project: Project, folder: ConnectionFolder | null): string[] {
  const own = idsIn(project, folder);
  const subs = (project.folders ?? []).filter((f) => (f.parent ?? null) === (folder?.id ?? null));
  return [...own, ...subs.flatMap((f) => idsUnder(project, f))];
}

function FolderRows({
  project,
  parent,
  depth,
  ticked,
  onTicked,
  byId,
}: {
  project: Project;
  parent: ConnectionFolder | null;
  depth: number;
  ticked: ReadonlySet<string>;
  onTicked: (next: Set<string>) => void;
  byId: Map<string, SavedConnection>;
}) {
  const folders = (project.folders ?? []).filter((f) => (f.parent ?? null) === (parent?.id ?? null));
  const own = idsIn(project, parent);
  return (
    <>
      {folders.map((f) => (
        <div key={f.id} role="group">
          <div className="import-row folder" role="treeitem" style={{ paddingLeft: depth * 18 }}>
            <Tick ids={idsUnder(project, f).filter((id) => byId.has(id))} ticked={ticked} onTicked={onTicked} />
            <Icon name="open" />
            <span>{f.name}</span>
          </div>
          <FolderRows project={project} parent={f} depth={depth + 1} ticked={ticked} onTicked={onTicked} byId={byId} />
        </div>
      ))}
      {own.flatMap((id) => byId.get(id) ?? []).map((c) => (
        <ConnectionRow key={c.id} c={c} depth={depth} ticked={ticked} onTicked={onTicked} />
      ))}
    </>
  );
}
