// Folders: a folder of connections made in a project, a connection moved into it from its
// menu, then the folder deleted with the connection staying in the project; connections
// reordered by drag and drop and Alt+Up, and one dropped onto another making a folder. In
// the rail, projects reordered by drag and drop (kept after a reload), a drag cancelled with
// Escape, a project dropped on another making a folder, reordered inside it, dropped into it
// and out of it at the end of the rail; the folder renamed and ungrouped from its menu.
// Every drag is real pointer input (press, moves, release), as a user's mouse makes it.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ALT } from "../cdp.mjs";

const P = "00000000-0000-4000-9000-000000000041";
const Q = "00000000-0000-4000-9000-000000000042";
const T = "00000000-0000-4000-9000-000000000043";
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

[[projects]]
id = "${T}"
name = "Terzo"
color = "#2f9e6e"

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

/** How far down a target a drop on its top edge (`before`), middle (`into`) or bottom edge
 *  (`after`) is. */
const EDGE = { before: 0.1, into: 0.5, after: 0.9 };

/** Drags the row `from` onto the row `to`, at `where` of it. */
const drag = (page, from, to, where) => page.dragEl(treeRow(from), treeRow(to), { fromAt: [0.3, 0.5], toAt: [0.3, EDGE[where]] });

/** The rail's square of the project `name`. */
const square = (name) => `[...document.querySelectorAll('.project-rail .project-square')].find(s => s.title === ${JSON.stringify(name)})`;
/** The rail's projects, top to bottom, and those of its open folder. */
const railNames = `[...document.querySelectorAll('.project-rail .project-square:not(.all)')].map(s => s.title).join()`;
const folderNames = `[...document.querySelectorAll('.project-folder-open .project-square')].map(s => s.title).join()`;
const pressed = (name) => `${square(name)}?.getAttribute('aria-pressed') === 'true'`;

/** Drags the project `from` in the rail onto `to` (an element expression), at `where`. */
const railDrag = (page, from, to, where, options = {}) => page.dragEl(square(from), to, { toAt: [0.5, EDGE[where]], ...options });

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

  await rail(page);
}

/** Drags and drops in the rail, with the project Primo active throughout. */
async function rail(page) {
  await page.waitFor(`${railNames} === 'Primo,Secondo,Terzo' && ${pressed("Primo")}`);

  // Terzo onto the top edge of Primo goes before it: a line shows where, the label follows
  // the pointer, and the release does not switch to the dragged project.
  let during = null;
  await railDrag(page, "Terzo", square("Primo"), "before", {
    before: async () => {
      during = await page.eval(`({ line: !!document.querySelector('.rail-drop-line'), ghost: document.querySelector('.drag-ghost')?.textContent })`);
    },
  });
  assert.deepEqual(during, { line: true, ghost: "Terzo" });
  await page.waitFor(`${railNames} === 'Terzo,Primo,Secondo'`);
  assert.equal(await page.eval(`document.querySelector('.drag-ghost') === null && document.querySelector('.rail-drop-line') === null`), true);
  assert.equal(await page.eval(pressed("Primo")), true, "the drag switched project");

  // Escape cancels a drag: nothing moves, nothing is grouped, no project is switched.
  await railDrag(page, "Secondo", square("Terzo"), "into", { before: () => page.key("Escape", "Escape", 27) });
  await page.sleep(400);
  assert.equal(await page.eval(`${railNames} + '|' + ${folderNames}`), "Terzo,Primo,Secondo|");
  assert.equal(await page.eval(`document.querySelector('.drag-ghost') === null && ${pressed("Primo")}`), true);

  // Primo onto the bottom edge of Secondo goes after it; the order is saved.
  await railDrag(page, "Primo", square("Secondo"), "after");
  await page.waitFor(`${railNames} === 'Terzo,Secondo,Primo'`);
  await page.eval(`location.reload()`);
  await page.waitFor(`${railNames} === 'Terzo,Secondo,Primo'`, 15_000);

  // Secondo onto the middle of Terzo: the target is outlined, then a folder holds both.
  await railDrag(page, "Secondo", square("Terzo"), "into", {
    before: async () => {
      during = await page.eval(`document.querySelector('.project-square.drop-into')?.title`);
    },
  });
  assert.equal(during, "Terzo");
  await page.waitFor(`${folderNames} === 'Terzo,Secondo'`);
  assert.equal(await page.eval(railNames), "Terzo,Secondo,Primo");

  // Inside the open folder, Secondo onto Terzo's top edge goes before it, in the folder.
  await railDrag(page, "Secondo", square("Terzo"), "before");
  await page.waitFor(`${folderNames} === 'Secondo,Terzo'`);

  // Primo onto the lower half of the folder's head goes into it, last.
  await railDrag(page, "Primo", `document.querySelector('.project-folder-head')`, "after");
  await page.waitFor(`${folderNames} === 'Secondo,Terzo,Primo'`);

  // Primo to the end of the rail leaves the folder, last.
  await railDrag(page, "Primo", `document.querySelector('.project-add')`, "into");
  await page.waitFor(`${folderNames} === 'Secondo,Terzo' && ${railNames} === 'Secondo,Terzo,Primo'`);
  await page.eval(`location.reload()`);
  await page.waitFor(`${folderNames} === 'Secondo,Terzo' && ${railNames} === 'Secondo,Terzo,Primo'`, 15_000);
  assert.equal(await page.eval(pressed("Primo")), true);

  // Renamed and ungrouped from its menu.
  await menu(page, ".project-folder-head", "Rename folder");
  await answerName(page, "Clienti");
  await page.waitFor(`document.querySelector('.project-folder-head').title === 'Clienti'`);
  await menu(page, ".project-folder-head", "Ungroup");
  await page.waitFor(`document.querySelector('.project-folder-open') === null`);
  assert.equal(await page.eval(railNames), "Secondo,Terzo,Primo");

  // A click without moving still switches project.
  await page.clickSel(`.project-rail .project-square[data-project="${Q}"]`);
  await page.waitFor(pressed("Secondo"));
}
