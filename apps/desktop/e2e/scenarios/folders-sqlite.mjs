// Folders: a folder of connections made in a project, a connection moved into it from its
// menu, then the folder deleted with the connection staying in the project; in the rail, a
// project dropped on another makes a folder, which is renamed and ungrouped from its menu.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const P = "00000000-0000-4000-9000-000000000041";
const Q = "00000000-0000-4000-9000-000000000042";
const A = "00000000-0000-4000-8000-0000000000a4";
const B = "00000000-0000-4000-8000-0000000000b4";

const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const labels = `[...document.querySelectorAll('.tree-row')].map(r => r.querySelector('.tree-label')?.textContent)`;
const menuButton = (text) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(text)}))`;
const nameDialog = `document.querySelector('.name-dialog')`;

export function setup({ dir }) {
  const connection = (id, name) =>
    `[[connections]]\nid = "${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${join(dir, `${name}.db`)}'\nsave_password = false\n`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 5

[[projects]]
id = "${P}"
name = "Primo"
color = "#d08f20"
connections = ["${A}", "${B}"]

[[projects]]
id = "${Q}"
name = "Secondo"
color = "#3d7bf7"

${connection(A, "alpha")}
${connection(B, "beta")}`,
  );
}

/** Opens the context menu of `selector` and clicks `item` (in `submenu` when given). */
async function menu(page, selector, item, submenu) {
  await page.clickSel(selector, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  if (submenu) {
    await page.eval(`${menuButton(submenu)}.click()`);
    await page.waitFor(`document.querySelector('.context-menu.submenu') !== null`);
  }
  await page.waitFor(`${menuButton(item)} !== undefined`);
  await page.eval(`${menuButton(item)}.click()`);
}

/** Types `name` in the name dialog and saves. */
async function answerName(page, name) {
  await page.waitFor(`${nameDialog} !== null`);
  await page.eval(`(() => {
    const el = ${nameDialog}.querySelector('input');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(name)});
    el.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  await page.eval(`${nameDialog}.querySelector('button[type=submit]').click()`);
  await page.waitFor(`${nameDialog} === null`);
}

const mark = (page, expression) =>
  page.eval(`document.querySelectorAll('#e2e-target').forEach(e => e.removeAttribute('id')); ${expression}.id = 'e2e-target'`);

export default async function (page) {
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`window.__platypus.switchProject(${JSON.stringify(P)})`);
  await page.waitFor(`${labels}.join() === 'alpha,beta'`);

  // A folder from the navigator's header.
  await page.eval(`document.querySelector('.sidebar-header button[title="New folder"]').click()`);
  await answerName(page, "Team");
  await page.waitFor(`${treeRow("Team")} !== undefined`);

  // A connection moved into it from its menu.
  await mark(page, `${treeRow("alpha")}.querySelector('.tree-label')`);
  await menu(page, "#e2e-target", "Team", "Move to folder");
  const pad = (label) => `parseInt(${treeRow(label)}.style.paddingLeft)`;
  await page.waitFor(`${pad("alpha")} > ${pad("Team")}`);
  assert.equal(await page.eval(`${labels}.join()`), "Team,alpha,beta");

  // Closing the folder hides it; deleting it leaves the connection at the root.
  await page.eval(`${treeRow("Team")}.querySelector('.twisty').click()`);
  await page.waitFor(`${labels}.join() === 'Team,beta'`);
  await mark(page, `${treeRow("Team")}.querySelector('.tree-label')`);
  await menu(page, "#e2e-target", "Delete folder");
  await page.waitFor(`${labels}.join() === 'alpha,beta'`);

  // In the rail: a project dropped on another makes a folder (the drop as the store sees it).
  const workspace = await page.eval(
    `window.__TAURI_INTERNALS__.invoke('group_projects', { target: ${JSON.stringify(P)}, dragged: ${JSON.stringify(Q)} })`,
  );
  assert.equal(workspace.project_folders.length, 1);
  await page.eval(`location.reload()`);
  await page.waitFor(`document.querySelectorAll('.project-folder-open .project-square').length === 2`, 15_000);

  // Renamed and ungrouped from its menu.
  await menu(page, ".project-folder-head", "Rename folder");
  await answerName(page, "Clienti");
  await page.waitFor(`document.querySelector('.project-folder-head').title === 'Clienti'`);
  await menu(page, ".project-folder-head", "Ungroup");
  await page.waitFor(`document.querySelector('.project-folder-open') === null`);
  assert.equal(await page.eval(`document.querySelectorAll('.project-rail .project-square:not(.all)').length`), 2);
}
