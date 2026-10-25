import { api, type Project, type ProjectFolder, toCommandError, type Workspace } from "../api";
import { Icon } from "../ui/Icon";
import { workbench } from "../workbench";
import { railItems } from "./state";

interface Props {
  projects: Project[];
  folders: ProjectFolder[];
  /** The active project's id, `null` for All connections. */
  active: string | null;
  /** Projects with an open transaction in one of their tabs, by `projectKey`. */
  txProjects: Set<string>;
  onSwitch: (id: string | null) => void;
  onNew: () => void;
  onMenu: (project: Project, e: React.MouseEvent) => void;
  onFolderMenu: (folder: ProjectFolder, e: React.MouseEvent) => void;
}

const DRAG_TYPE = "application/x-platypus-project";

/** Runs a rail change and shows what it returned, or its error. */
function change(run: () => Promise<Workspace>) {
  run().then(
    (w) => workbench().setWorkspace(w),
    (e: unknown) => workbench().showError(toCommandError(e).message),
  );
}

/** Lets a project be dropped here, unless it is `self`. */
const dropTarget = (self: string | null, onDrop: (dragged: string) => void) => ({
  onDragOver: (e: React.DragEvent) => {
    if (!e.dataTransfer.types.includes(DRAG_TYPE)) return;
    e.preventDefault();
    e.stopPropagation();
  },
  onDrop: (e: React.DragEvent) => {
    const dragged = e.dataTransfer.getData(DRAG_TYPE);
    if (!dragged) return;
    e.preventDefault();
    e.stopPropagation();
    if (dragged !== self) onDrop(dragged);
  },
});

/** The column of project squares left of the navigator: All connections, then the projects
 *  and folders of projects in their saved order, then a square to make a new project.
 *  Dropping a project on another makes a folder of the two, as in Discord. */
export function ProjectRail({ projects, folders, active, txProjects, onSwitch, onNew, onMenu, onFolderMenu }: Props) {
  const square = (key: string, current: boolean, tx: boolean) =>
    `project-square${current ? " active" : ""}${tx ? " tx" : ""}${key === "all" ? " all" : ""}`;

  const projectSquare = (p: Project) => (
    <button
      type="button"
      key={p.id}
      className={square(p.id, p.id === active, txProjects.has(p.id))}
      data-project={p.id}
      title={p.name}
      aria-pressed={p.id === active}
      style={{ background: p.color }}
      draggable
      onDragStart={(e) => e.dataTransfer.setData(DRAG_TYPE, p.id)}
      {...dropTarget(p.id, (dragged) => change(() => api.groupProjects(p.id, dragged)))}
      onClick={() => onSwitch(p.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu(p, e);
      }}
    >
      {p.initials}
    </button>
  );

  const folderMenu = (f: ProjectFolder) => (e: React.MouseEvent) => {
    e.preventDefault();
    onFolderMenu(f, e);
  };
  const toggle = (f: ProjectFolder) => change(() => api.saveProjectFolder({ ...f, collapsed: !f.collapsed }));
  const into = (f: ProjectFolder) => dropTarget(null, (dragged) => change(() => api.setProjectFolder(dragged, f.id)));

  return (
    <nav
      className="project-rail"
      aria-label="Projects"
      {...dropTarget(null, (dragged) =>
        // Dropped on the rail itself: out of its folder, at the end.
        change(async () => {
          await api.setProjectFolder(dragged, null);
          return api.reorderProjects([...projects.map((p) => p.id).filter((id) => id !== dragged), dragged]);
        }),
      )}
    >
      <button
        type="button"
        className={square("all", active === null, txProjects.has("all"))}
        data-project="all"
        title="All connections"
        aria-pressed={active === null}
        onClick={() => onSwitch(null)}
      >
        <Icon name="database" size={16} />
      </button>
      {railItems(projects, folders).map((item) => {
        if (item.kind === "project") return projectSquare(item.project);
        const f = item.folder;
        const holdsActive = item.projects.some((p) => p.id === active);
        const tx = item.projects.some((p) => txProjects.has(p.id));
        if (f.collapsed) {
          return (
            <button
              type="button"
              key={f.id}
              className={`project-folder${holdsActive ? " active" : ""}${tx ? " tx" : ""}`}
              data-folder={f.id}
              title={`${f.name}: ${item.projects.map((p) => p.name).join(", ")}`}
              aria-expanded={false}
              onClick={() => toggle(f)}
              onContextMenu={folderMenu(f)}
              {...into(f)}
            >
              {item.projects.slice(0, 4).map((p) => (
                <i key={p.id} style={{ background: p.color }} />
              ))}
            </button>
          );
        }
        return (
          <div key={f.id} className="project-folder-open" data-folder={f.id} style={{ borderColor: f.color }} {...into(f)}>
            <button
              type="button"
              className="project-folder-head"
              title={f.name}
              aria-expanded
              style={{ color: f.color }}
              onClick={() => toggle(f)}
              onContextMenu={folderMenu(f)}
            >
              <Icon name="open" size={14} />
            </button>
            {item.projects.map(projectSquare)}
          </div>
        );
      })}
      <button type="button" className="project-add" title="New project" onClick={onNew}>
        <Icon name="plus" size={14} />
      </button>
    </nav>
  );
}
