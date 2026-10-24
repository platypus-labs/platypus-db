import type { Project } from "../api";
import { Icon } from "../ui/Icon";

interface Props {
  projects: Project[];
  /** The active project's id, `null` for All connections. */
  active: string | null;
  /** Projects with an open transaction in one of their tabs, by `projectKey`. */
  txProjects: Set<string>;
  onSwitch: (id: string | null) => void;
  onNew: () => void;
  onMenu: (project: Project, e: React.MouseEvent) => void;
}

/** The column of project squares left of the navigator: All connections, then the projects
 *  in their saved order, then a square to make a new one. */
export function ProjectRail({ projects, active, txProjects, onSwitch, onNew, onMenu }: Props) {
  const square = (key: string, current: boolean, tx: boolean) =>
    `project-square${current ? " active" : ""}${tx ? " tx" : ""}${key === "all" ? " all" : ""}`;
  return (
    <nav className="project-rail" aria-label="Projects">
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
      {projects.map((p) => (
        <button
          type="button"
          key={p.id}
          className={square(p.id, p.id === active, txProjects.has(p.id))}
          data-project={p.id}
          title={p.name}
          aria-pressed={p.id === active}
          style={{ background: p.color }}
          onClick={() => onSwitch(p.id)}
          onContextMenu={(e) => {
            e.preventDefault();
            onMenu(p, e);
          }}
        >
          {p.initials}
        </button>
      ))}
      <button type="button" className="project-add" title="New project" onClick={onNew}>
        <Icon name="plus" size={14} />
      </button>
    </nav>
  );
}
