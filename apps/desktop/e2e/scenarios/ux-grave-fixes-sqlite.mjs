// The "grave" fixes of the UX review, at the 1280x720 window the review used: a context
// submenu opened low in the navigator stays inside the window; the driver picker's tile names
// wrap on two lines and never overflow their tile; the Agents > Security levels are real radios
// that change the connection's access (and cannot go past read only on production).
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { openSubmenu } from "../menu-sub.mjs";

const DEV = "00000000-0000-4000-8000-0000000000a1";
const PROD = "00000000-0000-4000-8000-0000000000a2";
const TABLES = 40;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const accessOf = (name) =>
  `document.querySelector('.agents-connections tr[data-connection=${JSON.stringify(name)}] td:last-child')?.textContent`;
const savedAccess = (dir, name) => {
  const saved = readFileSync(join(dir, "connections.toml"), "utf8");
  return /agent_access = "(\w+)"/.exec(saved.split("[[connections]]").find((c) => c.includes(`name = "${name}"`)) ?? "")?.[1];
};

export function setup({ dir }) {
  const path = join(dir, "grave.db");
  const db = new DatabaseSync(path);
  for (let i = 0; i < TABLES; i++) db.exec(`CREATE TABLE t${String(i).padStart(2, "0")} (id INTEGER PRIMARY KEY, v TEXT);`);
  db.close();
  const connection = (id, name, extra) => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlite"
database = '${path}'
save_password = false
${extra}
`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 4\n${connection(DEV, "Grave db", 'agent_access = "read_write"')}${connection(PROD, "Grave prod", 'connection_type = "prod"\nagent_access = "read_only"')}`,
  );
}

/** The submenu of a table row near the bottom of a short window stays inside it. */
async function submenuInside(page) {
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 720, deviceScaleFactor: 1, mobile: false });
  await page.waitFor(`${row("Grave db")} !== undefined`);
  await page.eval(`${row("Grave db")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row(`t${TABLES - 1}`)} !== undefined`, 15_000);
  // The last table sits at the bottom of the navigator: the menu opens low, its submenu lower.
  await page.eval(`${row(`t${TABLES - 1}`)}.scrollIntoView({ block: "end" })`);
  await page.eval(`${row(`t${TABLES - 1}`)}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.eval(`document.getElementById('e2e-target').removeAttribute('id')`);
  await page.click(r.x + 40, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  assert.ok(await openSubmenu(page, "Data"), "the table menu has a Data submenu");
  await page.waitFor(`document.querySelector('.context-menu.submenu')?.style.visibility !== 'hidden'`);
  const box = await page.eval(`(() => {
    const r = document.querySelector('.context-menu.submenu').getBoundingClientRect();
    return { top: r.top, bottom: r.bottom, left: r.left, right: r.right, w: innerWidth, h: innerHeight };
  })()`);
  assert.ok(box.top >= 0 && box.bottom <= box.h, `submenu inside vertically: ${JSON.stringify(box)}`);
  assert.ok(box.left >= 0 && box.right <= box.w, `submenu inside horizontally: ${JSON.stringify(box)}`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}

/** Every tile name fits its tile: wrapped, clamped to two lines, never wider than the tile. */
async function driverTiles(page) {
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  await page.waitFor(`document.querySelectorAll('.driver-tile').length > 10`);
  const bad = await page.eval(`[...document.querySelectorAll('.driver-tile')].flatMap(tile => {
    const label = tile.querySelector(':scope > span:last-child');
    const style = getComputedStyle(label);
    const ok = style.whiteSpace === 'normal' && label.scrollWidth <= label.clientWidth
      && label.scrollHeight <= Math.ceil(parseFloat(style.lineHeight) * 2) + 1;
    return ok ? [] : [label.textContent + ' ' + style.whiteSpace + ' ' + label.scrollWidth + '/' + label.clientWidth + ' ' + label.scrollHeight];
  })`);
  assert.deepEqual(bad, [], "driver tile names that overflow");
  // Other buttons keep their single line.
  assert.equal(await page.eval(`getComputedStyle(document.querySelector('.driver-grid').closest('.modal')?.querySelector('button:not(.driver-tile)') ?? document.body).whiteSpace`), "nowrap");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.driver-grid') === null`);
}

async function agentSecurity(page, dir) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Agents");
  await page.eval(`document.querySelector('[data-agents-page="security"]').click()`);
  const level = (v) => `document.querySelector('.agent-level[data-level="${v}"] input')`;
  await page.waitFor(`${accessOf("Grave db")} === 'Read and write'`);
  assert.equal(await page.eval(`document.querySelectorAll('.agent-level input[type=radio]').length`), 3);
  assert.equal(await page.eval(`${level("read_write")}.checked`), true);
  // Reachable from the keyboard: the radios are real inputs in one group.
  assert.equal(await page.eval(`[...document.querySelectorAll('.agent-level input')].every(i => i.tabIndex >= 0 && i.name === 'agent-access')`), true);
  assert.equal(await page.eval(`document.querySelector('.agent-levels').getAttribute('role')`), "radiogroup");

  // Picking Read only changes the setting: the table, the file, and the app's own list.
  await page.eval(`${level("read_only")}.click()`);
  await page.waitFor(`${accessOf("Grave db")} === 'Read only'`);
  await page.waitFor(`document.querySelector('.agent-level[data-level="read_only"]').classList.contains('on')`);
  assert.equal(savedAccess(dir, "Grave db"), "read_only");
  // And back: nothing else of the connection is lost.
  await page.eval(`${level("read_write")}.click()`);
  await page.waitFor(`${accessOf("Grave db")} === 'Read and write'`);
  assert.equal(savedAccess(dir, "Grave db"), "read_write");
  await page.eval(`${level("off")}.click()`);
  await page.waitFor(`${accessOf("Grave db")} === 'Off'`);
  assert.equal(savedAccess(dir, "Grave db"), "off");
  await page.eval(`${level("read_write")}.click()`);
  await page.waitFor(`${accessOf("Grave db")} === 'Read and write'`);

  // A production connection: Read and write is disabled, Read only is the choice.
  await page.eval(`document.querySelector('.agents-connections tr[data-connection="Grave prod"]').click()`);
  await page.waitFor(`document.querySelector('.agent-detail header strong')?.textContent === 'Grave prod'`);
  assert.equal(await page.eval(`${level("read_write")}.disabled`), true);
  assert.equal(await page.eval(`${level("read_only")}.checked`), true);
  await page.eval(`${level("read_write")}.click()`);
  await page.sleep(300);
  assert.equal(await page.eval(accessOf("Grave prod")), "Read only · production");
  assert.equal(savedAccess(dir, "Grave prod"), "read_only");
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

export default async function (page, { dir }) {
  await submenuInside(page);
  await driverTiles(page);
  await agentSecurity(page, dir);
}
