// Complete user journey 2, import from DBeaver (docs/dev/COLLAUDO.md 5.2): a DBeaver project export
// (folders, a production connection, an SSH one, files) imported from the first-start screen;
// imported again (no duplicates: every row "already saved", nothing to import); a connection
// moved to a new folder, one marked production, one read-only; the app restarted (folders,
// positions, types and read-only kept); a write on production asks to confirm, a write on the
// read-only one is refused. Run in English, in Italian and keyboard only.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { savedConnections } from "../import-helpers.mjs";
import {
  restartApp, fmt, journey, keys, palette, pressButton, runSql, screenIsItalian, setCheck, setSelect, tabTo, treeMenu, writeZip,
} from "../journey-day.mjs";
import { setLanguage } from "../journey.mjs";
import { visibleTab } from "../qa-jrn.mjs";

const PROJECT = "Journey DBeaver";
const dialog = `document.querySelector('.import-dialog')`;
const labels = `[...document.querySelectorAll('.tree-row')].map(r => [r.querySelector('.tree-label')?.textContent, parseInt(r.style.paddingLeft)])`;
const tabs = `[...document.querySelectorAll('.tab')].map(t => t.textContent)`;
const file = (dir, name) => join(dir, `jrn-${name}.db`);

function seed(path) {
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE s_jrn_orders (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO s_jrn_orders VALUES (1, 'a'), (2, 'b');");
  db.close();
}

const orders = (path) => {
  const db = new DatabaseSync(path, { readOnly: true });
  try {
    return db.prepare("SELECT group_concat(id || v) AS r FROM s_jrn_orders").get().r;
  } finally {
    db.close();
  }
};

export function setup({ dir }, mode) {
  setLanguage(dir, mode);
  for (const name of ["prod", "dev", "replica"]) seed(file(dir, name));
  const sqlite = (name, folder, type) => ({
    provider: "generic",
    driver: "sqlite_jdbc",
    name: `jrn-${name}`,
    ...(folder ? { folder } : {}),
    configuration: { url: `jdbc:sqlite:${file(dir, name)}`, type },
  });
  const sources = {
    folders: { Clienti: {}, "Clienti/Prod": { parent: "Clienti" }, Remoti: {} },
    connections: {
      "sqlite-prod": sqlite("prod", "Clienti/Prod", "prod"),
      "sqlite-replica": sqlite("replica", "Clienti", "test"),
      "sqlite-dev": sqlite("dev", null, "dev"),
      "pg-ssh": {
        provider: "postgresql",
        driver: "postgres-jdbc",
        name: "jrn-remote",
        folder: "Remoti",
        configuration: {
          host: "db.example.com",
          port: "5432",
          database: "app",
          user: "me",
          type: "dev",
          handlers: { ssh_tunnel: { enabled: true, properties: { host: "bastion.example.com", port: "22", user: "ops", authType: "PASSWORD" } } },
        },
      },
    },
  };
  writeZip(join(dir, "journey.dbp"), { [`${PROJECT}/.dbeaver/data-sources.json`]: JSON.stringify(sources) });
}

/** Opens the import dialog from the welcome page and reads the export. */
async function readExport(j, path) {
  const { page } = j;
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(path)})`);
  const card = `el.classList.contains('welcome-card') && el.textContent.includes(${JSON.stringify(j.t("Import connections"))})`;
  if (j.kb) {
    await tabTo(j, card);
    await keys.enter(page);
  } else await page.eval(`[...document.querySelectorAll('.welcome-card')].find(el => ${card}).click()`);
  await page.waitFor(`${dialog} !== null`, 10_000);
  await screenIsItalian(page, j.mode, "the import dialog");
  if (j.kb) {
    await tabTo(j, `el.classList.contains('import-choose')`);
    await keys.enter(page);
  } else await page.eval(`${dialog}.querySelector('.import-choose').click()`);
  await page.waitFor(`${dialog}.querySelectorAll('.import-row.connection').length === 4`, 15_000);
}

export async function run(page, ctx, mode) {
  const j = journey(page, mode);
  const lang = mode === "it" ? "it" : "en";
  assert.equal(await page.eval(`document.documentElement.lang`), lang);
  await screenIsItalian(page, mode, "the first-start screen", ["PlatypusDB"]);

  // 1. The export, read from the first-start screen: the preview first.
  await readExport(j, join(ctx.dir, "journey.dbp"));
  assert.deepEqual(await page.eval(`[...${dialog}.querySelectorAll('.import-row.folder')].map(r => r.textContent.trim())`), ["Clienti", "Prod", "Remoti"]);
  assert.deepEqual(
    await page.eval(`[...${dialog}.querySelectorAll('.import-row.connection .import-name')].map(r => r.textContent).sort()`),
    ["jrn-dev", "jrn-prod", "jrn-remote", "jrn-replica"],
  );
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.type-badge.prod').length`), 1, "the production connection is not marked in the preview");
  await screenIsItalian(page, mode, "the import preview");
  await pressButton(j, dialog, "Import {n}", { n: 4 });
  await page.waitFor(`${dialog} === null`, 15_000);
  const imported = savedConnections(ctx.dir);
  assert.deepEqual(Object.keys(imported).sort(), ["jrn-dev", "jrn-prod", "jrn-remote", "jrn-replica"]);
  assert.equal(imported["jrn-prod"].connection_type, "prod");
  assert.equal(imported["jrn-remote"].driver, "postgres");
  assert.match(readFileSync(join(ctx.dir, "connections.toml"), "utf8"), /bastion\.example\.com/, "the SSH tunnel was not imported");
  // The project, shown.
  await page.waitFor(`document.querySelector('.project-square[title=${JSON.stringify(PROJECT)}]') !== null`, 10_000);
  if ((await page.eval(`document.querySelector('.project-square.active')?.title`)) !== PROJECT) {
    if (j.kb) await page.key("2", "Digit2", 50, CTRL);
    else await page.eval(`document.querySelector('.project-square[title=${JSON.stringify(PROJECT)}]').click()`);
  }
  await page.waitFor(`document.querySelector('.project-square.active')?.title === ${JSON.stringify(PROJECT)}`, 10_000);
  await page.waitFor(`document.querySelectorAll('.tree-row').length >= 7`, 10_000);
  const first = (await page.eval(labels)).map(([l]) => l);
  for (const want of ["Clienti", "Prod", "Remoti", "jrn-prod", "jrn-replica", "jrn-remote", "jrn-dev"]) assert.ok(first.includes(want), `${want} in ${first}`);
  await screenIsItalian(page, mode, "the navigator after the import");

  // 2. Again: nothing new, every row already saved.
  await readExport(j, join(ctx.dir, "journey.dbp"));
  const saved = j.t("already saved");
  assert.equal(await page.eval(`[...${dialog}.querySelectorAll('.import-row.connection')].filter(r => r.textContent.includes(${JSON.stringify(saved)})).length`), 4);
  assert.equal(await page.eval(`${dialog}.querySelectorAll('.import-row.connection input:checked, .import-row.connection input[type=checkbox]:checked').length`), 0);
  assert.equal(await page.eval(`${dialog}.querySelector('.actions .primary').disabled`), true, "an import of nothing is offered");
  await screenIsItalian(page, mode, "the second import");
  if (j.kb) await keys.escape(page);
  else await pressButton(j, dialog, "Cancel");
  await page.waitFor(`${dialog} === null`);
  assert.equal(Object.keys(savedConnections(ctx.dir)).length, 4, "the second import duplicated connections");

  // 3. Organise: jrn-dev into a new folder, jrn-dev production, jrn-replica read-only.
  await treeMenu(j, ["jrn-dev"], "Move to folder", "New folder…");
  await page.waitFor(`document.querySelector('.name-dialog') !== null`, 5_000);
  await screenIsItalian(page, mode, "the new folder's name");
  if (j.kb) await page.waitFor(`document.activeElement === document.querySelector('.name-dialog input')`, 3_000);
  else await page.eval(`document.querySelector('.name-dialog input').focus()`);
  await page.type("Archivio");
  if (j.kb) await keys.enter(page);
  else await page.eval(`document.querySelector('.name-dialog button[type=submit]').click()`);
  await page.waitFor(`[...document.querySelectorAll('.tree-row .tree-label')].some(l => l.textContent === 'Archivio')`, 10_000);
  await editConnection(j, "jrn-dev", async () => {
    await setSelect(j, ".connection-dialog", "Environment", "prod");
  });
  await editConnection(j, "jrn-replica", async () => {
    await setCheck(j, ".connection-dialog", "Read-only", true);
  });
  const organised = savedConnections(ctx.dir);
  assert.equal(organised["jrn-dev"].connection_type, "prod");
  assert.equal(organised["jrn-replica"].read_only, "true");
  assert.equal(organised["jrn-prod"].connection_type, "prod");
  await page.waitFor(`document.querySelectorAll('.tree-row .type-badge.prod').length === 2`, 5_000);
  const tree = await page.eval(labels);
  const names = tree.map(([l]) => l);
  assert.ok(names.indexOf("Archivio") >= 0 && names[names.indexOf("Archivio") + 1] === "jrn-dev", `jrn-dev is not in Archivio: ${names}`);
  await screenIsItalian(page, mode, "the organised navigator");

  // 4. Restart: projects, folders, positions, types and read-only come back.
  await palette(j, fmt(j, "Open SQL editor: {name}", { name: "jrn-prod" }));
  await page.waitFor(`document.querySelectorAll('.tab').length === 1`, 10_000);
  await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('jrn-prod') || (localStorage.getItem('platypus.workspace.v2') ?? '') !== ''`);
  const openTabs = await page.eval(tabs);
  await restartApp(page, ctx, { ready: `document.querySelector('.tree-row') !== null` });
  await page.waitFor(`document.querySelector('.project-square.active')?.title === ${JSON.stringify(PROJECT)}`, 15_000);
  await page.waitFor(`[...document.querySelectorAll('.tree-row .tree-label')].some(l => l.textContent === 'Archivio')`, 15_000);
  assert.deepEqual(await page.eval(labels), tree);
  await page.waitFor(`document.querySelectorAll('.tab').length === ${openTabs.length}`, 15_000);
  assert.deepEqual(await page.eval(tabs), openTabs);
  assert.equal(await page.eval(`document.querySelectorAll('.tree-row .type-badge.prod').length`), 2);
  const after = savedConnections(ctx.dir);
  assert.equal(after["jrn-replica"].read_only, "true");
  assert.equal(after["jrn-dev"].connection_type, "prod");

  // 5. A write on production asks first; on the read-only replica it is refused.
  await palette(j, fmt(j, "Open SQL editor: {name}", { name: "jrn-dev" }));
  await runSql(j, "DELETE FROM s_jrn_orders WHERE id = 1");
  await page.waitFor(`document.querySelector('.modal.guard') !== null`, 20_000);
  assert.ok(await page.eval(`document.querySelector('.modal.guard h2').textContent`).then((x) => x === fmt(j, "Changes on {name} (production)", { name: "jrn-dev" })));
  await screenIsItalian(page, mode, "the production confirmation");
  assert.equal(await page.eval(`document.activeElement.textContent.trim()`), j.t("Cancel"), "the confirmation does not start on Cancel");
  await keys.enter(page);
  await page.waitFor(`document.querySelector('.modal.guard') === null`);
  assert.equal(orders(file(ctx.dir, "dev")), "1a,2b", "the cancelled change reached the file");
  // Confirmed, it runs (in the transaction production opens): rolled back after.
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.modal.guard') !== null`, 10_000);
  await pressButton(j, `document.querySelector('.modal.guard')`, "Run");
  await page.waitFor(`document.querySelector('.modal.guard') === null`);
  const txLabel = j.t("Transaction open {time}").split("{time}")[0].trim();
  await page.waitFor(`${visibleTab}.querySelector(".toolbar")?.textContent.includes(${JSON.stringify(txLabel)})`, 20_000);
  await runSql(j, "ROLLBACK");
  await page.waitFor(`!${visibleTab}.querySelector(".toolbar")?.textContent.includes(${JSON.stringify(txLabel)})`, 20_000);
  assert.equal(orders(file(ctx.dir, "dev")), "1a,2b");

  await palette(j, fmt(j, "Open SQL editor: {name}", { name: "jrn-replica" }));
  await runSql(j, "UPDATE s_jrn_orders SET v = 'x' WHERE id = 1");
  await page.waitFor(`${visibleTab}.querySelector('.notice.warn') !== null || document.querySelector('.modal.guard') !== null`, 20_000);
  assert.equal(await page.eval(`document.querySelector('.modal.guard') !== null`), false, "the read-only connection asked instead of refusing");
  assert.match(await page.eval(`${visibleTab}.querySelector('.notice.warn').textContent`), /read-only|sola lettura/i);
  await screenIsItalian(page, mode, "the read-only refusal");
  assert.equal(orders(file(ctx.dir, "replica")), "1a,2b");
  assert.equal(orders(file(ctx.dir, "prod")), "1a,2b");
}

/** Edits the connection `name` through its menu (Edit…), `change` fills the dialog, then Save. */
async function editConnection(j, name, change) {
  const { page } = j;
  await treeMenu(j, [name], "Edit…");
  await page.waitFor(`document.querySelector('.connection-dialog') !== null`, 10_000);
  // The safety settings are on the second page of the form.
  await pressButton(j, `document.querySelector('.connection-dialog')`, "Appearance and safety");
  await change();
  await screenIsItalian(page, j.mode, "the connection form's safety page");
  await pressButton(j, `document.querySelector('.connection-dialog')`, "Save");
  await page.waitFor(`document.querySelector('.connection-dialog') === null`, 15_000);
}
