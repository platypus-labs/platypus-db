import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  api,
  type ImportedProject,
  type ImportFile,
  type ImportInto,
  type ImportSource,
  isEmbedded,
  type Project,
  type SavedConnection,
  toCommandError,
  type Workspace,
} from "../api";
import { connectionColor, typeBadge } from "../connections";
import { answerFor } from "../testAnswers";
import { Icon } from "../ui/Icon";
import { checkState, connectionsUnder, defaultInto, type FolderView, importRequest, projectTree, toggleAll } from "./importTree";

interface Props {
  existing: SavedConnection[];
  projects: Project[];
  /** The active project, which plain connections join; `null` for none. */
  project: string | null;
  onImported: (workspace: Workspace) => void;
  onClose: () => void;
}

const SOURCES: { value: ImportSource; label: string; file: string }[] = [
  { value: "dbeaver", label: "DBeaver", file: ".dbp · data-sources.json" },
  { value: "datagrip", label: "DataGrip", file: "dataSources.xml" },
  { value: "pgadmin", label: "pgAdmin", file: "servers.json" },
  { value: "pg_service", label: "pg_service", file: ".pg_service.conf" },
];

const HINTS: Record<ImportSource, string> = {
  dbeaver: "File › Export › DBeaver project (.dbp), or data-sources.json in the workspace's .dbeaver folder.",
  datagrip: "dataSources.xml, in the project's .idea folder.",
  pgadmin: "servers.json, from Tools › Export Servers.",
  pg_service: "The libpq service file, .pg_service.conf.",
};

const label = (source: ImportSource) => SOURCES.find((s) => s.value === source)!.label;
const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
/** The rail folder's default name: the file's, without its extension. */
const railName = (path: string) => fileName(path).replace(/\.[^.]+$/, "") || "Imported";

/** Whether `c` looks like a connection that is already saved. */
function duplicate(c: SavedConnection, existing: SavedConnection[]) {
  return existing.some(
    (e) => e.driver === c.driver && e.host === c.host && e.port === c.port && e.database === c.database && e.user === c.user,
  );
}

/** Reads the projects, folders and connections of DBeaver, DataGrip, pgAdmin or
 *  `pg_service.conf`, and saves the ticked ones. Passwords are asked for on first use. */
export function ImportDialog({ existing, projects, project, onImported, onClose }: Props) {
  const [found, setFound] = useState<ImportFile[]>([]);
  const [source, setSource] = useState<ImportSource>("dbeaver");
  const [file, setFile] = useState<ImportFile | null>(null);
  const [read, setRead] = useState<ImportedProject[] | null>(null);
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set());
  const [into, setInto] = useState<ImportInto[]>([]);
  const [railFolder, setRailFolder] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    api.detectImports().then(setFound, () => setFound([]));
  }, []);

  async function load(next: ImportFile) {
    setFile(next);
    setSource(next.source);
    setError(null);
    setRead(null);
    setTicked(new Set());
    try {
      const all = await api.readImport(next.source, next.path);
      setRead(all);
      setTicked(new Set(all.flatMap((p) => p.connections).filter((c) => !duplicate(c, existing)).map((c) => c.id)));
      setInto(all.map((p) => defaultInto(p, project)));
      setRailFolder(all.filter((p) => p.name).length > 1 ? railName(next.path) : null);
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  // A file dropped on the dialog is read as the chosen source.
  useEffect(() => {
    const off = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "drop" && payload.paths[0]) void load({ source, path: payload.paths[0] });
    });
    return () => void off.then((unlisten) => unlisten());
  }, [source]);

  async function choose() {
    const path = await answerFor("file", () => open({ multiple: false, title: `${label(source)} connections` }));
    if (typeof path === "string") await load({ source, path });
  }

  const count = ticked.size;
  const newProjects = (read ?? []).filter((p, i) => into[i]?.kind === "new_project" && p.connections.some((c) => ticked.has(c.id)));

  async function save() {
    if (!read || saving) return;
    setSaving(true);
    try {
      // The rail folder is offered for two new projects or more.
      onImported(await api.saveImported(importRequest(read, ticked, into, newProjects.length > 1 ? railFolder : null)));
    } catch (e) {
      setError(toCommandError(e).message);
      setSaving(false);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide import-dialog" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>Import connections and projects</h2>
        <span className="muted small">Source</span>
        <div className="import-sources">
          {SOURCES.map((s) => (
            <button
              key={s.value}
              type="button"
              className={`import-tile${source === s.value ? " toggled" : ""}`}
              data-source={s.value}
              onClick={() => setSource(s.value)}
            >
              <strong>{s.label}</strong>
              <span className="muted small">{s.file}</span>
            </button>
          ))}
          {found.map((f) => (
            <button
              key={f.path}
              type="button"
              className={`import-tile found${file?.path === f.path ? " toggled" : ""}`}
              title={f.path}
              onClick={() => void load(f)}
            >
              <strong>Found: {label(f.source)}</strong>
              <span className="muted small">{fileName(f.path)}</span>
            </button>
          ))}
        </div>
        <div className="import-drop">
          <Icon name="import" size={16} />
          {file ? (
            <span className="import-file" title={file.path}>
              {fileName(file.path)}
            </span>
          ) : (
            <span className="muted">Drop a file here, or</span>
          )}
          <button type="button" className="import-choose" onClick={() => void choose()}>
            {file ? "Choose another…" : "Choose file…"}
          </button>
          <span className="muted small import-hint">{HINTS[source]}</span>
        </div>
        {error && <div className="notice error small">{error}</div>}
        {read && read.length === 0 && <p className="muted small">No connections found in this file.</p>}
        {read && read.length > 0 && (
          <>
            <div className="import-tree" role="tree">
              {read.map((p, i) => (
                <ProjectRows
                  key={i}
                  project={p}
                  ticked={ticked}
                  existing={existing}
                  projects={projects}
                  into={into[i] ?? { kind: "none" }}
                  onTicked={setTicked}
                  onInto={(next) => setInto((all) => all.map((x, j) => (j === i ? next : x)))}
                />
              ))}
            </div>
            <p className="muted small">Passwords are not imported: each connection asks for its own the first time.</p>
          </>
        )}
        <div className="actions">
          {newProjects.length > 1 && (
            <label className="check import-rail">
              <input type="checkbox" checked={railFolder !== null} onChange={(e) => setRailFolder(e.target.checked ? railName(file?.path ?? "") : null)} />
              Put in a rail folder
              {railFolder !== null && <input className="import-rail-name" value={railFolder} onChange={(e) => setRailFolder(e.target.value)} />}
            </label>
          )}
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="primary" disabled={!read || count === 0 || saving} onClick={() => void save()}>
            Import {count > 0 ? count : ""}
          </button>
        </div>
      </div>
    </div>
  );
}

function Tick({ ids, ticked, onTicked }: { ids: string[]; ticked: ReadonlySet<string>; onTicked: (next: Set<string>) => void }) {
  const state = checkState(ids, ticked);
  return (
    <input
      type="checkbox"
      checked={state === "on"}
      ref={(el) => {
        if (el) el.indeterminate = state === "mixed";
      }}
      onChange={() => onTicked(toggleAll(ids, ticked))}
    />
  );
}

interface ProjectRowsProps {
  project: ImportedProject;
  ticked: ReadonlySet<string>;
  existing: SavedConnection[];
  projects: Project[];
  into: ImportInto;
  onTicked: (next: Set<string>) => void;
  onInto: (into: ImportInto) => void;
}

/** A project of the file, with where it goes, then its folders and connections. */
function ProjectRows({ project, ticked, existing, projects, into, onTicked, onInto }: ProjectRowsProps) {
  const root = projectTree(project);
  const ids = project.connections.map((c) => c.id);
  const value = into.kind === "existing" ? into.id : into.kind;
  const name = project.name ?? "";
  return (
    <>
      <div className="import-row project" role="treeitem">
        <Tick ids={ids} ticked={ticked} onTicked={onTicked} />
        <Icon name="database" />
        <strong>{project.name ?? "Connections"}</strong>
        <select
          className="import-into"
          value={value}
          onChange={(e) => {
            const v = e.target.value;
            onInto(v === "new_project" ? { kind: "new_project", name: name || "Imported" } : v === "none" ? { kind: "none" } : { kind: "existing", id: v });
          }}
        >
          <option value="new_project">New project{name ? ` "${name}"` : ""}</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              Into “{p.name}”
            </option>
          ))}
          <option value="none">No project</option>
        </select>
      </div>
      <FolderRows folder={root} depth={1} ticked={ticked} existing={existing} onTicked={onTicked} />
    </>
  );
}

function FolderRows({
  folder,
  depth,
  ticked,
  existing,
  onTicked,
}: {
  folder: FolderView;
  depth: number;
  ticked: ReadonlySet<string>;
  existing: SavedConnection[];
  onTicked: (next: Set<string>) => void;
}) {
  return (
    <>
      {folder.folders.map((f) => (
        <div key={f.path.join("/")} role="group">
          <div className="import-row folder" role="treeitem" style={{ paddingLeft: depth * 18 }}>
            <Tick ids={connectionsUnder(f).map((c) => c.id)} ticked={ticked} onTicked={onTicked} />
            <Icon name="open" />
            <span>{f.path[f.path.length - 1]}</span>
          </div>
          <FolderRows folder={f} depth={depth + 1} ticked={ticked} existing={existing} onTicked={onTicked} />
        </div>
      ))}
      {folder.connections.map((c) => {
        const badge = typeBadge(c);
        const saved = duplicate(c, existing);
        return (
          <label key={c.id} className="import-row connection" role="treeitem" style={{ paddingLeft: depth * 18 }}>
            <Tick ids={[c.id]} ticked={ticked} onTicked={onTicked} />
            <span className="dot" style={{ background: connectionColor(c) ?? "var(--border)" }} />
            <span className="import-name">{c.name}</span>
            {badge && <span className={`type-badge ${badge.toLowerCase()}`}>{badge}</span>}
            <span className="muted small import-target">
              {isEmbedded(c.driver) ? c.database : `${c.user ? `${c.user}@` : ""}${c.host}:${c.port ?? ""}/${c.database}`}
              {c.ssh ? ` · SSH ${c.ssh.host}` : ""}
              {saved ? " · already saved" : ""}
            </span>
          </label>
        );
      })}
    </>
  );
}
