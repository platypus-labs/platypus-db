// What the maintenance scenarios (group S) share: the navigator's Maintenance menu of a table
// and its dialog, on a table of the dev database. The menu offers exactly the operations of
// the database; each opens a dialog that says what it does and asks first (Cancel and Escape
// run nothing); once run it says done and shows what the server answered; a table that is
// gone gives the server's error in the dialog; a read-only connection refuses.
import assert from "node:assert/strict";
import { openPath } from "./qa-dbs.mjs";
import { treeMenu } from "./qa-grid.mjs";

const LABELS = { vacuum: "Vacuum…", analyze: "Analyze…", reindex: "Reindex…", optimize: "Optimize…", check: "Check…" };
const WORDS = { vacuum: "Vacuum", analyze: "Analyze", reindex: "Reindex", optimize: "Optimize", check: "Check" };

export const dialog = `document.querySelector('.maintenance-dialog')`;
const result = `${dialog}?.querySelector('[data-field="maintenance-result"]')`;

/** Opens the Maintenance menu of the tree row `table` and returns the labels it offers. */
export async function maintenanceMenu(page, table) {
  await treeMenu(page, table, "Maintenance");
  await page.waitFor(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.endsWith('…'))`);
  return page.eval(
    `[...document.querySelectorAll('.context-menu button')].map(b => b.textContent.trim()).filter(t => ${JSON.stringify(Object.values(LABELS))}.some(l => t.startsWith(l)))`,
  );
}

/** Picks `operation` in the Maintenance menu of `table` and waits for its dialog. */
export async function openOperation(page, table, operation) {
  const label = LABELS[operation];
  await treeMenu(page, table, "Maintenance");
  await page.waitFor(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.startsWith(${JSON.stringify(label)}))`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(label)})).click()`);
  await page.waitFor(`${dialog} !== null`, 10_000);
}

/** Runs the open dialog's operation; resolves to `{ ok, text, output }`. */
export async function runOperation(page) {
  await page.eval(`${dialog}.querySelector('button.primary').click()`);
  await page.waitFor(`${result} != null`, 120_000);
  return page.eval(
    `({ ok: ${result}.classList.contains('ok'), text: ${result}.textContent, output: ${dialog}.querySelector('pre')?.textContent ?? null })`,
  );
}

/** Closes the dialog with its primary button (Close). */
export async function closeDialog(page) {
  await page.eval(`${dialog}.querySelector('button.primary').click()`);
  await page.waitFor(`${dialog} === null`);
}

/**
 * The maintenance of `table` (`path` in the tree) on `operations`: the menu, Cancel and Escape,
 * each operation run with its output matched by `outputs[operation]` (a RegExp, or null for
 * none to check), and the table gone.
 * `gone` is `{ table, drop }`: a second table in the tree that `drop()` removes behind the app's back.
 */
export async function maintenanceFlow(page, { path, table, operations, outputs, gone, missing }) {
  await openPath(page, path);
  const offered = await maintenanceMenu(page, table);
  assert.deepEqual(
    offered.map((l) => l.replace(/…$/, "")).sort(),
    operations.map((o) => WORDS[o]).sort(),
    `the menu offers only the operations of the database: ${offered}`,
  );
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);

  // Each operation asks first: the dialog says what it does; Cancel and Escape change nothing.
  const first = operations[0];
  await openOperation(page, table, first);
  assert.match(await page.eval(`${dialog}.querySelector('h2').textContent`), new RegExp(`^${WORDS[first]} .*${table}$`));
  assert.ok((await page.eval(`${dialog}.querySelector('p').textContent`)).length > 10, "it says what it does");
  assert.equal(await page.eval(`${result} != null`), false, "not run yet");
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`${dialog} === null`);
  await openOperation(page, table, first);
  await page.eval(`${dialog}.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))`);
  await page.waitFor(`${dialog} === null`);

  for (const operation of operations) {
    await openOperation(page, table, operation);
    assert.match(await page.eval(`${dialog}.querySelector('h2').textContent`), new RegExp(`^${WORDS[operation]} `));
    const r = await runOperation(page);
    assert.ok(r.ok, `${operation}: ${r.text}`);
    assert.equal(r.text, `${WORDS[operation]} done.`);
    if (outputs[operation]) assert.match(r.output ?? "", outputs[operation], `${operation} output`);
    // Done: the buttons are Close only, and running it again needs the menu again.
    assert.equal(await page.eval(`[...${dialog}.querySelectorAll('button')].map(b => b.textContent).join('|')`), "Close");
    await closeDialog(page);
  }

  // A table dropped behind the app's back: the server's error, in the dialog, which stays open.
  await openPath(page, [...path.slice(0, -1), gone.table]);
  gone.drop();
  await openOperation(page, gone.table, operations[0]);
  const r = await runOperation(page);
  assert.equal(r.ok, false, "the table is gone");
  assert.match(r.text, missing);
  assert.equal(r.output, null);
  await closeDialog(page);
}

/** On a read-only connection the operation is refused with its reason, and the table is as it was. */
export async function refusedReadOnly(page, { path, table, operation }) {
  await openPath(page, path);
  await openOperation(page, table, operation);
  const r = await runOperation(page);
  assert.equal(r.ok, false, `refused: ${r.text}`);
  assert.match(r.text, /read-only|read only/i);
  await closeDialog(page);
}

/** Runs the parts one after the other, all of them even if one fails, and fails with every message. */
export async function allParts(...parts) {
  const failures = [];
  for (const part of parts) {
    try {
      await part();
    } catch (e) {
      failures.push(e);
    }
  }
  if (failures.length === 1) throw failures[0];
  if (failures.length > 1) throw new Error(failures.map((e, i) => `[${i + 1}] ${e.message}`).join("\n"));
}
