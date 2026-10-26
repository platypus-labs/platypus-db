// Folders: a folder of connections made in a project, a connection moved into it from its
// menu, then the folder deleted with the connection staying in the project; connections
// reordered by drag and drop and Alt+Up, and one dropped onto another making a folder; in
// the rail, a project dropped on another makes a folder, which is renamed and ungrouped from
// its menu.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ALT } from "../cdp.mjs";

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

/** Drags the row `from` onto the row `to`: on its top edge (`before`), its middle (`into`)
 *  or its bottom edge (`after`). */
const drag = (page, from, to, where) =>
  page.eval(`(() => {
    const dt = new DataTransfer();
    ${treeRow(from)}.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
    const target = ${treeRow(to)};
    const r = target.getBoundingClientRect();
    const y = { before: r.top + 2, into: r.top + r.height / 2, after: r.bottom - 2 }[${JSON.stringify(where)}];
    const at = { bubbles: true, cancelable: true, dataTransfer: dt, clientX: r.left + 40, clientY: y };
    target.dispatchEvent(new DragEvent('dragover', at));
    target.dispatchEvent(new DragEvent('drop', at));
    ${treeRow(from)}?.dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: dt }));
  })()`);

const mark = (page, expression) =>
  page.eval(`document.querySelectorAll('#e2e-target').forEach(e => e.removeAttribute('id')); ${expression}.id = 'e2e-target'`);

export default async function (page, { dir }) {
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

  // Drag and drop: beta dropped on the top edge of alpha goes before it, in the saved file too.
  await drag(page, "beta", "alpha", "before");
  await page.waitFor(`${labels}.join() === 'beta,alpha'`);
  assert.match(readFileSync(join(dir, "connections.toml"), "utf8"), new RegExp(`connections = \\[\\s*"${B}",\\s*"${A}",?\\s*\\]`));

  // Alpha dropped onto the middle of beta: a new folder holds both, offered for renaming.
  await drag(page, "alpha", "beta", "into");
  await answerName(page, "Gruppo");
  await page.waitFor(`${labels}.join() === 'Gruppo,beta,alpha'`);
  assert.ok(await page.eval(`${pad("alpha")} > ${pad("Gruppo")} && ${pad("beta")} > ${pad("Gruppo")}`));

  // Alt+Up moves the selected connection above the one before it in its folder.
  await page.eval(`${treeRow("alpha")}.click()`);
  await page.eval(`${treeRow("alpha")}.focus()`);
  await page.key("ArrowUp", "ArrowUp", 38, ALT);
  await page.waitFor(`${labels}.join() === 'Gruppo,alpha,beta'`);

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
