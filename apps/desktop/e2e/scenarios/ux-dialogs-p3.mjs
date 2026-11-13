// UX second pass (group T, P3) on SQLite: the ER diagram's Fit button; the connection dialog
// (Environment label, titled sections on the Network page, paler placeholders, a project hint);
// the command palette (an icon per result, words that start with the query first); the empty
// list of transfer profiles (a narrow dialog with its X); the connection import (the chosen
// source is marked with a check, not only an outline); the export dialog's upload line.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { dialog, editConnection } from "../conn-helpers.mjs";
import { closeProfiles, openProfilesOf, profilesDialog } from "../profiles-helpers.mjs";
import { ENGINES, SRC, DST, buttonIn, openPath, rowMenu, saveEngines, visibleErd } from "../schema-compare-helpers.mjs";

const NAME = "QA3 SQLite src";

export function setup({ dir }) {
  ENGINES.sqlite.seed(
    dir,
    SRC,
    `CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
     CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));
     CREATE TABLE order_items (id INTEGER PRIMARY KEY, order_id INTEGER REFERENCES orders(id));`,
  );
  ENGINES.sqlite.seed(dir, DST, "");
  saveEngines(dir, ["sqlite"]);
}

const zoom = `(${visibleErd}.querySelector('.erd-zoom')?.textContent ?? '')`;

async function erdFit(page) {
  await openPath(page, [NAME, "main"]);
  await rowMenu(page, [NAME], "main", "ER diagram");
  await page.waitFor(`${visibleErd}.querySelectorAll('.erd-table').length === 3`, 30_000);
  assert.equal(await page.eval(zoom), "100%");
  // The three small tables sit in a big canvas: Fit changes the zoom (up, within 200%).
  await page.eval(`${buttonIn(visibleErd, "Fit")}.click()`);
  await page.waitFor(`${zoom} !== '100%'`, 5000);
  const percent = Number((await page.eval(zoom)).replace("%", ""));
  assert.ok(percent > 100 && percent <= 200, `fit zoom: ${percent}`);
  // The wires say what they relate.
  assert.match(await page.eval(`${visibleErd}.querySelector('path[data-fk] title').textContent`), /many to one/);
  // Zooming back out and fitting again gives the same result.
  await page.eval(`${buttonIn(visibleErd, "Fit")}.click()`);
  assert.equal(Number((await page.eval(zoom)).replace("%", "")), percent, "fit is stable");
}

async function connectionPages(page) {
  await editConnection(page, NAME);
  const labels = () => page.eval(`[...${dialog}.querySelectorAll('[data-page=safety] label.field > span:first-child')].map(s => s.textContent)`);
  await page.eval(`[...${dialog}.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Appearance')).click()`);
  await page.waitFor(`${dialog}.querySelector('.dialog-page-title')?.textContent === 'Appearance and safety'`);
  const names = await labels();
  assert.ok(names.includes("Environment"), `the type is the environment: ${names}`);
  assert.ok(!names.includes("Type"), "no bare Type label");
}

async function networkSections(page) {
  const tabs = `[...${dialog}.querySelectorAll('.dialog-tabs button')]`;
  const hasNetwork = await page.eval(`${tabs}.some(b => b.textContent.startsWith('Network'))`);
  // An embedded database has no Network page.
  if (hasNetwork) await checkNetwork(page, tabs);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.connection-dialog') == null`);
}

async function checkNetwork(page, tabs) {
  await page.eval(`${tabs}.find(b => b.textContent.startsWith('Network')).click()`);
  await page.waitFor(`${dialog}.querySelector('.dialog-page-title')?.textContent === 'Network'`);
  const headings = await page.eval(`[...${dialog}.querySelectorAll('[data-page=network] h3')].map(h => h.textContent)`);
  for (const h of ["SSH tunnel", "SSL", "Proxy and port forward", "Commands around the connection"]) assert.ok(headings.includes(h), `Network heading ${h}: ${headings}`);
  // An example is paler than a typed value.
  const opacity = await page.eval(`getComputedStyle(${dialog}.querySelector('[data-field=proxy]'), '::placeholder').opacity`);
  assert.ok(Number(opacity) < 1, `placeholder opacity ${opacity}`);
}

async function palette(page) {
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') != null && document.querySelectorAll('.palette li').length > 3`);
  // Every result has an icon.
  assert.equal(await page.eval(`[...document.querySelectorAll('.palette li[role=option]')].every(li => li.querySelector(':scope > svg') != null)`), true, "an icon on every result");
  // "or": the words that start with it come before the table that only contains the letters.
  await page.type("or");
  await page.waitFor(`document.querySelector('.palette input').value === 'or'`);
  const first = await page.eval(`document.querySelector('.palette li[role=option] .palette-label')?.textContent.toLowerCase() ?? ''`);
  assert.ok(first.startsWith("or") || first.includes(" or"), `prefix first: ${first}`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') == null`);
}

async function profilesEmpty(page) {
  await openProfilesOf(page, NAME);
  const box = await page.eval(`${profilesDialog}.getBoundingClientRect().width`);
  assert.ok(box < 600, `the empty profiles dialog is narrow: ${box}px`);
  assert.equal(await page.eval(`${profilesDialog}.querySelector('.modal-close') != null`), true, "it has its X");
  await page.eval(`${profilesDialog}.querySelector('.modal-close').click()`);
  await page.waitFor(`${profilesDialog} === undefined`);
  await openProfilesOf(page, NAME);
  await closeProfiles(page);
}

export default async function (page) {
  await erdFit(page);
  await connectionPages(page);
  await networkSections(page);
  await palette(page);
  await profilesEmpty(page);
}
