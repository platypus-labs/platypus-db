// Regression guard: the URL mode by keyboard and with a sloppy paste. Tab goes Name ->
// Connect by -> URL field, in that order; text pasted with spaces around it and a trailing
// line break reads as the URL; the Port of Host mode follows a URL without a port (the
// driver's default) and an empty database and user stay empty.
import assert from "node:assert/strict";
import { formField, newConnection, seedSqlite } from "../conn-helpers.mjs";
import { byHost, byUrl, cancel, generalFields, tab, toHost, toUrl, url, urlError } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

const active = `(document.activeElement.dataset.field ?? document.activeElement.closest('label')?.querySelector('span')?.textContent ?? document.activeElement.tagName)`;

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await toUrl(page);
  await page.eval(`${formField("Name")}.focus()`);
  const order = [];
  for (let i = 0; i < 10; i++) {
    await tab(page);
    order.push(await page.eval(active));
  }
  const iUrl = order.indexOf("connection_url");
  assert.ok(iUrl > 0, `Tab order: ${order.join(" > ")}`);
  assert.ok(order.slice(0, iUrl).some((f) => f.startsWith("connect_by_")), `Tab order: ${order.join(" > ")}`);

  // Pasted with spaces and a line break around it.
  await page.eval(`${url}.focus(); ${url}.select()`);
  await page.type("   postgresql://ana@db.example/inventory  \n");
  await page.sleep(500);
  assert.equal(await page.eval(urlError), "");
  await toHost(page);
  let fields = await generalFields(page);
  assert.equal(fields.Host, "db.example");
  assert.equal(fields.Port, "5432");
  assert.equal(fields.Database, "inventory");
  assert.equal(fields.User, "ana");

  // No database, no user.
  await toUrl(page);
  await page.eval(`${url}.focus(); ${url}.select()`);
  await page.type("postgres://db2.example:6000");
  await page.sleep(500);
  await toHost(page);
  fields = await generalFields(page);
  assert.equal(fields.Host, "db2.example");
  assert.equal(fields.Port, "6000");
  assert.equal(fields.Database, "");
  assert.equal(fields.User, "");
  assert.equal(await page.eval(`${byHost}.getAttribute('aria-checked')`), "true");
  assert.equal(await page.eval(`${byUrl}.getAttribute('aria-checked')`), "false");
  await cancel(page);
}
