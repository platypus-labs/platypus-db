/**
 * Where a connection or folder dragged in the navigator lands, and the store change that
 * puts it there. Pure: the navigator and the Alt+Up/Alt+Down actions call it, and the unit
 * tests check it without the app.
 *
 * In a level of the tree the folders come first, then the connections; each keeps the
 * order the user gave it. A connection dropped onto another makes a folder of the two.
 */
import type { Project } from "../api";

/** A connection or folder of the navigator's list, dragged or under the pointer. */
export interface DragItem {
  kind: "connection" | "folder";
  id: string;
}

/** Above the row, onto it, or below it. */
export type DropZone = "before" | "into" | "after";

/** What the list arranges: a project, or All connections (no folders, nothing placed). */
export type Layout = Pick<Project, "connections" | "folders" | "placement">;

export type Arrange =
  /** Into `folder` (`null`: the root), just before the connection `before`, or last. */
  | { kind: "connection"; id: string; folder: string | null; before: string | null }
  /** Into `parent` (`null`: the root), just before the folder `before`, or last. */
  | { kind: "folder"; id: string; parent: string | null; before: string | null }
  /** A new folder holding `target`, `dragged` and then `more`, where `target` was. */
  | { kind: "group"; target: string; dragged: string; more?: string[] };

/** The layout of All connections: the saved list, in its order. */
export const listLayout = (connections: { id: string }[]): Layout => ({ connections: connections.map((c) => c.id) });

/** The zone of a row the pointer is in, `y` pixels from its top. A folder and, when they
 *  can be grouped, a connection take a drop onto their middle half; a connection dragged
 *  onto a folder goes into it, and a folder onto a connection goes where it is. */
export function dropZone(y: number, height: number, dragged: DragItem["kind"], target: DragItem["kind"], canGroup: boolean): DropZone {
  if (dragged !== target) return "into";
  if (dragged === "connection" && !canGroup) return y < height / 2 ? "before" : "after";
  if (y < height / 4) return "before";
  return y > (height * 3) / 4 ? "after" : "into";
}

const folderOf = (layout: Layout, connection: string) => layout.placement?.[connection] ?? null;

const parentOf = (layout: Layout, folder: string) => layout.folders?.find((f) => f.id === folder)?.parent ?? null;

/** The connections in `folder`, in order. */
const connectionsIn = (layout: Layout, folder: string | null) => layout.connections.filter((c) => folderOf(layout, c) === folder);

/** The folders in `parent`, in order. */
const foldersIn = (layout: Layout, parent: string | null) =>
  (layout.folders ?? []).filter((f) => (f.parent ?? null) === parent).map((f) => f.id);

/** Whether `folder` is `ancestor` or inside it. */
function isWithin(layout: Layout, folder: string | null, ancestor: string): boolean {
  const seen = new Set<string>();
  for (let at = folder; at !== null && !seen.has(at); at = parentOf(layout, at)) {
    if (at === ancestor) return true;
    seen.add(at);
  }
  return false;
}

/** What follows `id` among `siblings` once it is taken out: `before` of a drop after it. */
const nextOf = (siblings: string[], id: string) => siblings[siblings.indexOf(id) + 1] ?? null;

/** The `before` of `id` moved next to `target` among `siblings`, in `zone`; `undefined`
 *  when `id` is there already (`same`: it is among `siblings` now). */
function beside(siblings: string[], id: string, target: string, zone: "before" | "after", same: boolean): string | null | undefined {
  const others = siblings.filter((s) => s !== id);
  const before = zone === "before" ? target : nextOf(others, target);
  return same && nextOf(siblings, id) === before ? undefined : before;
}

/** The change a drop of `dragged` makes on `target` in `zone`; `target` `null` is the
 *  empty space below the rows, the end of the root. `null` when it changes nothing. */
export function dropChange(layout: Layout, dragged: DragItem, target: DragItem | null, zone: DropZone): Arrange | null {
  if (target === null) {
    return dragged.kind === "connection"
      ? connectionTo(layout, dragged.id, null, null)
      : folderTo(layout, dragged.id, null, null);
  }
  if (dragged.kind === "connection") {
    if (target.kind === "folder") return connectionTo(layout, dragged.id, target.id, null, true);
    if (dragged.id === target.id) return null;
    if (zone === "into") return { kind: "group", target: target.id, dragged: dragged.id };
    const folder = folderOf(layout, target.id);
    const same = folderOf(layout, dragged.id) === folder;
    const before = beside(connectionsIn(layout, folder), dragged.id, target.id, zone, same);
    return before === undefined ? null : { kind: "connection", id: dragged.id, folder, before };
  }
  if (target.kind === "connection") return folderTo(layout, dragged.id, folderOf(layout, target.id), null, true);
  if (isWithin(layout, target.id, dragged.id)) return null;
  if (zone === "into") return folderTo(layout, dragged.id, target.id, null, true);
  const parent = parentOf(layout, target.id);
  const same = parentOf(layout, dragged.id) === parent;
  const before = beside(foldersIn(layout, parent), dragged.id, target.id, zone, same);
  return before === undefined ? null : { kind: "folder", id: dragged.id, parent, before };
}

/** The changes a drop of `dragged` makes, in the order to apply them: several connections stay
 *  in the order given, whatever their folders were. None when nothing moves, or when they are
 *  dropped on one of themselves. */
export function dropChanges(layout: Layout, dragged: DragItem[], target: DragItem | null, zone: DropZone): Arrange[] {
  const [first] = dragged;
  if (!first) return [];
  if (dragged.length === 1) {
    const change = dropChange(layout, first, target, zone);
    return change ? [change] : [];
  }
  const ids = dragged.map((d) => d.id);
  if (target?.kind === "connection" && ids.includes(target.id)) return [];
  const move = (folder: string | null, before: string | null): Arrange[] => ids.map((id) => ({ kind: "connection", id, folder, before }));
  if (target === null) {
    const root = connectionsIn(layout, null);
    const inPlace = root.slice(-ids.length).join() === ids.join();
    return inPlace ? [] : move(null, null);
  }
  if (target.kind === "folder") {
    return ids.filter((id) => folderOf(layout, id) !== target.id).map((id) => ({ kind: "connection", id, folder: target.id, before: null }));
  }
  if (zone === "into") return [{ kind: "group", target: target.id, dragged: first.id, more: ids.slice(1) }];
  const folder = folderOf(layout, target.id);
  const others = connectionsIn(layout, folder).filter((c) => !ids.includes(c));
  return move(folder, zone === "before" ? target.id : nextOf(others, target.id));
}

/** A connection moved into `folder` before `before`; `null` when it is there already (in
 *  that folder, when `anywhere`; else also last in it). */
function connectionTo(layout: Layout, id: string, folder: string | null, before: string | null, anywhere = false): Arrange | null {
  const here = folderOf(layout, id) === folder;
  if (here && (anywhere || nextOf(connectionsIn(layout, folder), id) === before)) return null;
  return { kind: "connection", id, folder, before };
}

/** A folder moved into `parent` before `before`; `null` when it is there already, or when
 *  `parent` is inside it. */
function folderTo(layout: Layout, id: string, parent: string | null, before: string | null, anywhere = false): Arrange | null {
  if (isWithin(layout, parent, id)) return null;
  const here = parentOf(layout, id) === parent;
  if (here && (anywhere || nextOf(foldersIn(layout, parent), id) === before)) return null;
  return { kind: "folder", id, parent, before };
}

/** Alt+Up (`-1`) or Alt+Down (`1`): `item` swapped with the sibling above or below it, in
 *  its folder; `null` at the edge. */
export function stepChange(layout: Layout, item: DragItem, delta: -1 | 1): Arrange | null {
  const folder = item.kind === "connection" ? folderOf(layout, item.id) : parentOf(layout, item.id);
  const siblings = item.kind === "connection" ? connectionsIn(layout, folder) : foldersIn(layout, folder);
  const at = siblings.indexOf(item.id);
  if (at < 0 || at + delta < 0 || at + delta >= siblings.length) return null;
  // Down: before the one after the next, or last.
  const before = delta < 0 ? siblings[at - 1]! : (siblings[at + 2] ?? null);
  return item.kind === "connection"
    ? { kind: "connection", id: item.id, folder, before }
    : { kind: "folder", id: item.id, parent: folder, before };
}
