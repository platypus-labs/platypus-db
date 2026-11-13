// Renders docs/brand/social-preview.html to docs/brand/social-preview.png (1280×640), the
// image GitHub shows when the repository is linked (Settings › Social preview). Uses a
// Chromium-based browser in headless mode (Chrome, Edge or Chromium); set BROWSER to its
// executable if it is not found.
//
//   node scripts/social-preview.mjs
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const CANDIDATES = [
  process.env.BROWSER,
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
  "/usr/bin/microsoft-edge",
];

const browser = CANDIDATES.find((path) => path && existsSync(path));
if (!browser) {
  console.error("No Chromium-based browser found: set BROWSER to its executable.");
  process.exit(1);
}

const brand = join(dirname(dirname(fileURLToPath(import.meta.url))), "docs", "brand");
// A profile of its own: with the browser already open on the user's profile, a headless run on
// that profile exits at once without writing the screenshot.
const profile = mkdtempSync(join(tmpdir(), "social-preview-"));
execFileSync(browser, [
  `--user-data-dir=${profile}`,
  "--headless",
  "--disable-gpu",
  "--hide-scrollbars",
  "--force-device-scale-factor=1",
  "--window-size=1280,640",
  "--virtual-time-budget=3000",
  `--screenshot=${join(brand, "social-preview.png")}`,
  pathToFileURL(join(brand, "social-preview.html")).href,
]);
rmSync(profile, { recursive: true, force: true });
console.log("Wrote docs/brand/social-preview.png");
