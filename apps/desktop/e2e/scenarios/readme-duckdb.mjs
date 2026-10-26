// The README's screenshots of the data catalog, on a DuckDB field survey the app creates in a
// throwaway file (DuckDB keeps comments, SQLite does not): the object tab with the database
// comments, notes and tags of a table and its columns, then the schema documentation exported
// as HTML, shown in the window, with its ER diagram. Runs only with PLATYPUS_README_SHOTS set to
// the folder the pictures go to (docs/screenshots).
import { writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { ALT } from "../cdp.mjs";
import { navigatorMenu, openProject, prepare, skip, treeRow } from "./readme-sqlite.mjs";

export { skip };

const NAME = "Field survey";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;

const SCRIPT = `CREATE TABLE rivers (
  id INTEGER PRIMARY KEY,
  name VARCHAR NOT NULL,
  state VARCHAR NOT NULL,
  length_km DECIMAL(6, 1)
);
CREATE TABLE researchers (
  id INTEGER PRIMARY KEY,
  full_name VARCHAR NOT NULL,
  email VARCHAR NOT NULL UNIQUE,
  institution VARCHAR NOT NULL
);
CREATE TABLE platypuses (
  id INTEGER PRIMARY KEY,
  tag_code VARCHAR NOT NULL UNIQUE,
  nickname VARCHAR,
  sex VARCHAR NOT NULL CHECK (sex IN ('F', 'M')),
  born_year INTEGER,
  home_river_id INTEGER NOT NULL REFERENCES rivers (id)
);
CREATE TABLE surveys (
  id INTEGER PRIMARY KEY,
  river_id INTEGER NOT NULL REFERENCES rivers (id),
  researcher_id INTEGER NOT NULL REFERENCES researchers (id),
  started_at TIMESTAMP NOT NULL,
  water_temp_c DECIMAL(4, 1)
);
CREATE TABLE sightings (
  id INTEGER PRIMARY KEY,
  survey_id INTEGER NOT NULL REFERENCES surveys (id),
  platypus_id INTEGER REFERENCES platypuses (id),
  seen_at TIMESTAMP NOT NULL,
  behaviour VARCHAR NOT NULL,
  duration_s INTEGER NOT NULL
);
INSERT INTO rivers VALUES (1, 'Derwent', 'TAS', 215.0), (2, 'Snowy', 'NSW', 352.0), (3, 'Yarra', 'VIC', 242.0), (4, 'Mary', 'QLD', 291.0);
INSERT INTO researchers VALUES (1, 'Sara Greco', 'sara@survey.example', 'University of Tasmania'),
  (2, 'Tom Walsh', 'tom@survey.example', 'Monash University'), (3, 'Mia Nguyen', 'mia@survey.example', 'University of Queensland');
INSERT INTO platypuses SELECT i, 'AU-' || (1000 + i), CASE WHEN i <= 4 THEN ['Bubbles', 'Paddle', 'Pebble', 'Squeak'][i] END,
  CASE WHEN i % 2 = 0 THEN 'F' ELSE 'M' END, 2015 + i % 8, 1 + i % 4 FROM range(1, 61) t(i);
INSERT INTO surveys SELECT i, 1 + i % 4, 1 + i % 3, TIMESTAMP '2026-01-05 18:30:00' + INTERVAL (i * 3) DAY, 12.0 + i % 9
  FROM range(1, 121) t(i);
INSERT INTO sightings SELECT i, 1 + i % 120, CASE WHEN i % 7 <> 0 THEN 1 + i % 60 END,
  TIMESTAMP '2026-01-05 19:00:00' + INTERVAL (i % 120 * 3) DAY + INTERVAL (i % 50) MINUTE,
  ['foraging', 'swimming', 'resting', 'burrowing'][1 + i % 4], 20 + i % 300 FROM range(1, 1501) t(i);
COMMENT ON TABLE rivers IS 'Rivers walked by the survey';
COMMENT ON TABLE researchers IS 'People who lead the surveys';
COMMENT ON TABLE platypuses IS 'Animals fitted with a microchip, one row each';
COMMENT ON TABLE surveys IS 'One evening on one river';
COMMENT ON TABLE sightings IS 'An animal seen during a survey';
COMMENT ON COLUMN platypuses.tag_code IS 'Microchip code, as the reader prints it';
COMMENT ON COLUMN platypuses.sex IS 'F or M';
COMMENT ON COLUMN platypuses.born_year IS 'Estimated at the first capture';
COMMENT ON COLUMN platypuses.home_river_id IS 'River of the first capture';`;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2

[[projects]]
id = "00000000-0000-4000-9000-000000000051"
name = "Survey"
color = "#0d9488"
connections = ["00000000-0000-4000-8000-000000000051"]

[[connections]]
id = "00000000-0000-4000-8000-000000000051"
name = "${NAME}"
driver = "duckdb"
database = '${join(dir, "survey.duckdb")}'
save_password = false
connection_type = "dev"
`,
  );
  writeFileSync(join(dir, "settings.toml"), '[appearance]\ntheme = "platypus_dark"\n');
}

/** Creates the tables, their rows and comments from a query tab, then shows them in the
 *  navigator. */
async function createTables(page) {
  await page.eval(`${treeRow(NAME)}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-connection')?.removeAttribute('id')`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type(SCRIPT);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes('River of the first capture')`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("x", "KeyX", 88, ALT);
  const idle = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')?.disabled`;
  await page.waitFor(`${idle} && ${status} !== '' && !${status}.includes('Executing')`, 30_000);
  // Opening the tab may have read the empty database already: read it again.
  if (await page.eval(`${treeRow(NAME)}.getAttribute('aria-expanded') === 'true'`)) {
    await navigatorMenu(page, NAME, "Refresh");
  } else {
    await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  }
  await page.waitFor(`${treeRow("platypuses")} !== undefined`, 15_000);
}

/** Types `text` in the field `selector` of the object tab, then leaves it, which saves it. */
async function fill(page, selector, text) {
  const field = `${objectTab}.querySelector(${JSON.stringify(selector)})`;
  await page.eval(`${field}.focus()`);
  await page.type(text);
  await page.waitFor(`${field}.value === ${JSON.stringify(text)}`);
  await page.eval(`${field}.blur()`);
}

/** Adds `tags` (a comma-separated list) in the tag field `selector`. */
async function tag(page, selector, tags, count) {
  const field = `${objectTab}.querySelector(${JSON.stringify(`${selector} input`)})`;
  await page.eval(`${field}.focus()`);
  await page.type(`${tags},`);
  await page.waitFor(`${objectTab}.querySelectorAll(${JSON.stringify(`${selector} .tag-chip`)}).length === ${count}`);
  await page.eval(`${field}.blur()`);
}

/** Notes and tags on platypuses and its columns, beside the comments the database keeps. */
async function catalog(page, shot) {
  await navigatorMenu(page, "platypuses", "Open details");
  await page.waitFor(`${objectTab}?.querySelector('textarea[aria-label="Note"]') != null`, 15_000);
  await fill(page, 'textarea[aria-label="Note"]', "Filled by the nightly import from the chip readers");
  await page.waitFor(`${treeRow("platypuses")}.querySelector('.catalog-mark') !== null`);
  await tag(page, ".notes-panel .tag-input", "survey, core", 2);
  await page.eval(`[...${objectTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith('Columns')).click()`);
  await page.waitFor(`${objectTab}.querySelector('input[aria-label="Note of nickname"]') != null`, 15_000);
  await fill(page, 'input[aria-label="Note of nickname"]', "Given by the field team");
  await fill(page, 'input[aria-label="Note of born_year"]', "Check against the bill length");
  await tag(page, '.tag-input[aria-label="Tags of tag_code"]', "id", 1);
  await tag(page, '.tag-input[aria-label="Tags of home_river_id"]', "geo", 1);
  await page.waitFor(
    `[...${objectTab}.querySelectorAll('.meta-table tbody tr')].some(r => r.textContent.includes('Microchip code'))`,
  );
  await page.sleep(300);
  await shot("catalog");
}

/** The documentation of the schema as one HTML file, opened in the window in the light scheme. */
async function docs(page, dir, shot) {
  const dialog = `document.querySelector('.modal.docs-dialog')`;
  const button = (text) => `[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
  const file = join(dir, "survey.html");
  await page.eval(`window.__platypus.answer("file", ${JSON.stringify(file)})`);
  await navigatorMenu(page, "main", "Export documentation…");
  await page.waitFor(`${dialog} !== null`, 15_000);
  await page.eval(`${button("Choose file…")}.click()`);
  await page.waitFor(`${dialog}.querySelector('input[aria-label="File"]').value === ${JSON.stringify(file)}`);
  await page.eval(`${button("Export")}.click()`);
  await page.waitFor(`${dialog}.textContent.includes('Documentation written')`, 30_000);
  await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "light" }] });
  // Tall enough for the list of tables and the whole diagram.
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 1080, deviceScaleFactor: 1, mobile: false });
  await page.send("Page.navigate", { url: pathToFileURL(file).href });
  await page.waitFor(`location.protocol === 'file:' && document.querySelector('.mermaid svg') !== null`, 15_000);
  await page.sleep(500);
  await shot("schema-docs");
}

export default async function (page, { dir }) {
  const out = resolve(process.env.PLATYPUS_README_SHOTS);
  const shot = (name) => page.screenshot(join(out, `${name}.png`));
  await prepare(page);
  await openProject(page, 2, "Survey");
  await createTables(page);
  await catalog(page, shot);
  await docs(page, dir, shot);
}
