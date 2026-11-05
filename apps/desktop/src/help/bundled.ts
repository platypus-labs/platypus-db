/**
 * What the app carries of its documentation, read from the repository when the frontend is
 * built: the pages of `docs/guide/` (and `docs/guide/databases/`), the PNG screenshots they
 * show, and the changelog. The guide opens offline from these, not from GitHub.
 */
import changelogText from "../../../../CHANGELOG.md?raw";
import { pageKeyOf } from "./pages";

export interface Guide {
  /** The Markdown of each page, by key (`connect`, `databases/postgres`). */
  pages: Record<string, string>;
  /** The address of each image, by its path in the repository (`docs/screenshots/palette.png`). */
  images: Record<string, string>;
}

const pageFiles = import.meta.glob("../../../../docs/guide/**/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
// The animated GIFs are left out: the hero alone is 1.7 MB.
const imageFiles = import.meta.glob("../../../../docs/screenshots/*.png", { query: "?url", import: "default", eager: true }) as Record<string, string>;

/** `../../../../docs/guide/connect.md` as the path from the repository root. */
const fromRoot = (file: string) => file.slice(file.indexOf("docs/"));

const bundled: Guide = {
  pages: Object.fromEntries(
    Object.entries(pageFiles).flatMap(([file, text]) => {
      const key = pageKeyOf(fromRoot(file));
      return key ? [[key, text]] : [];
    }),
  ),
  images: Object.fromEntries(Object.entries(imageFiles).map(([file, url]) => [fromRoot(file), url])),
};

let replaced: Guide | null = null;

/** The guide the dialog shows. */
export const guide = (): Guide => replaced ?? bundled;

/** The end-to-end tests show pages of their own in place of the bundled ones (`null`: the bundled again). */
export const replaceGuide = (pages: Record<string, string> | null) => {
  replaced = pages ? { pages, images: {} } : null;
};

/** The changelog of the repository. */
export const changelog = changelogText;
