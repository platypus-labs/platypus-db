// A URL entered over the one the URL field already prints (`postgres://localhost:5432/`) with
// the caret still in it: pasted, it takes the field's place; typed, the two together are
// refused. Either way its password never reaches connections.toml, nor does a URL with its
// password written by hand in the Database field or in the Name field.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, fill, formField, newConnection, paste, seedSqlite } from "../conn-helpers.mjs";
import { cancel, newUrlForm, secretField, toHost, toUrl, url, urlError } from "../qa3-url.mjs";

const URL = "postgres://ana:s3cret@db.example:6000/shop?sslmode=disable";
const save = button(dialog, "Save");

export function setup({ dir }) {
  seedSqlite(dir);
}

function assertNoSecret(dir) {
  const file = join(dir, "connections.toml");
  const text = existsSync(file) ? readFileSync(file, "utf8") : "";
  assert.doesNotMatch(text, /s3cret/, "the password stays out of the file");
  return text;
}

export default async function (page, { dir }) {
  // Pasted with the caret at the end of the printed URL, then in the middle of it.
  for (const caret of ["end", "middle"]) {
    await newUrlForm(page, "PostgreSQL");
    await page.waitFor(`${url}.value !== ''`);
    const at = caret === "end" ? `${url}.value.length` : "9";
    await page.eval(`${url}.focus(); ${url}.setSelectionRange(${at}, ${at})`);
    await page.eval(paste(url, URL));
    await page.waitFor(`${secretField}.value === 's3cret'`);
    assert.equal(await page.eval(urlError), "");
    await page.waitFor(`${url}.value === 'postgres://ana@db.example:6000/shop?sslmode=disable'`);
    await toHost(page);
    assert.equal(await page.eval(`${formField("Host")}.value`), "db.example");
    assert.equal(await page.eval(`${formField("Port")}.value`), "6000");
    assert.equal(await page.eval(`${formField("User")}.value`), "ana");
    await cancel(page);
  }

  // Saved as pasted: the address in its fields, the password out of the file.
  await newUrlForm(page, "PostgreSQL");
  await page.waitFor(`${url}.value !== ''`);
  await page.eval(fill(formField("Name"), "QA pasted"));
  await page.eval(`${url}.focus(); ${url}.setSelectionRange(${url}.value.length, ${url}.value.length)`);
  await page.eval(paste(url, URL));
  await page.eval(`${save}.click()`);
  await page.waitFor(`${dialog} === null`, 15_000);
  const saved = assertNoSecret(dir);
  assert.match(saved, /host = "db\.example"/);
  assert.match(saved, /database = "shop"/);

  // Typed after the printed URL, with no paste to catch: refused, with nothing saved.
  await newUrlForm(page, "PostgreSQL");
  await page.waitFor(`${url}.value !== ''`);
  await page.eval(fill(formField("Name"), "QA typed"));
  await page.eval(`${url}.focus(); ${url}.setSelectionRange(${url}.value.length, ${url}.value.length)`);
  await page.type(URL);
  await page.waitFor(`${urlError}.includes('two URLs')`);
  assert.doesNotMatch(await page.eval(urlError), /s3cret/);
  await page.eval(`${save}.click()`);
  await page.sleep(500);
  assert.equal(await page.eval(`${dialog} !== null`), true, "the form stays open");
  assert.doesNotMatch(assertNoSecret(dir), /QA typed/);

  // Going to the host fields leaves the settings from before the text.
  await toHost(page);
  assert.equal(await page.eval(`${formField("Host")}.value`), "localhost");
  assert.equal(await page.eval(`${secretField}.value`), "");
  await toUrl(page);
  await cancel(page);

  // By hand in the Database field, then in the Name field.
  for (const [field, name] of [["Database", "QA database"], ["Name", URL]]) {
    await newConnection(page, "PostgreSQL");
    await page.eval(fill(formField("Name"), name));
    if (field === "Database") await page.eval(fill(formField("Database"), URL));
    await page.eval(`${save}.click()`);
    await page.waitFor(`document.querySelector('.connection-dialog .notice.error') !== null`, 10_000);
    const message = await page.eval(`document.querySelector('.connection-dialog .notice.error').textContent`);
    assert.match(message, /password/);
    assert.doesNotMatch(message, /s3cret/);
    assertNoSecret(dir);
    await cancel(page);
  }
}
