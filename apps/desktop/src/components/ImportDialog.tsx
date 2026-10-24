import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type ImportFile, type ImportSource, type SavedConnection, toCommandError } from "../api";

interface Props {
  existing: SavedConnection[];
  /** The active project, which the imported connections join; `null` for none. */
  project: string | null;
  onImported: (all: SavedConnection[]) => void;
  onClose: () => void;
}

const SOURCES: { value: ImportSource; label: string; file: string }[] = [
  { value: "dbeaver", label: "DBeaver", file: "data-sources.json" },
  { value: "datagrip", label: "DataGrip", file: "dataSources.xml (in the project's .idea folder)" },
  { value: "pgadmin", label: "pgAdmin", file: "servers.json (Tools › Export Servers)" },
  { value: "pg_service", label: "pg_service.conf", file: ".pg_service.conf" },
];

const label = (source: ImportSource) => SOURCES.find((s) => s.value === source)!.label;

/** Whether `c` looks like a connection that is already saved. */
function duplicate(c: SavedConnection, existing: SavedConnection[]) {
  return existing.some(
    (e) => e.driver === c.driver && e.host === c.host && e.port === c.port && e.database === c.database && e.user === c.user,
  );
}

/** Reads the connections of DBeaver, DataGrip, pgAdmin or `pg_service.conf` and saves the
 *  chosen ones. Passwords are asked for on first use. */
export function ImportDialog({ existing, project, onImported, onClose }: Props) {
  const [found, setFound] = useState<ImportFile[] | null>(null);
  const [file, setFile] = useState<ImportFile | null>(null);
  const [connections, setConnections] = useState<SavedConnection[] | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [source, setSource] = useState<ImportSource>("dbeaver");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.detectImports().then(setFound, () => setFound([]));
  }, []);

  async function read(next: ImportFile) {
    setFile(next);
    setError(null);
    setConnections(null);
    try {
      const all = await api.readImport(next.source, next.path);
      setConnections(all);
      setPicked(new Set(all.filter((c) => !duplicate(c, existing)).map((c) => c.id)));
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  async function choose() {
    const path = await open({ multiple: false, title: `${label(source)} connections` });
    if (typeof path === "string") await read({ source, path });
  }

  async function save() {
    try {
      onImported(await api.saveImported(connections!.filter((c) => picked.has(c.id)), project));
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  const toggle = (id: string) =>
    setPicked((p) => {
      const next = new Set(p);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>Import connections</h2>
        {found && found.length > 0 && (
          <div className="import-found">
            <span className="muted small">Found on this computer:</span>
            {found.map((f) => (
              <button
                key={f.path}
                type="button"
                className={file?.path === f.path ? "toggled" : ""}
                onClick={() => void read(f)}
                title={f.path}
              >
                {label(f.source)}
              </button>
            ))}
          </div>
        )}
        <div className="row">
          <label className="field grow">
            <span>From a file</span>
            <select value={source} onChange={(e) => setSource(e.target.value as ImportSource)}>
              {SOURCES.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label} — {s.file}
                </option>
              ))}
            </select>
          </label>
          <button type="button" className="align-end" onClick={() => void choose()}>
            Choose file…
          </button>
        </div>
        {error && <div className="notice error small">{error}</div>}
        {file && connections && (
          <>
            <p className="muted small" title={file.path}>
              {connections.length} {connections.length === 1 ? "connection" : "connections"} in {label(file.source)}.
              Passwords are not imported: each connection asks for its own the first time.
            </p>
            <div className="import-list">
              {connections.map((c) => (
                <label key={c.id} className="check">
                  <input type="checkbox" checked={picked.has(c.id)} onChange={() => toggle(c.id)} />
                  <strong>{c.name}</strong>
                  <span className="muted small">
                    {c.driver === "sqlite" ? c.database : `${c.user ? `${c.user}@` : ""}${c.host}:${c.port ?? ""}/${c.database}`}
                    {c.ssh ? ` · SSH ${c.ssh.host}` : ""}
                    {c.connection_type ? ` · ${c.connection_type}` : ""}
                    {duplicate(c, existing) ? " · already saved" : ""}
                  </span>
                </label>
              ))}
            </div>
          </>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="primary" disabled={!connections || picked.size === 0} onClick={() => void save()}>
            Import {picked.size > 0 ? picked.size : ""}
          </button>
        </div>
      </div>
    </div>
  );
}
