/**
 * Which page of the bundled guide a link or an image points to, the order and titles of the
 * pages, and the section of the changelog for a version. No app imports: the unit tests load
 * it as it is.
 */

/** Where the guide lives in the repository; page keys are the paths below it, without `.md`. */
export const GUIDE_DIR = "docs/guide/";

/** The page the guide opens on, and the one of the databases' own index. */
export const START_PAGE = "README";

/** The pages of the guide in reading order; other pages follow, by name. */
const ORDER = [
  "README",
  "getting-started",
  "connect",
  "queries",
  "edit-data",
  "import-export",
  "safety",
  "projects-sharing",
  "agents",
  "troubleshooting",
  "databases/README",
];

/** The repository path of page `key`; a key that already is a path (a file outside the guide) stays as it is. */
export const pagePath = (key: string) => (key.endsWith(".md") ? key : `${GUIDE_DIR}${key}.md`);

/** `path` made of its `.` and `..` parts, from the repository root; `null` when it climbs out of it. */
function normalize(path: string): string | null {
  const out: string[] = [];
  for (const part of path.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (out.pop() === undefined) return null;
    } else out.push(part);
  }
  return out.join("/");
}

/** The repository path `href` names when written in the file `from`; `null` if it leaves the repository. */
export function resolvePath(from: string, href: string): string | null {
  const dir = from.includes("/") ? from.slice(0, from.lastIndexOf("/") + 1) : "";
  const trailing = href.endsWith("/") ? "/" : "";
  const path = normalize(href.startsWith("/") ? href : dir + href);
  return path === null ? null : path + (path !== "" ? trailing : "");
}

/** The text of page `key`, or the path of the page that is missing (a page the guide does not have yet, a
 *  file outside it): the dialog says so in a plain message. */
export function pageSource(pages: Record<string, string>, key: string): { text: string } | { missing: string } {
  const text = Object.hasOwn(pages, key) ? pages[key] : undefined;
  return text === undefined ? { missing: pagePath(key) } : { text };
}

export type LinkTarget =
  /** Another page of the guide (`key`) or a file outside it (`key` is `null`), at `anchor`. */
  | { kind: "page"; key: string | null; path: string; anchor: string | null }
  /** A heading of the page shown. */
  | { kind: "anchor"; anchor: string }
  | { kind: "external"; url: string }
  /** Something that cannot be followed: a `javascript:` address, a file that is not a page. */
  | { kind: "none" };

const SCHEME = /^([a-z][a-z0-9+.-]*):/i;

const decode = (text: string) => {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
};

/** The page key of a repository path inside the guide (`docs/guide/databases/` is its README), else `null`. */
export function pageKeyOf(path: string): string | null {
  if (!path.startsWith(GUIDE_DIR) && `${path}/` !== GUIDE_DIR) return null;
  const rest = path.slice(GUIDE_DIR.length);
  if (rest === "" || rest.endsWith("/")) return `${rest}${START_PAGE}`;
  return rest.endsWith(".md") ? rest.slice(0, -3) : rest.includes(".") ? null : rest;
}

/** What a link written in the page `from` leads to. */
export function classifyLink(from: string, href: string): LinkTarget {
  const trimmed = href.trim();
  const scheme = SCHEME.exec(trimmed)?.[1]?.toLowerCase();
  if (scheme) return scheme === "http" || scheme === "https" || scheme === "mailto" ? { kind: "external", url: trimmed } : { kind: "none" };
  const hash = trimmed.indexOf("#");
  const file = hash < 0 ? trimmed : trimmed.slice(0, hash);
  const anchor = hash < 0 || hash === trimmed.length - 1 ? null : decode(trimmed.slice(hash + 1)).toLowerCase();
  if (file === "") return anchor ? { kind: "anchor", anchor } : { kind: "none" };
  const path = resolvePath(pagePath(from), decode(file));
  if (path === null) return { kind: "none" };
  const isPage = path.endsWith("/") || path.endsWith(".md") || !/\.[a-z0-9]+$/i.test(path);
  if (!isPage) return { kind: "none" };
  return { kind: "page", key: pageKeyOf(path), path: path.endsWith("/") ? `${path}README.md` : path, anchor };
}

/** The repository path of an image written in the page `from`, or `null` for one that is not a file of
 *  the repository (an address: the guide works offline, so it is left out). */
export function imagePath(from: string, src: string): string | null {
  if (SCHEME.test(src.trim()) || src.trim().startsWith("//")) return null;
  return resolvePath(pagePath(from), decode(src.trim().split(/[?#]/)[0]!));
}

/** The anchor GitHub gives a heading: lower case, punctuation dropped, spaces as hyphens. */
export function slugOf(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M}\s_-]/gu, "")
    .trim()
    .replace(/\s/g, "-");
}

/** Gives each heading its anchor; a repeated one gets `-1`, `-2`, ... as on GitHub. */
export function slugger() {
  const seen = new Map<string, number>();
  return (text: string) => {
    const slug = slugOf(text);
    const n = seen.get(slug) ?? 0;
    seen.set(slug, n + 1);
    return n === 0 ? slug : `${slug}-${n}`;
  };
}

/** The keys sorted for the index: reading order first, then databases and the rest by name. */
export function sortedKeys(keys: string[]): string[] {
  const rank = (k: string) => {
    const at = ORDER.indexOf(k);
    if (at >= 0) return at;
    return k.startsWith("databases/") ? ORDER.length : ORDER.length + 1;
  };
  return [...keys].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}

/** The title of a page: its first `# heading`, else its name. */
export function titleOf(source: string, key: string): string {
  const match = /^#\s+(.+?)\s*#*\s*$/m.exec(source.replace(/```[\s\S]*?```/g, ""));
  const name = key.slice(key.lastIndexOf("/") + 1);
  return match?.[1]?.replace(/[`*_]/g, "") ?? (name === START_PAGE ? "Guide" : name);
}

export interface ChangelogSection {
  /** The heading as written: `[0.4.1] - 2026-11-04`. */
  heading: string;
  body: string;
  /** Whether it is the section of the version asked for, not the unreleased one. */
  exact: boolean;
}

/** The section of the changelog for `version`; when it has none, the unreleased one; `null` for neither. */
export function changelogSection(text: string, version: string): ChangelogSection | null {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const find = (matches: (heading: string) => boolean) => {
    const start = lines.findIndex((l) => l.startsWith("## ") && matches(l.slice(3).trim()));
    if (start < 0) return null;
    let end = lines.findIndex((l, i) => i > start && l.startsWith("## "));
    if (end < 0) end = lines.length;
    return { heading: lines[start]!.slice(3).trim(), body: lines.slice(start + 1, end).join("\n").trim() };
  };
  const exact = find((h) => {
    const bare = h.replace(/^\[?v?/, "");
    return bare === version || bare.startsWith(`${version}]`) || bare.startsWith(`${version} `);
  });
  if (exact) return { ...exact, exact: true };
  const unreleased = find((h) => /^\[?unreleased\]?/i.test(h));
  return unreleased ? { ...unreleased, exact: false } : null;
}
