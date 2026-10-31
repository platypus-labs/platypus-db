// qa4-ana2-3: a numbered list in a notebook's text cell starts at 1 whatever its first number.
// Steps: notebook, text cell "Steps 3 and 4:\n\n3. three\n4. four", shown as rendered text
//   (and exported as HTML).
// Expected: the list is numbered 3. and 4. (<ol start="3">), as Markdown renders it.
// Actual: <ol> without start: it reads 1. three, 2. four.
// Code: apps/desktop/src/notebook/html.ts parseMarkdown (the list keeps `ordered` only, the
//   number of the first item is dropped), Markdown.tsx and blockHtml render <ol> plain.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { setField } from "../plans.mjs";
import { saveConnections, sqliteEntry } from "../perf.mjs";
import { click, newNotebook, tab, written } from "../qa4-ana2.mjs";

const NAME = "Ana2 NB list";
export function setup({ dir }) {
  const file = join(dir, "nb.db");
  new DatabaseSync(file).close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page, { dir }) {
  await newNotebook(page, NAME);
  await page.waitFor(`${tab}.querySelector('textarea.notebook-text') !== null`);
  await setField(page, ".notebook-tab textarea.notebook-text", "Steps 3 and 4:\n\n3. three\n4. four");
  await page.eval(click(`${tab}.querySelector('.notebook-cell.markdown')`, "edit"));
  await page.waitFor(`${tab}.querySelector('.notebook-markdown ol') !== null`);
  const start = await page.eval(`${tab}.querySelector('.notebook-markdown ol').start`);
  assert.equal(start, 3, `the list starts at ${start}`);
}
