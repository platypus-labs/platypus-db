import { useRef, useState } from "react";
import { api, type Project, type ProjectFolder, toCommandError, type Workspace } from "../api";
import { Icon } from "../ui/Icon";
import { workbench } from "../workbench";
import { type RailChange, type RailSpot, railDrop, railItems } from "./state";
import { t } from "../i18n";
import { storeMessage } from "../storeErrors";
import { edgeZone, markedTarget, type Point } from "../ui/pointerDrag";
import { useDrag } from "../ui/useDrag";

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
  /** Right click on "+": New project, Open shared project. */
  onAddMenu: (e: React.MouseEvent) => void;
}

/** What a part of the rail is as a drop target: `data-drop` names it, `data-project` or
 *  `data-folder` says which. */
const DROP = "data-drop";

/** Runs a rail change and shows what it returned, or its error. */
function change(run: () => Promise<Workspace>) {
  run().then(
    (w) => workbench().setWorkspace(w),
    (e: unknown) => workbench().showError(storeMessage(toCommandError(e).message)),
  );
}

/** Makes a drop's changes in the store, in order. When one fails, the rail is read again
 *  from the store, so it never shows a drop made only in part. */
async function apply(dragged: string, c: RailChange): Promise<Workspace> {
  try {
    if (c.group) await api.groupProjects(c.group, dragged);
    if (c.folder) await api.setProjectFolder(dragged, c.folder.id, c.folder.index);
    return await api.reorderProjects(c.order);
  } catch (e) {
    await Promise.all([api.listProjects(), api.listProjectFolders()]).then(
      ([projects, folders]) => workbench().setWorkspace({ connections: workbench().connections(), projects, project_folders: folders }),
      () => {},
    );
    throw e;
  }
}

/** A drop the rail would take: its change, and where it shows: a line at `line` pixels from
 *  the top of the rail's content, or the project or folder `into` (`project:<id>`,
 *  `folder:<id>`) outlined. */
interface RailTarget {
  change: RailChange;
  line?: number;
  into?: string;
}

/** The key of a project's or folder's element, as `RailTarget.into` names it. */
const keyOf = (el: Element) => {
  const project = el.getAttribute("data-project");
  return project ? `project:${project}` : `folder:${el.getAttribute("data-folder") ?? ""}`;
};

/** The spot at `y` among `items` (the rail's or an open folder's targets, top to bottom)
 *  when the pointer is in a gap between them: before the first one below it, else `last`
 *  after the last one. */
function gapSpot(items: Element[], y: number, last: RailSpot): { spot: RailSpot; element: Element | null } {
  for (const element of items) {
    if (element.getBoundingClientRect().bottom < y) continue;
    const id = element.getAttribute("data-project") ?? element.getAttribute("data-folder") ?? "";
    return { spot: { kind: element.hasAttribute("data-project") ? "project" : "folder", id, zone: "before" }, element };
  }
  return { spot: last, element: items.at(-1) ?? null };
}

/** The spot of the rail under the pointer, and the element that shows it. */
function spotAt(el: Element | null, at: Point): { spot: RailSpot; element: Element | null } | null {
  const hit = markedTarget(el, DROP);
  if (!hit) return null;
  const element = hit.element;
  const r = element.getBoundingClientRect();
  const zone = edgeZone(at.y - r.top, r.height);
  const project = element.getAttribute("data-project") ?? "";
  const folder = element.getAttribute("data-folder") ?? "";
  const targets = (scope: Element) => [...scope.querySelectorAll(`:scope > [${DROP}="project"], :scope > [${DROP}="folder"], :scope > [${DROP}="open"]`)];
  switch (hit.value) {
    case "project":
      return { spot: { kind: "project", id: project, zone }, element };
    case "folder":
      return { spot: { kind: "folder", id: folder, zone }, element };
    case "head":
      // The head of an open folder: above its middle before the folder, else into it.
      return { spot: { kind: "folder", id: folder, zone: at.y < r.top + r.height / 2 ? "before" : "inside" }, element };
    case "open": {
      // Between the projects of an open folder: next to them, in the folder.
      const squares = targets(element);
      const last = squares.at(-1)?.getAttribute("data-project");
      return last ? gapSpot(squares, at.y, { kind: "project", id: last, zone: "after" }) : null;
    }
    case "all":
      return { spot: { kind: "start" }, element };
    default:
      return gapSpot(targets(element), at.y, { kind: "end" });
  }
}

/** The column of project squares left of the navigator: All connections, then the projects
 *  and folders of projects in their saved order, then a square to make a new project.
 *  A project dragged onto another makes a folder of the two, as in Discord; dragged onto
 *  a folder it goes into it; dragged to the edge of a square it moves there (in an open
 *  folder, inside it), and to the end of the rail, out of its folder, last. */
export function ProjectRail({ projects, folders, active, txProjects, onSwitch, onNew, onMenu, onFolderMenu, onAddMenu }: Props) {
  const nav = useRef<HTMLElement>(null);
  const startDrag = useDrag();
  const [shown, setShown] = useState<RailTarget | null>(null);
  // The drop is read at each move from the rail as it is then.
  const latest = useRef({ projects, folders });
  latest.current = { projects, folders };

  const square = (key: string, current: boolean, tx: boolean) =>
    `project-square${current ? " active" : ""}${tx ? " tx" : ""}${key === "all" ? " all" : ""}`;

  /** The drop of `dragged` under the pointer, `null` where it changes nothing. */
  function targetAt(dragged: string, el: Element | null, at: Point): RailTarget | null {
    const rail = nav.current;
    const hit = rail && el && rail.contains(el) ? spotAt(el, at) : null;
    if (!rail || !hit) return null;
    const c = railDrop(latest.current.projects, latest.current.folders, dragged, hit.spot);
    if (!c) return null;
    // The end of a rail with no projects: above the square that adds one.
    const zone = "zone" in hit.spot ? hit.spot.zone : hit.element ? "after" : "before";
    const element = hit.element ?? rail.querySelector(".project-add");
    if (!element) return { change: c };
    if (zone === "inside") return { change: c, into: keyOf(element) };
    // Halfway into the gap above or below the element, in the rail's scrolled content.
    const r = element.getBoundingClientRect();
    const top = rail.getBoundingClientRect().top - rail.scrollTop;
    return { change: c, line: (zone === "before" ? r.top - 4 : r.bottom + 4) - top };
  }

  const drag = (p: Project) => (e: React.PointerEvent) =>
    startDrag<RailTarget>(e, {
      label: p.name,
      target: (el, at) => targetAt(p.id, el, at),
      show: setShown,
      drop: (target) => change(() => apply(p.id, target.change)),
    });

  const into = (key: string) => (shown?.into === key ? " drop-into" : "");

  const projectSquare = (p: Project) => (
    <button
      type="button"
      key={p.id}
      className={`${square(p.id, p.id === active, txProjects.has(p.id))}${p.shared ? " shared" : ""}${into(`project:${p.id}`)}`}
      data-project={p.id}
      data-drop="project"
      data-shared={p.shared ? "true" : undefined}
      title={p.shared ? t("{name} (shared through {path})", { name: p.name, path: p.shared.path }) : p.name}
      aria-pressed={p.id === active}
      style={{ background: p.color }}
      onPointerDown={drag(p)}
      onClick={() => onSwitch(p.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu(p, e);
      }}
    >
      {p.initials}
      {p.shared && <i className="shared-mark" aria-hidden />}
    </button>
  );

  const folderMenu = (f: ProjectFolder) => (e: React.MouseEvent) => {
    e.preventDefault();
    onFolderMenu(f, e);
  };
  const toggle = (f: ProjectFolder) => change(() => api.saveProjectFolder({ ...f, collapsed: !f.collapsed }));

  return (
    <nav ref={nav} className="project-rail" aria-label={t("Projects")} data-drop="rail">
      <button
        type="button"
        className={square("all", active === null, txProjects.has("all"))}
        data-project="all"
        data-drop="all"
        title={t("All connections")}
        aria-label={t("All connections")}
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
              className={`project-folder${holdsActive ? " active" : ""}${tx ? " tx" : ""}${into(`folder:${f.id}`)}`}
              data-folder={f.id}
              data-drop="folder"
              title={`${f.name}: ${item.projects.map((p) => p.name).join(", ")}`}
              aria-expanded={false}
              onClick={() => toggle(f)}
              onContextMenu={folderMenu(f)}
            >
              {item.projects.slice(0, 4).map((p) => (
                <i key={p.id} style={{ background: p.color }} />
              ))}
            </button>
          );
        }
        return (
          <div key={f.id} className={`project-folder-open${into(`folder:${f.id}`)}`} data-folder={f.id} data-drop="open">
            <button
              type="button"
              className="project-folder-head"
              data-folder={f.id}
              data-drop="head"
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
      <button type="button" className="project-add" title={t("New project")} aria-label={t("New project")} onClick={onNew} onContextMenu={onAddMenu}>
        <Icon name="plus" size={14} />
      </button>
      {shown?.line !== undefined && <i className="rail-drop-line" style={{ top: shown.line }} aria-hidden />}
    </nav>
  );
}
