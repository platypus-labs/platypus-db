// Renders docs/brand/social-preview.html to docs/brand/social-preview.png (1280×640), the
// image GitHub shows when the repository is linked (Settings › Social preview). Needs
// Playwright with Chromium installed globally (`npm i -g playwright`, then
// `npx playwright install chromium`): the project does not depend on it.
//
//   node scripts/social-preview.mjs
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const { chromium } = createRequire(import.meta.url)("playwright");

const brand = join(dirname(dirname(fileURLToPath(import.meta.url))), "docs", "brand");
const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 640 } });
  await page.goto(pathToFileURL(join(brand, "social-preview.html")).href);
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: join(brand, "social-preview.png") });
} finally {
  await browser.close();
}
