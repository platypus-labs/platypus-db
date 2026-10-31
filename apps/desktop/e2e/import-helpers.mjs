// What the scenarios on importing connections from other tools share: the import dialog, its
// rows, and the connections the app saved in connections.toml.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const importDialog = `document.querySelector('.import-dialog')`;
export const importRow = (name) =>
  `[...${importDialog}.querySelectorAll('.import-row.connection')].find(r => r.textContent.includes(${JSON.stringify(name)}))`;
const importButton = `[...${importDialog}.querySelectorAll('.actions button')].find(b => /^Import/.test(b.textContent.trim()))`;

/** Opens the import dialog on the source tile `source` and reads `path` as the file picker would
 *  answer. Returns the error shown, or null once the rows (or the empty notice) are there. */
export async function readImportFile(page, source, path) {
  await page.waitFor(`window.__platypus !== undefined`);
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(path)})`);
  if ((await page.eval(`${importDialog} === null`)) === true) {
    await page.eval(`document.querySelector('.sidebar-header button[title^="Import connections"]').click()`);
    await page.waitFor(`${importDialog} !== null`);
  }
  await page.eval(`${importDialog}.querySelector('.import-tile[data-source="${source}"]').click()`);
  // The notice of an earlier file goes when the next one is read.
  await page.eval(`${importDialog}.querySelector('.import-choose').click()`);
  await page.waitFor(
    `${importDialog}.querySelector('.import-row.connection') !== null || ${importDialog}.querySelector('.notice.error') !== null || ${importDialog}.textContent.includes('No connections found')`,
    15_000,
  );
  return page.eval(`${importDialog}.querySelector('.notice.error')?.textContent ?? null`);
}

/** The label of the dialog's Import button (`Import 2`). */
export const importLabel = (page) => page.eval(`${importButton}.textContent.trim()`);

/** Presses the dialog's Import button and waits for the dialog to go. */
export async function saveImport(page) {
  await page.waitFor(`!${importButton}.disabled`);
  await page.eval(`${importButton}.click()`);
  await page.waitFor(`${importDialog} === null`, 15_000);
}

/** Closes the dialog with Cancel. */
export async function cancelImport(page) {
  await page.eval(`[...${importDialog}.querySelectorAll('.actions button')].find(b => b.textContent.trim() === 'Cancel').click()`);
  await page.waitFor(`${importDialog} === null`);
}

/** The connections in the data folder's connections.toml, by name, as flat key → value (the
 *  top-level keys of each `[[connections]]` block; values as the file spells them). Empty
 *  while the file does not exist. */
export function savedConnections(dir) {
  const file = join(dir, "connections.toml");
  const out = {};
  if (!existsSync(file)) return out;
  for (const block of readFileSync(file, "utf8").split("[[connections]]").slice(1)) {
    const flat = {};
    for (const line of block.split(/\r?\n/)) {
      if (line.startsWith("[")) break;
      const m = line.match(/^(\w+)\s*=\s*(.*)$/);
      if (m) flat[m[1]] = m[2].replace(/^["']|["']$/g, "");
    }
    out[flat.name] = flat;
  }
  return out;
}
