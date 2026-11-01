import { useCallback, useEffect, useRef, useState } from "react";
import { dispatchKey } from "../actions";
import { type GitStatus, type Project, type SavedConnection, type ScriptEntry, type ScriptList, api, toCommandError } from "../api";
import { ContextMenu, type MenuState } from "../components/ContextMenu";
import { Icon } from "../ui/Icon";
import { moveScript, registerScriptConnections } from "./actions";
import { GitBar } from "./GitBar";
import { badgeTitle } from "./GitDialogs";
import { changedInFolder, statusByPath } from "./gitModel";
import { connectionFor, nameOf, onScriptsChanged, openScript, parentOf, scriptsChanged } from "./scripts";
import { t } from "../i18n";
import { EmptyState } from "../ui/State";
import { markedTarget } from "../ui/pointerDrag";
import { useDrag } from "../ui/useDrag";

/** Marks the folder a dragged script or folder drops into: `""` is the top. */
const DROP = "data-script-folder";

interface Props {
  project: Project | null;
  /** The project's connections: the ones a script can open on. */
  connections: SavedConnection[];
  onError: (message: string) => void;
}

/** The active project's saved scripts, under the navigator. */
export function ScriptsPanel({ project, connections, onError }: Props) {
  const projectId = project?.id ?? null;
  const [list, setList] = useState<ScriptList | null>(null);
  const [open, setOpen] = useState(true);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [editing, setEditing] = useState<string | null>(null);
  const [name, setName] = useState("");
  // Escape leaves the name as it was; the blur that follows must not save it.
  const cancelled = useRef(false);
  const [menu, setMenu] = useState<MenuState | null>(null);
  /** The folder a dragged script would move into. */
  const [dropInto, setDropInto] = useState<string | null>(null);
  const startDrag = useDrag();
  // A drop moves the script in the project shown then.
  const latestProject = useRef(projectId);
  latestProject.current = projectId;
  const closeMenu = useCallback(() => setMenu(null), []);
  const fail = (e: unknown) => onError(toCommandError(e).message);

  // Git: the status of the scripts folder, or why it could not be read.
  const [git, setGit] = useState<GitStatus | null>(null);
  const [gitProblem, setGitProblem] = useState<{ kind: string; message: string } | null>(null);

  const load = useCallback(() => {
    api.listScripts(projectId).then(setList, fail);
    api.gitStatus(projectId).then(
      (s) => {
        setGit(s);
        setGitProblem(null);
      },
      (e) => {
        const { kind, message } = toCommandError(e);
        setGit(null);
        setGitProblem({ kind, message });
      },
    );
  }, [projectId]);
  useEffect(load, [load]);
  // Files change outside the app too: look again when the window comes back.
  useEffect(() => {
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [load]);
  useEffect(() => onScriptsChanged(load), [load]);
  useEffect(() => registerScriptConnections(connections), [connections]);

  function rename(path: string) {
    setEditing(path);
    setName(nameOf(path).replace(/\.sql$/i, ""));
    // A new entry may be inside a collapsed folder.
    setCollapsed((c) => new Set([...c].filter((f) => !path.startsWith(`${f}/`))));
  }

  async function commitRename(entry: ScriptEntry) {
    setEditing(null);
    if (cancelled.current) {
      cancelled.current = false;
      return;
    }
    const typed = name.trim();
    if (!typed) return;
    const file = entry.folder || /\.sql$/i.test(typed) ? typed : `${typed}.sql`;
    const parent = parentOf(entry.path);
    const to = parent ? `${parent}/${file}` : file;
    if (to === entry.path) return;
    try {
      await api.renameScript(projectId, entry.path, to);
      scriptsChanged();
    } catch (e) {
      fail(e);
    }
  }

  async function create(folder: boolean) {
    try {
      let path: string;
      if (folder) {
        const base = t("New folder");
        path = base;
        for (let n = 2; list?.entries.some((e) => e.path === path); n++) path = `${base} ${n}`;
        await api.createScriptFolder(projectId, path);
      } else {
        const only = connections.length === 1 ? connections[0]!.id : null;
        path = await api.createScript(projectId, "", t("Script"), only);
      }
      setOpen(true);
      scriptsChanged();
      rename(path);
    } catch (e) {
      fail(e);
    }
  }

  const entries = list?.entries ?? [];
  const changed = statusByPath(git?.files ?? []);
  const hidden = (path: string) => [...collapsed].some((f) => path.startsWith(`${f}/`));
  /** Drags a script or folder into another folder, or (on the header) to the top. */
  const drag = (path: string) => (e: React.PointerEvent) =>
    startDrag<string>(e, {
      label: nameOf(path),
      target: (el) => {
        const folder = markedTarget(el, DROP)?.value;
        const moves = folder !== undefined && folder !== parentOf(path) && folder !== path && !folder.startsWith(`${path}/`);
        return moves ? folder : null;
      },
      show: setDropInto,
      drop: (folder) => moveScript(latestProject.current, path, folder).catch(fail),
    });

  return (
    <section className={`scripts-panel${open ? " open" : ""}`} aria-label={t("Scripts")}>
      <div className={`sidebar-header scripts-header${dropInto === "" ? " drop-into" : ""}`} data-script-folder="">
        <button className="icon twisty-button" onClick={() => setOpen((o) => !o)} aria-expanded={open} title={t(open ? "Hide scripts" : "Show scripts")}>
          <Icon name={open ? "chevron-down" : "chevron-right"} size={12} />
        </button>
        <span className="sidebar-title">{t("Scripts")}</span>
        <button className="icon" title={t("New script")} aria-label={t("New script")} onClick={() => void create(false)}>
          <Icon name="new-file" />
        </button>
        <button className="icon" title={t("New folder")} aria-label={t("New folder")} onClick={() => void create(true)}>
          <Icon name="new-folder" />
        </button>
      </div>
      {/* With no scripts the Git proposal goes after the empty state: it is not the main action. */}
      {open && !(list && entries.length === 0) && <GitBar project={projectId} status={git} problem={gitProblem} reload={load} onError={onError} />}
      {open && (
        <ul className="tree scripts-tree">
          {entries.map((entry) => {
            if (hidden(entry.path)) return null;
            const depth = entry.path.split("/").length - 1;
            const isCollapsed = collapsed.has(entry.path);
            const linked = connections.find((c) => c.id === entry.connection);
            const fileStatus = changed.get(entry.path);
            const arg = { entry, project: projectId, root: list!.root, connections, rename, git: fileStatus };
            return (
              <li
                key={entry.path}
                className={`tree-row script-row${dropInto === entry.path ? " drop-into" : ""}`}
                style={{ paddingLeft: 6 + depth * 14 }}
                tabIndex={0}
                onKeyDown={(e) => editing !== entry.path && dispatchKey("script", arg, e.nativeEvent, fail)}
                data-path={entry.path}
                data-script-folder={entry.folder ? entry.path : undefined}
                onPointerDown={editing === entry.path ? undefined : drag(entry.path)}
                onClick={() =>
                  entry.folder &&
                  setCollapsed((c) => {
                    const next = new Set(c);
                    if (next.has(entry.path)) next.delete(entry.path);
                    else next.add(entry.path);
                    return next;
                  })
                }
                onDoubleClick={() => {
                  const c = connectionFor(entry, connections);
                  if (!entry.folder && c) openScript(list!.root, entry, c).catch(fail);
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, target: { scope: "script", arg } });
                }}
              >
                <span className="twisty">
                  {entry.folder && <Icon name={isCollapsed ? "chevron-right" : "chevron-down"} size={12} />}
                </span>
                <span className="tree-icon">
                  <Icon name={entry.folder ? "folder" : "file"} />
                </span>
                {editing === entry.path ? (
                  <input
                    className="script-rename"
                    autoFocus
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    onClick={(e) => e.stopPropagation()}
                    onBlur={() => void commitRename(entry)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") e.currentTarget.blur();
                      if (e.key === "Escape") {
                        cancelled.current = true;
                        e.currentTarget.blur();
                      }
                    }}
                  />
                ) : (
                  <span className="tree-label">{entry.folder ? nameOf(entry.path) : nameOf(entry.path).replace(/\.sql$/i, "")}</span>
                )}
                {fileStatus && (
                  <span className={`git-badge ${fileStatus.badge === "?" ? "untracked" : fileStatus.badge}`} title={badgeTitle(fileStatus.badge)}>
                    {fileStatus.badge}
                  </span>
                )}
                {entry.folder && changedInFolder(entry.path, git?.files ?? []) && (
                  <span className="git-badge folder" title={t("Contains changed scripts")}>
                    •
                  </span>
                )}
                {linked && (
                  <span className="tree-detail" title={t("Opens on {name}", { name: linked.name })}>
                    {linked.name}
                  </span>
                )}
              </li>
            );
          })}
          {list && entries.length === 0 && (
            <li className="scripts-empty">
              <EmptyState
                compact
                title={t("No saved scripts")}
                actions={
                  <button type="button" onClick={() => void create(false)}>
                    {t("New script")}
                  </button>
                }
              />
            </li>
          )}
        </ul>
      )}
      {open && list && entries.length === 0 && <GitBar project={projectId} status={git} problem={gitProblem} reload={load} onError={onError} />}
      {menu && <ContextMenu menu={menu} onClose={closeMenu} onError={fail} />}
    </section>
  );
}
