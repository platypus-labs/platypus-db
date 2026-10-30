// The project dialog's Folder field: it lists the rail's folders after "None (top level)",
// shows the folder the project is in, and moves the project into or out of a folder on Save
// (as dragging does). A new project can be made inside a folder. Without folders, no field.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ids = { a: "00000000-0000-4000-9000-0000000000a1", b: "00000000-0000-4000-9000-0000000000b1", c: "00000000-0000-4000-9000-0000000000c1" };
const FOLDER = "00000000-0000-4000-9000-0000000000f1";

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2

[[projects]]
id = "${ids.a}"
name = "Alpha"
color = "#d08f20"
connections = []

[[projects]]
id = "${ids.b}"
name = "Beta"
color = "#3d7bf7"
connections = []

[[projects]]
id = "${ids.c}"
name = "Gamma"
color = "#2f9e6e"
connections = []

[[project_folders]]
id = "${FOLDER}"
name = "Clients"
color = "#6c63ff"
projects = ["${ids.a}", "${ids.b}"]
collapsed = false
`,
  );
}

const square = (id) => `document.querySelector('.project-square[data-project="${id}"]')`;
/** The folder a project's square sits in on the rail, or "" at the top level. */
const folderOf = (id) => `(${square(id)}?.closest('[data-folder]')?.dataset.folder ?? '')`;
const field = `document.querySelector('.project-dialog [data-field=folder]')`;

// No connection: the steps start once the rail shows the projects.
export const ready = `${square(ids.c)} !== null`;

/** Opens the edit dialog of a project from its rail menu. */
async function edit(page, id) {
  const r = await page.rect(`.project-square[data-project="${id}"]`);
  await page.click(r.x + r.w / 2, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.includes('Rename'))`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.includes('Rename')).click()`);
  await page.waitFor(`document.querySelector('.project-dialog') !== null`);
}

const save = (page) => page.eval(`[...document.querySelectorAll('.project-dialog button')].find(b => b.textContent === 'Save').click()`);

export default async function (page, { dir }) {
  assert.equal(await page.eval(folderOf(ids.a)), FOLDER);
  assert.equal(await page.eval(folderOf(ids.c)), "");

  // The field lists "None (top level)" first, then the folders; it shows the current one.
  await edit(page, ids.a);
  assert.deepEqual(await page.eval(`[...${field}.options].map(o => [o.value, o.textContent])`), [
    ["", "None (top level)"],
    [FOLDER, "Clients"],
  ]);
  assert.equal(await page.eval(`${field}.value`), FOLDER);

  // Taken out of the folder.
  await page.eval(`${field}.value = ''; ${field}.dispatchEvent(new Event('change', { bubbles: true }))`);
  await save(page);
  await page.waitFor(`document.querySelector('.project-dialog') === null`);
  await page.waitFor(`${folderOf(ids.a)} === ''`);
  assert.doesNotMatch(readFileSync(join(dir, "connections.toml"), "utf8"), new RegExp(`projects = \[[^\]]*${ids.a}`));

  // Put into the folder.
  await edit(page, ids.c);
  assert.equal(await page.eval(`${field}.value`), "");
  await page.eval(`${field}.value = ${JSON.stringify(FOLDER)}; ${field}.dispatchEvent(new Event('change', { bubbles: true }))`);
  await save(page);
  await page.waitFor(`document.querySelector('.project-dialog') === null`);
  await page.waitFor(`${folderOf(ids.c)} === ${JSON.stringify(FOLDER)}`);
  assert.equal(await page.eval(folderOf(ids.b)), FOLDER, "the other members stay");
  await edit(page, ids.b);
  assert.equal(await page.eval(`${field}.value`), FOLDER);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.project-dialog') === null`);

  // A new project can start in a folder.
  await page.eval(`document.querySelector('.project-add').click()`);
  await page.waitFor(`document.querySelector('.project-dialog') !== null`);
  await page.type("Delta");
  await page.eval(`${field}.value = ${JSON.stringify(FOLDER)}; ${field}.dispatchEvent(new Event('change', { bubbles: true }))`);
  await save(page);
  await page.waitFor(`document.querySelector('.project-square[title="Delta"]') !== null`);
  const delta = await page.eval(`document.querySelector('.project-square[title="Delta"]').dataset.project`);
  await page.waitFor(`${folderOf(delta)} === ${JSON.stringify(FOLDER)}`);
}
