// QA ui (UX): destructive confirmations put the focus on the destructive button.
//
// Steps: right-click a project's square > Delete project (or close a tab with unsaved
// changes, or delete a connection).
// Expected: the focus on Cancel, so that an Enter pressed by habit (or meant for the dialog
// that just closed) does not delete or discard anything; the red button is chosen on purpose.
// Actual: the red "Delete" button has the focus; Enter deletes the project at once.
// Code: ui/ask.tsx, `autoFocus` on the confirming button whatever `danger` is.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const PROJECT = "00000000-0000-4000-9000-00000000e140";

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2

[[projects]]
id = "${PROJECT}"
name = "QA UI doomed"
color = "#3d7bf7"
connections = []
`,
  );
}

export const ready = `document.querySelector('.project-square[data-project="${PROJECT}"]') !== null`;

export default async function (page) {
  await page.eval(
    `document.querySelector('.project-square[data-project="${PROJECT}"]').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 30, clientY: 110 }))`,
  );
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu [role=menuitem]')].find(b => b.textContent.startsWith('Delete project')).click()`);
  await page.waitFor(`document.querySelector('.modal.confirm')?.contains(document.activeElement) === true`);
  assert.equal(await page.eval(`document.activeElement.textContent`), "Cancel");
}
