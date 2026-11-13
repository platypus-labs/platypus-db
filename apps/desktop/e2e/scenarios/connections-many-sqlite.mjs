// Several connections at once, and what the navigator keeps across a restart. The selected
// connections go into a folder together, whichever of them is dragged; a connection dragged
// from outside the selection goes alone. Deleting asks once, naming them all, and the menu of
// a connection outside the selection still deals with that one. A real restart reopens the
// last project with its folders as they were left, and deleting a folder forgets it was closed.
// Every drag is real pointer input.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { restart } from "../journey-helpers.mjs";

const P = "00000000-0000-4000-9000-000000000051";
const ID = { alpha: "a1", beta: "b1", gamma: "c1", delta: "d1" };
const uuid = (name) => `00000000-0000-4000-8000-0000000000${ID[name]}`;

const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const labelOf = (label) => `${treeRow(label)}.querySelector('.tree-label')`;
const labels = `[...document.querySelectorAll('.tree-row')].map(r => r.querySelector('.tree-label')?.textContent)`;
const pad = (label) => `parseInt(${treeRow(label)}.style.paddingLeft)`;
const menuButton = (text) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(text)}))`;
const modalButton = (text) => `[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const nameDialog = `document.querySelector('.name-dialog')`;
const closedFolders = `JSON.parse(localStorage.getItem('platypus.navigator') ?? '{}').closedFolders ?? []`;

export function setup({ dir }) {
  const connection = (name) =>
    `[[connections]]\nid = "${uuid(name)}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${join(dir, `${name}.db`)}'\nsave_password = false\n`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 5

[[projects]]
id = "${P}"
name = "Primo"
color = "#d08f20"
connections = [${Object.keys(ID)
      .map((n) => `"${uuid(n)}"`)
      .join(", ")}]

${Object.keys(ID).map(connection).join("\n")}`,
  );
}

async function newFolder(page, name) {
  await page.eval(`document.querySelector('.sidebar-header button[title="New folder"]').click()`);
  await page.waitFor(`${nameDialog} !== null`);
  await page.eval(`(() => {
    const el = ${nameDialog}.querySelector('input');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(name)});
    el.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  await page.eval(`${nameDialog}.querySelector('button[type=submit]').click()`);
  await page.waitFor(`${nameDialog} === null && ${treeRow(name)} !== undefined`);
}

/** Selects the rows `names` as the mouse does: a click, then Ctrl+clicks. */
async function selectRows(page, ...names) {
  for (const [i, name] of names.entries()) {
    const r = await page.rectOf(labelOf(name));
    await page.click(r.x + r.w / 2, r.y + r.h / 2, i === 0 ? {} : { modifiers: CTRL });
    await page.waitFor(`${treeRow(name)}.classList.contains('selected')`);
  }
}

const dragInto = (page, from, to) => page.dragEl(labelOf(from), treeRow(to), { toAt: [0.3, 0.5] });

async function openMenu(page, row) {
  await page.eval(`document.querySelectorAll('#e2e-target').forEach(e => e.removeAttribute('id')); ${labelOf(row)}.id = 'e2e-target'`);
  await page.clickSel("#e2e-target", { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

export default async function (page, ctx) {
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`window.__platypus.switchProject(${JSON.stringify(P)})`);
  await page.waitFor(`${labels}.join() === 'alpha,beta,gamma,delta'`);
  await newFolder(page, "Team");
  await newFolder(page, "Archive");

  // Two selected go together, in their order, dragged by either.
  await selectRows(page, "alpha", "beta");
  await dragInto(page, "beta", "Team");
  await page.waitFor(`${labels}.join() === 'Team,alpha,beta,Archive,gamma,delta'`);
  assert.ok(await page.eval(`${pad("alpha")} > ${pad("Team")} && ${pad("beta")} > ${pad("Team")}`), "both in the folder");

  // A connection outside the selection goes alone, and the selection stays out of it.
  await selectRows(page, "alpha", "beta");
  await dragInto(page, "gamma", "Archive");
  await page.waitFor(`${labels}.join() === 'Team,alpha,beta,Archive,gamma,delta' && ${pad("gamma")} > ${pad("Archive")}`);
  assert.ok(await page.eval(`${pad("delta")} === ${pad("Archive")} && ${pad("alpha")} > ${pad("Team")}`));

  // Closed, then a real restart: the same project, the same folders open and closed.
  await page.eval(`${treeRow("Archive")}.querySelector('.twisty').click()`);
  await page.waitFor(`${labels}.join() === 'Team,alpha,beta,Archive,delta'`);
  await restart(page, ctx);
  await page.waitFor(`document.querySelector('.project-rail .project-square[aria-pressed="true"]')?.title === 'Primo'`);
  await page.waitFor(`${labels}.join() === 'Team,alpha,beta,Archive,delta'`);
  await page.eval(`${treeRow("Archive")}.querySelector('.twisty').click()`);
  await page.waitFor(`${labels}.join() === 'Team,alpha,beta,Archive,gamma,delta'`);
  assert.deepEqual(await page.eval(closedFolders), []);

  // The menu of a connection outside the selection is about that one.
  await selectRows(page, "alpha", "beta");
  await openMenu(page, "delta");
  assert.equal(await page.eval(`${menuButton("Delete 2")} === undefined && ${menuButton("Delete…")} !== undefined`), true);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);

  // Deleting several asks once, naming them; Cancel keeps them.
  await selectRows(page, "alpha", "beta");
  await openMenu(page, "beta");
  await page.eval(`${menuButton("Delete 2 connections…")}.click()`);
  await page.waitFor(`document.querySelector('.modal') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal').innerText`), /Delete these 2 connections: alpha, beta\?/);
  await page.eval(`${modalButton("Cancel")}.click()`);
  await page.waitFor(`document.querySelector('.modal') === null`);
  assert.equal(await page.eval(`${labels}.join()`), "Team,alpha,beta,Archive,gamma,delta");

  await openMenu(page, "alpha");
  await page.eval(`${menuButton("Delete 2 connections…")}.click()`);
  await page.waitFor(`${modalButton("Delete")} !== undefined`);
  await page.eval(`${modalButton("Delete")}.click()`);
  await page.waitFor(`${labels}.join() === 'Team,Archive,gamma,delta'`);
  const after = readFileSync(join(ctx.dir, "connections.toml"), "utf8");
  for (const gone of ["alpha", "beta"]) assert.ok(!after.includes(`name = "${gone}"`), `${gone} is still saved`);
  for (const kept of ["gamma", "delta"]) assert.ok(after.includes(`name = "${kept}"`), `${kept} was lost`);

  // Deleting a folder left closed forgets it.
  await page.eval(`${treeRow("Archive")}.querySelector('.twisty').click()`);
  await page.waitFor(`(${closedFolders}).length === 1`);
  await openMenu(page, "Archive");
  await page.eval(`${menuButton("Delete folder")}.click()`);
  await page.waitFor(`${treeRow("Archive")} === undefined && (${closedFolders}).length === 0`);
}
