// A connection that names its database shows only that one, as in DBeaver: its schemas right
// under the connection. "Show all databases", beside the Database field, lists the server's
// other databases too; with no database named it is ticked and cannot be changed.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { dialog, editConnection, fill, formField, saveForm } from "../conn-helpers.mjs";
import { needsPostgres, pgConnection, treeRow } from "../qa-jrn.mjs";

export const skip = needsPostgres;
const NAME = "S fix one database";
const box = `document.querySelector('.connection-dialog input[data-field=show_all_databases]')`;

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${pgConnection("00000000-0000-4000-8000-0000000ff201", NAME)}`);
}

const prompt = `document.querySelector(".modal input[type=password]")`;

/** Expands the connection, answering its password prompt if it asks again. */
async function connect(page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${prompt} !== null || ${treeRow("public")} !== undefined`, 20_000);
  if (await page.eval(`${prompt} === null`)) return;
  await page.eval(`${prompt}.focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector(".modal button[type=submit]").click()`);
  await page.waitFor(`${prompt} === null`, 30_000);
}

export default async function (page) {
  // Only the named database: its schemas under the connection, no database rows.
  await connect(page);
  await page.waitFor(`${treeRow("public")} !== undefined`, 20_000);
  assert.equal(await page.eval(`${treeRow("platypus")} === undefined && ${treeRow("postgres")} === undefined`), true, "no database rows");

  // The box beside Database: off, and on the same line.
  await editConnection(page, NAME);
  assert.deepEqual(await page.eval(`({ checked: ${box}.checked, disabled: ${box}.disabled })`), { checked: false, disabled: false });
  const line = await page.eval(`(() => {
    const a = ${formField("Database")}.getBoundingClientRect(), b = ${box}.getBoundingClientRect();
    return b.left > a.right && b.top < a.bottom && b.bottom > a.top;
  })()`);
  assert.equal(line, true, "the box is beside the Database field");

  // Empty database: every database is listed, the box ticked and locked.
  await page.eval(fill(formField("Database"), ""));
  await page.waitFor(`${box}.disabled && ${box}.checked`);
  await page.eval(fill(formField("Database"), "platypus"));
  await page.waitFor(`!${box}.disabled && !${box}.checked`);

  // Ticked and saved: the server's databases, the named one open.
  await page.eval(`${box}.click()`);
  await page.waitFor(`${box}.checked`);
  await saveForm(page);
  await page.waitFor(`${treeRow("public")} === undefined`, 10_000);
  await connect(page);
  await page.waitFor(`${treeRow("postgres")} !== undefined && ${treeRow("platypus")} !== undefined && ${treeRow("public")} !== undefined`, 20_000);

  // Off again: back to the one database.
  await editConnection(page, NAME);
  await page.eval(`${box}.click()`);
  await page.waitFor(`!${box}.checked`);
  await saveForm(page);
  await page.waitFor(`${treeRow("postgres")} === undefined`, 10_000);
  await connect(page);
  await page.waitFor(`${treeRow("public")} !== undefined`, 20_000);
  assert.equal(await page.eval(`${treeRow("postgres")} === undefined && ${dialog} === null`), true);
}
