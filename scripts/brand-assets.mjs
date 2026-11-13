// Regenerates every copy of the PlatypusDB logos from the masters in docs/brand/, so a new logo
// is one file to replace and one command to run:
//
//   docs/brand/app-icon.png               the app icon (square, 1024×1024, transparent): the
//                                         installers' icons, the top bar, the site's favicon
//   docs/brand/logo-only-1024.png         the mascot, the full platypus (1024×1024)
//   docs/brand/platypus-db-on-light.png   mark and wordmark for light backgrounds (1600×400)
//   docs/brand/platypus-db-on-dark.png    mark and wordmark for dark backgrounds (1600×400)
//   docs/brand/parts/*.png                the mascot in parts, layers of the same 1024 canvas: the
//                                         animated platypus of the site's home page
//
//   node scripts/brand-assets.mjs
//
// It needs the desktop app's npm packages (npm --prefix apps/desktop ci) for `tauri icon`.
// The avatar and the social preview are rendered from HTML by scripts/social-preview.mjs.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const at = (path) => join(root, ...path.split("/"));
const tauri = at("apps/desktop/node_modules/@tauri-apps/cli/tauri.js");
const icons = "apps/desktop/src-tauri/icons";

// Runs `tauri icon` on a master; with sizes, only those PNGs are written.
function tauriIcon(master, output, sizes = []) {
  const png = sizes.flatMap((size) => ["--png", String(size)]);
  execFileSync(process.execPath, [tauri, "icon", at(master), "--output", output, ...png], { stdio: "inherit" });
}

function copy(from, ...targets) {
  for (const target of targets) {
    copyFileSync(at(from), at(target));
    console.log(`${from} -> ${target}`);
  }
}

// The app icon: the installers' set, then the small copies the UI and the site use.
tauriIcon("docs/brand/app-icon.png", at(icons));
// The bundle targets only desktop platforms: the mobile sets are not kept.
for (const mobile of ["android", "ios"]) rmSync(at(`${icons}/${mobile}`), { recursive: true, force: true });
copy(`${icons}/64x64.png`, "apps/desktop/src/assets/brand/platypus-icon.png");
copy(`${icons}/128x128.png`, "site/img/icon.png", "docs/mockups/site/base/img/icon.png");

// The mascot: full size on the site, 64 px on the app's lock screen.
copy("docs/brand/logo-only-1024.png", "site/img/mark.png", "docs/mockups/site/base/img/mark.png");
const scratch = mkdtempSync(join(tmpdir(), "brand-assets-"));
try {
  tauriIcon("docs/brand/logo-only-1024.png", scratch, [64]);
  copyFileSync(join(scratch, "64x64.png"), at("apps/desktop/src/assets/brand/platypus-mark.png"));
  copy("apps/desktop/src/assets/brand/platypus-mark.png", "docs/mockups/assets/img/platypus-mark.png");
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

// The mascot in parts: the home page of the site and its mock.
for (const part of readdirSync(at("docs/brand/parts")).filter((name) => name.endsWith(".png"))) {
  copy(`docs/brand/parts/${part}`, `site/img/parts/${part}`, `docs/mockups/site/base/img/parts/${part}`);
}

// The banners: the welcome screen and the mockups.
for (const theme of ["light", "dark"]) {
  const banner = `platypus-db-on-${theme}.png`;
  copy(`docs/brand/${banner}`, `apps/desktop/src/assets/brand/${banner}`, `docs/mockups/assets/img/${banner}`);
}
