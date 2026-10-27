// QA ui: Ctrl+T in a project without tabs opens an editor on a connection of another project.
//
// Steps: a project whose only connection is "QA UI mine"; another connection "QA UI other"
// is saved first, outside the project. Show the project (no tab open), press Ctrl+T.
// Expected: a SQL editor on the project's own connection (the navigator lists only it), or
// nothing if the project has none.
// Actual: the editor opens on "QA UI other", the first saved connection of all, which the
// project does not have.
// Code: App.tsx registerShellActions "tab.new", `s().connections?.[0]` reads every saved
// connection, not the active project's (`connectionsOf(project, ...)`).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const PROJECT = "00000000-0000-4000-9000-00000000e113";

export function setup({ dir }) {
  const conns = ["QA UI other", "QA UI mine"].map((name, i) => {
    const file = join(dir, `qa_ui_p${i}.db`);
    new DatabaseSync(file).close();
    return `[[connections]]\nid = "00000000-0000-4000-8000-00000000e11${i}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${file}'\nsave_password = false\n`;
  });
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2

[[projects]]
id = "${PROJECT}"
name = "QA UI project"
color = "#3d7bf7"
connections = ["00000000-0000-4000-8000-00000000e111"]

${conns.join("\n")}`,
  );
}

export default async function (page) {
  await page.eval(`document.querySelector('.project-square[data-project="${PROJECT}"]').click()`);
  await page.waitFor(`document.querySelector('.topbar-project')?.textContent === 'QA UI project'`);
  await page.waitFor(`[...document.querySelectorAll('.tree-label')].some(l => l.textContent === 'QA UI mine')`);
  assert.equal(await page.eval(`document.querySelectorAll('.tab').length`), 0);
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`document.querySelectorAll('.tab').length === 1`, 3000);
  assert.equal(await page.eval(`document.querySelector('.topbar-connection')?.textContent`), "QA UI mine");
}
