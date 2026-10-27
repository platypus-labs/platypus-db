// Regression guard: "New connection…" from a connection's menu, in a project, opens the form
// with the project ticked and saves the new connection next to the clicked one, in its
// folder; leaving the project from the form takes it out of the project's view.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { dialog, editConnection, fill, formField, menu, menuItem, picker, saveForm, treeRow } from "../conn-helpers.mjs";

const P = "00000000-0000-4000-9000-0000000000f9";
const A = "00000000-0000-4000-8000-0000000000fa";
const FOLDER = "00000000-0000-4000-9000-0000000000fb";

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[projects]]
id = "${P}"
name = "QA folders"
color = "#3d7bf7"
connections = ["${A}"]

[[projects.folders]]
id = "${FOLDER}"
name = "Inner"

[projects.placement]
"${A}" = "${FOLDER}"

[[connections]]
id = "${A}"
name = "QA in folder"
driver = "sqlite"
database = '${join(dir, "a.db")}'
save_password = false
`,
  );
}

const indent = (label) => `parseInt(${treeRow(label)}.style.paddingLeft)`;

export default async function (page, { dir }) {
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`window.__platypus.switchProject(${JSON.stringify(P)})`);
  await page.waitFor(`${treeRow("QA in folder")} !== undefined`);
  assert.ok((await page.eval(indent("QA in folder"))) > (await page.eval(indent("Inner"))));

  await page.eval(`${treeRow("QA in folder")}.id = 'qa-in-folder'`);
  await menu(page, "#qa-in-folder");
  await menuItem(page, "New connection");
  await page.waitFor(`${picker} !== null`);
  await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === 'SQLite').click()`);
  await page.waitFor(`${dialog} !== null`);
  assert.deepEqual(await page.eval(`[...document.querySelectorAll('.project-chip')].map(c => [c.textContent, c.getAttribute('aria-pressed')])`), [["QA folders", "true"]]);
  await page.eval(fill(formField("Name"), "QA new in folder"));
  await page.eval(fill(formField("Database file"), join(dir, "b.db")));
  await saveForm(page);
  await page.waitFor(`${treeRow("QA new in folder")} !== undefined`);
  assert.equal(await page.eval(indent("QA new in folder")), await page.eval(indent("QA in folder")));

  await editConnection(page, "QA new in folder");
  await page.eval(`[...document.querySelectorAll('.project-chip')].find(c => c.textContent === 'QA folders').click()`);
  await saveForm(page);
  await page.waitFor(`${treeRow("QA new in folder")} === undefined`);
  assert.equal(await page.eval(`${treeRow("QA in folder")} !== undefined`), true);
}
