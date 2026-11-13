/**
 * What the app carries of its documentation, read from the repository when the frontend is
 * built: the pages of `docs/user/guide/` (and `docs/user/guide/databases/`), the PNG screenshots they
 * show, and the changelog. The guide opens offline from these, not from GitHub. Only the help
 * dialogs import this module, and they load at first use (`open.ts`): keep it out of the app's
 * main chunk.
 */
import changelogText from "../../../../CHANGELOG.md?raw";
import { pageKeyOf } from "./pages";
import { replacedPages } from "./replaced";

interface Guide {
  /** The Markdown of each page, by key (`connect`, `databases/postgres`), and of each file
   *  outside the guide, by its path (`docs/user/FEATURES.md`). */
  pages: Record<string, string>;
  /** The address of each image, by its path in the repository (`docs/screenshots/palette.png`). */
  images: Record<string, string>;
}

const pageFiles = import.meta.glob("../../../../docs/user/guide/**/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
// The reference the guide's pages link to: shown like a page, not listed in the index.
const referenceFiles = import.meta.glob(["../../../../docs/user/FEATURES.md", "../../../../docs/user/AGENTS.md", "../../../../docs/user/SHORTCUTS.md"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;
// The animated GIFs are left out: the hero alone is 1.7 MB.
const imageFiles = import.meta.glob("../../../../docs/screenshots/**/*.png", { query: "?url", import: "default", eager: true }) as Record<string, string>;

/** `../../../../docs/user/guide/connect.md` as the path from the repository root. */
const fromRoot = (file: string) => file.slice(file.indexOf("docs/"));

const bundled: Guide = {
  pages: Object.fromEntries([
    ...Object.entries(pageFiles).flatMap(([file, text]) => {
      const key = pageKeyOf(fromRoot(file));
      return key ? [[key, text]] : [];
    }),
    ...Object.entries(referenceFiles).map(([file, text]) => [fromRoot(file), text]),
  ]),
  images: Object.fromEntries(Object.entries(imageFiles).map(([file, url]) => [fromRoot(file), url])),
};

/** The guide the dialog shows. */
export const guide = (): Guide => {
  const pages = replacedPages();
  return pages ? { pages, images: {} } : bundled;
};

/** The changelog of the repository. */
export const changelog = changelogText;
