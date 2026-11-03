// Export connections to a .platypusconn file and read it back: the menu entry preselects its
// connection; Select all; the password needs 8 characters and a matching confirmation, and
// "Include saved passwords" waits for a password; an export in clear reads back in the import
// dialog (and imports); an encrypted one is not readable text, asks for its password when
// imported, shows an error for a wrong one and opens with the right one.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fill, menu, menuItem, treeRow } from "../conn-helpers.mjs";
import { cancelImport, importDialog, saveImport } from "../import-helpers.mjs";

const NAMES = ["Exp A", "Exp B", "Exp C"];
const dlg = `document.querySelector('.export-connections')`;
const check = (text) => `[...${dlg}.querySelectorAll('label.check')].find(l => l.textContent.includes(${JSON.stringify(text)})).querySelector('input')`;
const exportButton = `${dlg}.querySelector('.actions .primary')`;
const passwordFields = `${dlg}.querySelectorAll('.export-password input[type=password]')`;
const tickedNames = `[...${dlg}.querySelectorAll('.import-row.connection')].filter(r => r.querySelector('input').checked).map(r => r.querySelector('.import-name').textContent)`;

export function setup({ dir }) {
  let toml = "version = 7\n";
  NAMES.forEach((name, i) => {
    toml += `\n[[connections]]\nid = "00000000-0000-4000-8000-0000000000a${i}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${join(dir, `${i}.db`)}'\nsave_password = false\n`;
  });
  writeFileSync(join(dir, "connections.toml"), toml);
}

async function openExport(page, name) {
  await page.eval(`document.getElementById('qa-exp')?.removeAttribute('id'); ${treeRow(name)}.id = 'qa-exp'`);
  const items = await menu(page, "#qa-exp");
  assert.ok(items.some((i) => i === "Export"), JSON.stringify(items));
  await menuItem(page, "Export connections…");
  await page.waitFor(`${dlg} !== null`);
}

/** Types into the n-th password field of the export dialog. */
const typeInto = (page, n, value) => page.eval(fill(`${passwordFields}[${n}]`, value));

/** Presses Export with the file picker answering `path`, and waits for the dialog to go. */
async function exportTo(page, path) {
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(path)})`);
  await page.waitFor(`!${exportButton}.disabled`);
  await page.eval(`${exportButton}.click()`);
  await page.waitFor(`${dlg} === null`, 15_000);
}

/** Opens the import dialog on the PlatypusDB tile and reads `path`. */
async function openImportOf(page, path) {
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(path)})`);
  await page.eval(`document.querySelector('.sidebar-header button[title^="Import connections"]').click()`);
  await page.waitFor(`${importDialog} !== null`);
  await page.eval(`${importDialog}.querySelector('.import-tile[data-source="platypus"]').click()`);
  await page.eval(`${importDialog}.querySelector('.import-choose').click()`);
}

export default async function (page, { dir }) {
  await page.waitFor(`window.__platypus !== undefined`);
  for (const name of NAMES) await page.waitFor(`${treeRow(name)} !== undefined`);

  // The menu entry of a connection opens the dialog with that one ticked.
  await openExport(page, "Exp B");
  assert.deepEqual(await page.eval(tickedNames), ["Exp B"]);
  assert.equal((await page.eval(`${exportButton}.textContent`)).trim(), "Export 1");
  // Select all, then none; with none the button is disabled.
  const selectAll = `${dlg}.querySelector('.export-all input')`;
  await page.eval(`${selectAll}.click()`);
  await page.waitFor(`(${tickedNames}).length === 3`);
  assert.equal((await page.eval(`${exportButton}.textContent`)).trim(), "Export 3");
  await page.eval(`${selectAll}.click()`);
  await page.waitFor(`(${tickedNames}).length === 0`);
  assert.equal(await page.eval(`${exportButton}.disabled`), true);
  await page.eval(`${selectAll}.click()`);
  await page.waitFor(`(${tickedNames}).length === 3`);

  // Passwords: none of the checks below exist until "Encrypt" is on; saved passwords wait for it.
  assert.equal(await page.eval(`${passwordFields}.length`), 0);
  assert.equal(await page.eval(`${check("Include saved passwords")}.disabled`), true);
  await page.eval(`${check("Encrypt with a password")}.click()`);
  await page.waitFor(`${passwordFields}.length === 2`);
  assert.equal(await page.eval(`${check("Include saved passwords")}.disabled`), false);
  // Encrypting with nothing typed yet: not allowed, and no complaint before typing.
  assert.equal(await page.eval(`${exportButton}.disabled`), true);
  await typeInto(page, 0, "short");
  await page.waitFor(`${dlg}.textContent.includes('Use at least 8 characters.')`);
  assert.equal(await page.eval(`${exportButton}.disabled`), true);
  // Long enough, but the confirmation differs.
  await typeInto(page, 0, "correct horse");
  await typeInto(page, 1, "correct hors");
  await page.waitFor(`${dlg}.textContent.includes('The passwords do not match.')`);
  assert.equal(await page.eval(`${exportButton}.disabled`), true);
  await typeInto(page, 1, "correct horse");
  await page.waitFor(`!${exportButton}.disabled`);
  // Without encryption again, the password does not matter.
  await page.eval(`${check("Encrypt with a password")}.click()`);
  await page.waitFor(`${passwordFields}.length === 0 && !${exportButton}.disabled`);
  assert.equal(await page.eval(`${check("Include saved passwords")}.disabled`), true);

  // Export in clear: the file is JSON with the three connections.
  const plain = join(dir, "plain.platypusconn");
  await exportTo(page, plain);
  await page.waitFor(`document.querySelector('.toast')?.textContent.includes('Exported 3 connections')`);
  const text = readFileSync(plain, "utf8");
  for (const name of NAMES) assert.ok(text.includes(name), `${name} in the file`);
  assert.doesNotMatch(text, /password/i);

  // It reads back in the import dialog; the connections exist, so they come unticked and
  // the replace option waits for a ticked one.
  const replaceBox = `${importDialog}.querySelector('.import-replace input')`;
  const rowsText = `${importDialog}.querySelector('.import-tree').textContent`;
  const tickAll = `[...${importDialog}.querySelectorAll('.import-row.connection input')].forEach(i => { if (!i.checked) i.click(); })`;
  const savedToml = () => readFileSync(join(dir, "connections.toml"), "utf8");
  const savedCount = () => savedToml().split("[[connections]]").length - 1;
  await openImportOf(page, plain);
  await page.waitFor(`${importDialog}.querySelectorAll('.import-row.connection').length === 3`, 15_000);
  assert.equal(await page.eval(`${importDialog}.querySelector('.import-password') === null`), true);
  assert.match(await page.eval(rowsText), /already saved/);
  assert.equal(await page.eval(`${replaceBox} === null`), true);
  // Ticked, they replace the saved ones (on by default): same ids, no new connection.
  await page.eval(tickAll);
  await page.waitFor(`${replaceBox}?.checked === true`);
  assert.match(await page.eval(rowsText), /replaces “Exp A”/);
  await saveImport(page);
  assert.equal(savedCount(), 3, "replaced, not added");
  for (let i = 0; i < 3; i++) assert.ok(savedToml().includes(`0000000000a${i}`), `connection ${i} kept its id`);

  // Again with the option off: they are added beside the saved ones.
  await openImportOf(page, plain);
  await page.waitFor(`${importDialog}.querySelectorAll('.import-row.connection').length === 3`, 15_000);
  await page.eval(tickAll);
  await page.waitFor(`${replaceBox}?.checked === true`);
  await page.eval(`${replaceBox}.click()`);
  await page.waitFor(`!${replaceBox}.checked && !(${rowsText}).includes('replaces')`);
  assert.match(await page.eval(rowsText), /already saved/);
  await saveImport(page);
  assert.equal(savedCount(), 6, "the three connections were imported next to the first three");

  // Encrypted, with the saved passwords option on: not readable text.
  await openExport(page, "Exp A");
  await page.eval(`${selectAll}.click()`);
  await page.eval(`${check("Encrypt with a password")}.click()`);
  await page.waitFor(`${passwordFields}.length === 2`);
  await typeInto(page, 0, "correct horse");
  await typeInto(page, 1, "correct horse");
  await page.eval(`${check("Include saved passwords")}.click()`);
  const sealed = join(dir, "sealed.platypusconn");
  await exportTo(page, sealed);
  assert.ok(!readFileSync(sealed, "utf8").includes("Exp A"), "the names are not readable in the encrypted file");

  // Importing it asks for the password; a wrong one shows an error and asks again.
  await openImportOf(page, sealed);
  await page.waitFor(`${importDialog}.querySelector('.import-password') !== null`, 15_000);
  assert.equal(await page.eval(`${importDialog}.querySelectorAll('.import-row.connection').length`), 0);
  const field = `${importDialog}.querySelector('.import-password input[type=password]')`;
  const open = `${importDialog}.querySelector('.import-password button')`;
  assert.equal(await page.eval(`${open}.disabled`), true);
  await page.eval(fill(field, "not the password"));
  await page.waitFor(`!${open}.disabled`);
  await page.eval(`${open}.click()`);
  await page.waitFor(`${importDialog}.querySelector('.notice.error') !== null`, 15_000);
  assert.match(await page.eval(`${importDialog}.querySelector(".notice.error").textContent`), /Wrong password/);
  assert.equal(await page.eval(`${importDialog}.querySelectorAll('.import-row.connection').length`), 0);
  assert.equal(await page.eval(`${importDialog}.querySelector('.import-password') !== null`), true);
  // The right password opens it: the field and the error go, the six connections show
  // (the three of the first import and the three of the file are named the same).
  await page.eval(fill(field, "correct horse"));
  await page.eval(`${open}.click()`);
  await page.waitFor(`${importDialog}.querySelectorAll('.import-row.connection').length === 6`, 15_000);
  assert.equal(await page.eval(`${importDialog}.querySelector('.import-password') === null`), true);
  assert.equal(await page.eval(`${importDialog}.querySelector('.notice.error') === null`), true);
  await cancelImport(page);
}
