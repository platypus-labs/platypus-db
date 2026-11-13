// Elasticsearch and OpenSearch from the `search` profile of docker/compose.yml, connected from
// the connection dialog: the form (Basic login, then an API key in the password field with an
// empty user), Test connection (the product and version, a wrong password, a wrong or missing
// API key, a closed port, TLS against a plain HTTP port, no login at all), the URL mode
// (`elasticsearch://user:pass@host:port/pattern`, a key-only URL, `elasticsearchs://` turning
// TLS on, `opensearch://` against the OpenSearch container when it runs), saving and editing
// the connection, and the saved connection listing its indices.
// Skipped when Elasticsearch is not running.
import assert from "node:assert/strict";
import {
  button,
  dialog,
  editConnection,
  fill,
  fillForm,
  formField,
  newConnection,
  picker,
  saveForm,
  seedSqlite,
  testConnection,
  urlField,
} from "../conn-helpers.mjs";
import { OPENSEARCH_PORT, PASSWORD, PORT, USER, apiKey, drop, openSearchUp, seed, skip as skipSearch } from "../search-helpers.mjs";
import { treeRow } from "../editor-helpers.mjs";

const TILE = "Elasticsearch / OpenSearch";
const WRONG = "e2e-WRONG-pw-4471";
let key;

export const skip = skipSearch;
export async function setup({ dir }) {
  seedSqlite(dir);
  key = await apiKey("e2e-connect");
  await seed("e2e_conn_a", { properties: { n: { type: "long" } } }, [{ n: 1 }]);
  await seed("e2e_conn_b", { properties: { n: { type: "long" } } }, [{ n: 2 }]);
}

/** Closes the open form without saving. */
async function cancel(page) {
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null && ${picker} === null`);
}

/** Opens a new Elasticsearch form and fills the host fields. */
async function form(page, fields) {
  await newConnection(page, TILE);
  await fillForm(page, { Host: "127.0.0.1", Port: PORT, ...fields });
}

/** Opens a new Elasticsearch form in URL mode with `url` typed in. */
async function byUrl(page, url) {
  await newConnection(page, TILE);
  await page.eval(`document.querySelector('.connection-dialog [data-field="connect_by_url"]').click()`);
  await page.waitFor(`document.querySelector(${JSON.stringify(urlField)}) !== null`);
  await page.eval(fill(`document.querySelector(${JSON.stringify(urlField)})`, url));
  // The settings follow the URL once the backend has read it.
  await page.sleep(500);
}

/** Back to the host fields of a form opened by URL. */
async function toFields(page) {
  await page.eval(`document.querySelector('.connection-dialog [data-field="connect_by_host"]').click()`);
  await page.waitFor(`${formField("Host")} !== undefined`);
}

const field = (page, label) => page.eval(`${formField(label)}.value`);

export default async function (page) {
  // The form of the driver: its own labels and its default port.
  await newConnection(page, TILE);
  assert.equal(await field(page, "Port"), "9200");
  assert.ok(await page.eval(`${formField("Index pattern (optional)")} !== undefined`), "index pattern field");
  assert.ok(await page.eval(`${formField("Password or API key")} !== undefined`), "password or API key field");
  assert.ok(await page.eval(`${formField("Database")} == null`), "no Database field");
  await cancel(page);

  // Basic login: the right password connects and names the product; a wrong one is refused
  // with the server's words and never repeats the password.
  await form(page, { User: USER, "Password or API key": PASSWORD });
  assert.match(await testConnection(page), /^Connected: Elasticsearch 8\./);
  await page.eval(fill(formField("Password or API key"), WRONG));
  const refused = await testConnection(page);
  assert.doesNotMatch(refused, /^Connected/, "a wrong password connected");
  assert.match(refused, /security_exception|unable to authenticate|unauthorized|401/i, refused);
  assert.ok(!refused.includes(WRONG), `the password shows: ${refused}`);
  // A user that does not exist is refused too.
  await page.eval(fill(formField("User"), "nobody"));
  await page.eval(fill(formField("Password or API key"), PASSWORD));
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  // No login at all: a cluster with security on answers 401.
  await page.eval(fill(formField("User"), ""));
  await page.eval(fill(formField("Password or API key"), ""));
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  await cancel(page);

  // An API key goes in the password field with an empty user.
  await form(page, { User: "", "Password or API key": key });
  assert.match(await testConnection(page), /^Connected: Elasticsearch 8\./);
  // A key the cluster does not know, and one that is not even base64.
  await page.eval(fill(formField("Password or API key"), Buffer.from("no:such-key").toString("base64")));
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  await page.eval(fill(formField("Password or API key"), "not a key"));
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  // A key with a user name is a Basic login: refused.
  await page.eval(fill(formField("User"), USER));
  await page.eval(fill(formField("Password or API key"), key));
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  await cancel(page);

  // A closed port is an error with a reason, not a hang.
  await form(page, { Port: 59299, User: USER, "Password or API key": PASSWORD });
  const closed = await testConnection(page);
  assert.doesNotMatch(closed, /^Connected/);
  assert.ok(closed.length > 0, "an empty error");
  await cancel(page);

  // TLS against a plain HTTP port fails; the form's SSL mode is what turns it on.
  await form(page, { User: USER, "Password or API key": PASSWORD });
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network')).click()`);
  await page.eval(fill(formField("SSL mode"), "require"));
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('General')).click()`);
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  await cancel(page);

  // URL mode: the user, password, host, port and index pattern of the URL.
  await byUrl(page, `elasticsearch://${USER}:${PASSWORD}@127.0.0.1:${PORT}/e2e_conn_*`);
  assert.match(await testConnection(page), /^Connected: Elasticsearch 8\./);
  await toFields(page);
  assert.equal(await field(page, "Host"), "127.0.0.1");
  assert.equal(await field(page, "Port"), String(PORT));
  assert.equal(await field(page, "User"), USER);
  assert.equal(await field(page, "Index pattern (optional)"), "e2e_conn_*");
  await cancel(page);

  // A URL with a key only (no user) is an API key login. `=` is escaped in the URL.
  await byUrl(page, `elasticsearch://:${encodeURIComponent(key)}@127.0.0.1:${PORT}`);
  assert.match(await testConnection(page), /^Connected: Elasticsearch 8\./);
  await toFields(page);
  assert.equal(await field(page, "User"), "");
  await cancel(page);

  // `elasticsearchs://` is TLS: the SSL mode is no longer disable, and a plain port refuses.
  await byUrl(page, `elasticsearchs://${USER}:${PASSWORD}@127.0.0.1:${PORT}`);
  await toFields(page);
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network')).click()`);
  assert.notEqual(await field(page, "SSL mode"), "disable");
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('General')).click()`);
  assert.doesNotMatch(await testConnection(page), /^Connected/);
  await cancel(page);

  // OpenSearch has no login in the container; the URL scheme or the server says which it is.
  if (await openSearchUp()) {
    await byUrl(page, `opensearch://127.0.0.1:${OPENSEARCH_PORT}`);
    assert.match(await testConnection(page), /^Connected: OpenSearch 2\./);
    await cancel(page);
    await form(page, { Port: OPENSEARCH_PORT, User: "", "Password or API key": "" });
    assert.match(await testConnection(page), /^Connected: OpenSearch 2\./);
    await cancel(page);
  }

  // Saved from the form, with an index pattern: the navigator lists only the indices it names.
  await form(page, { Name: "E2E ES saved", User: USER, "Password or API key": PASSWORD, "Index pattern (optional)": "e2e_conn_a" });
  assert.match(await testConnection(page), /^Connected/);
  await saveForm(page);
  await page.waitFor(`${treeRow("E2E ES saved")} !== undefined`, 15_000);
  await page.eval(`${treeRow("E2E ES saved")}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("e2e_conn_a")} !== undefined`, 30_000);
  assert.ok(await page.eval(`${treeRow("e2e_conn_b")} === undefined`), "the pattern lists e2e_conn_b");

  // Editing it shows what was saved; the secret is not shown, and an empty field keeps it.
  await editConnection(page, "E2E ES saved");
  assert.equal(await field(page, "Index pattern (optional)"), "e2e_conn_a");
  assert.equal(await field(page, "User"), USER);
  assert.equal(await field(page, "Port"), String(PORT));
  await page.eval(fill(formField("Index pattern (optional)"), "e2e_conn_*"));
  assert.match(await testConnection(page), /^Connected/);
  await saveForm(page);
  await drop("e2e_conn_a", "e2e_conn_b");
}
