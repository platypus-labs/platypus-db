/** What the Scripts panel derives from `git status`: no text and no calls, so it is tested alone. */
import type { GitFileStatus } from "../api";

/** The changed scripts by their relative path. */
export function statusByPath(files: GitFileStatus[]): Map<string, GitFileStatus> {
  return new Map(files.map((f) => [f.path, f]));
}

/** Whether a script under `folder` has changed: the folder is marked as well. */
export function changedInFolder(folder: string, files: GitFileStatus[]): boolean {
  return files.some((f) => f.path.startsWith(`${folder}/`));
}

/** The files a commit takes when the box of each is ticked, in the order of the list. */
export function selectedPaths(files: GitFileStatus[], unticked: ReadonlySet<string>): string[] {
  return files.filter((f) => !unticked.has(f.path)).map((f) => f.path);
}

/** The conflicting files of a merge that is waiting. */
export function conflicts(files: GitFileStatus[]): string[] {
  return files.filter((f) => f.badge === "U").map((f) => f.path);
}

/** Whether the author typed is complete: a name and something that looks like an email. */
export function validIdentity(name: string, email: string): boolean {
  return name.trim() !== "" && /^[^\s@]+@[^\s@]+$/.test(email.trim());
}
