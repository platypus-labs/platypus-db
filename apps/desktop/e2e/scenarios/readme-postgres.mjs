// The README's screenshots that need the dev PostgreSQL from docker/compose.yml: Explain
// analyze with the heatmap and the index suggestion, then the query manager. Runs only with
// PLATYPUS_README_SHOTS set to the folder the pictures go to (docs/screenshots), and with the
// database running.
import { writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { CTRL, SHIFT } from "../cdp.mjs";
import { skip as noPostgres } from "./dev-postgres.mjs";
import { wide } from "./readme-sqlite.mjs";

export async function skip() {
  return process.env.PLATYPUS_README_SHOTS ? noPostgres() : "PLATYPUS_README_SHOTS not set";
}

const NAME = "Local PG";
const queryTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;
const row = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000041"
name = "${NAME}"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
connection_type = "dev"
`,
  );
}

/** Replaces the editor's text with `sql`. */
async function setEditor(page, sql) {
  await page.eval(`${queryTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`[...${queryTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
}

export default async function (page) {
  const out = resolve(process.env.PLATYPUS_README_SHOTS);
  await wide(page);
  await page.waitFor(`${row} !== undefined`);
  await page.eval(`${row}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`document.querySelector('.modal') === null`, 15_000);
  await page.eval(`${row}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.waitFor(`${queryTab} !== undefined`, 15_000);

  // A few statements first, for the query manager to list.
  await setEditor(
    page,
    "SELECT count(*) FROM demo.big_table;\nSELECT * FROM demo.customers ORDER BY id LIMIT 100;\n" +
      "SELECT status, count(*) FROM demo.orders GROUP BY status;\nSELECT * FROM demo.customer_totals LIMIT 20;",
  );
  await page.eval(`(() => { const s = ${queryTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`[...${queryTab}.querySelectorAll('.toolbar button')].find(b => b.textContent.includes('Script')).click()`);
  const idle = `[...${queryTab}.querySelectorAll('.toolbar button')].find(b => b.textContent.includes('Cancel'))?.disabled`;
  await page.waitFor(`${idle} && /rows?$/.test(${queryTab}.querySelector('.statusbar span')?.textContent ?? '')`, 30_000);

  await setEditor(
    page,
    "SELECT c.country, count(*) AS orders, sum(o.total) AS revenue\nFROM demo.orders o\n" +
      "JOIN demo.customers c ON c.id = o.customer_id\nWHERE o.status = 'paid'\nGROUP BY c.country\nORDER BY revenue DESC",
  );
  await page.eval(`${queryTab}.querySelector('.cm-editor').id = 'e2e-editor'`);
  const r = await page.rect("#e2e-editor");
  await page.click(r.x + r.w / 2, r.y + 10, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Explain analyze')).click()`);
  await page.waitFor(`${explainTab}?.querySelector('.explain-suggestion') != null`, 15_000);
  await page.sleep(300);
  await page.screenshot(join(out, "explain.png"));

  await page.key("M", "KeyM", 77, CTRL | SHIFT);
  await page.waitFor(`document.querySelectorAll('.query-manager tbody tr').length > 3`);
  // Without the metadata requests: the statements alone.
  await page.eval(`document.querySelector('.query-manager-bar input[type=checkbox]').click()`);
  // The trace of the count over a million rows: network, server and fetch.
  const rows = `[...document.querySelectorAll('.query-manager tbody tr')]`;
  await page.waitFor(`${rows}.some(r => r.textContent.includes('big_table'))`);
  await page.eval(`${rows}.find(r => r.textContent.includes('big_table')).click()`);
  await page.waitFor(`document.querySelector('.query-trace') !== null`);
  await page.eval(`document.activeElement?.blur()`);
  await page.sleep(300);
  await page.screenshot(join(out, "query-manager.png"));
}
