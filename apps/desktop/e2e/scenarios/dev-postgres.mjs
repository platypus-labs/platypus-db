// The navigator and the data viewer against the dev PostgreSQL from docker/compose.yml.
// Skipped when it is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";

const NAME = "E2E PG";
const PORT = 55432;
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `${visibleTab}?.querySelector('.statusbar span')?.textContent ?? ''`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function skip() {
  return new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port: PORT, timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolve(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done(`no PostgreSQL on port ${PORT} (docker compose -f docker/compose.yml up -d)`));
    socket.on("timeout", () => done(`no PostgreSQL on port ${PORT}`));
  });
}

export function setup({ dir }) {
  // No saved password: the app asks for it, which this scenario answers.
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000002"
name = "${NAME}"
driver = "postgres"
host = "localhost"
port = ${PORT}
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
`,
  );
}

/** Waits until no statement runs (Cancel is disabled) and the status passes `test`. */
async function waitStatus(page, test) {
  const idle = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')?.disabled`;
  await page.waitFor(`(() => { const s = ${status}; return ${idle} && s !== '' && !s.includes('Executing') && (${test}); })()`, 15_000);
  return page.eval(status);
}

async function menu(page, selector) {
  const r = await page.rect(selector);
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  return page.eval(`[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`);
}

export default async function (page) {
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.querySelector(".twisty").click()`);

  // Password prompt.
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);

  // The current schema opens by itself; open the demo schema.
  await page.waitFor(`${row("demo")} !== undefined`, 15_000);
  await page.eval(`${row("demo")}.querySelector(".twisty").click()`);
  await page.waitFor(`${row("orders")} !== undefined`, 15_000);

  await page.eval(`${row("orders")}.id = 'e2e-orders'`);
  const items = await menu(page, "#e2e-orders");
  for (const item of ["Open data", "Open details", "Generate SELECT", "Copy name", "Refresh", "Drop…"]) {
    assert.ok(items.includes(item), `${item} in ${items}`);
  }
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent === 'Open data').click()`);

  // Data viewer: autorun, header sort.
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`);
  const first = await waitStatus(page, "s.includes('row')");
  assert.match(first, /rows/);

  const grid = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); s.textContent = ''; })()`);
  await page.click(grid.x + 80, grid.y + 14); // first column header
  await waitStatus(page, "true");
  assert.match(await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`), /ORDER BY \S+ ASC/);

  assert.equal(await page.eval(`${visibleTab}.querySelector('.viewer-bar input')`), null, "no WHERE bar");

  // Generate SELECT uses the navigator.select_limit setting (default 200).
  await menu(page, "#e2e-orders");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent === 'Generate SELECT').click()`);
  await page.waitFor(`/LIMIT 200;/.test(${visibleTab}?.querySelector('.cm-content')?.textContent ?? '')`);
}
