// Regression guard: a PostgreSQL connection entered as a URL in the form's URL mode. The URL
// fills the settings, its password goes to the (masked) Password field and never shows in the
// URL field; Host and URL modes switch both ways without losing anything; Test connection
// works in both modes; the saved connection keeps no password in connections.toml, opens in
// the navigator, and its form opens again in URL mode with the same URL.
import assert from "node:assert/strict";
import { dialog, editConnection, expand, formField, needsPort, saveForm, seedSqlite, testConnection, treeRow, fill } from "../conn-helpers.mjs";
import { cancel, enterUrl, filesHolding, generalFields, newUrlForm, savedBlock, secretField, toHost, toUrl, url, urlError } from "../qa3-url.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  seedSqlite(dir);
}

const SECRET = "platypus";

export default async function (page, { dir }) {
  await newUrlForm(page, "PostgreSQL");
  await page.eval(fill(formField("Name"), "QA url pg"));
  await enterUrl(page, `postgresql://platypus:${SECRET}@localhost:55432/platypus?sslmode=disable`);
  assert.equal(await page.eval(urlError), "");
  // The password left the URL for the Password field, which masks it.
  assert.equal(await page.eval(`${secretField}.value`), SECRET);
  assert.equal(await page.eval(`${secretField}.type`), "password");
  const printed = await page.eval(`${url}.value`);
  assert.equal(printed, "postgres://platypus@localhost:55432/platypus?sslmode=disable");

  // Test connection in URL mode.
  assert.match(await testConnection(page), /^Connected/);

  // To the host fields and back: same settings, same URL.
  await toHost(page);
  const fields = await generalFields(page);
  assert.equal(fields.Host, "localhost");
  assert.equal(fields.Port, "55432");
  assert.equal(fields.Database, "platypus");
  assert.equal(fields.User, "platypus");
  assert.equal(fields.Name, "QA url pg");
  assert.equal(await page.eval(`${secretField}.value`), SECRET);
  assert.match(await testConnection(page), /^Connected/);
  // A change in Host mode shows in the URL.
  await page.eval(fill(formField("Database"), "postgres"));
  await toUrl(page);
  assert.equal(await page.eval(`${url}.value`), "postgres://platypus@localhost:55432/postgres?sslmode=disable");
  await enterUrl(page, printed);
  await saveForm(page);

  const block = savedBlock(dir, "QA url pg");
  assert.match(block, /host = "localhost"/);
  assert.match(block, /port = 55432/);
  assert.match(block, /ssl_mode = "disable"/);
  assert.doesNotMatch(block, /^password\s*=/m);
  assert.deepEqual(filesHolding(dir, `:${SECRET}@`), [], "the URL with its password was written somewhere");

  // Opens with the saved password.
  await expand(page, "QA url pg");
  await page.waitFor(`${treeRow("public")} !== undefined || ${treeRow("platypus")} !== undefined`, 20_000);

  // Edited later: the form opens by URL as it was saved, with the same URL, and Test
  // connection works with the saved password; the Host fields hold the same settings.
  await editConnection(page, "QA url pg");
  await page.waitFor(`${url}?.value === ${JSON.stringify(printed)}`);
  assert.match(await testConnection(page), /^Connected/);
  await toHost(page);
  assert.equal((await generalFields(page)).Host, "localhost");
  await cancel(page);
  assert.equal(await page.eval(`${dialog} === null`), true);
}
