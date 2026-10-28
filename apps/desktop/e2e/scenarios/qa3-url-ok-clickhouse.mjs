// Regression guard: ClickHouse's URL mode follows its two ports: `https://` without a port is
// TLS on 8443, `http://` without a port plain HTTP on 8123; a connection set in Host mode
// (TLS off on the Docker port) prints a URL that reads back the same, and Test connection
// from it works.
import assert from "node:assert/strict";
import { fill, formField, needsPort, seedSqlite, testConnection } from "../conn-helpers.mjs";
import { cancel, enterUrl, generalFields, leaveUrl, newUrlForm, secretField, toHost, toUrl, url, urlError } from "../qa3-url.mjs";

export const skip = needsPort(58123, "ClickHouse");

export function setup({ dir }) {
  seedSqlite(dir);
}

async function printed(page) {
  await leaveUrl(page);
  return page.eval(`${url}.value`);
}

export default async function (page) {
  await newUrlForm(page, "ClickHouse");
  await enterUrl(page, "https://ana@ch.example/analytics");
  assert.equal(await page.eval(urlError), "");
  assert.equal(await printed(page), "clickhouse://ana@ch.example:8443/analytics?sslmode=verify-full");
  await page.eval(`${url}.focus()`);
  await enterUrl(page, "http://ana@ch.example/analytics");
  assert.equal(await printed(page), "clickhouse://ana@ch.example:8123/analytics?sslmode=disable");

  // Host mode to URL mode and back to a working connection.
  await toHost(page);
  await page.eval(fill(formField("Host"), "localhost"));
  await page.eval(fill(formField("Port"), "58123"));
  await page.eval(fill(formField("User"), "platypus"));
  await page.eval(fill(formField("Database (optional)"), "platypus"));
  await page.eval(fill(secretField, "platypus"));
  await toUrl(page);
  const url1 = await page.eval(`${url}.value`);
  assert.equal(url1, "clickhouse://platypus@localhost:58123/platypus?sslmode=disable");
  await page.eval(`${url}.focus()`);
  await enterUrl(page, url1);
  assert.equal(await printed(page), url1);
  assert.match(await testConnection(page), /^Connected/);
  await toHost(page);
  assert.equal((await generalFields(page)).Port, "58123");
  await cancel(page);
}
