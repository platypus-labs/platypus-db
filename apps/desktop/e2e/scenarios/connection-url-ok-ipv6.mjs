// Regression guard: IPv6 hosts in the URL mode. `[::1]` reads as the host ::1, prints back in
// brackets and connects (docker/compose.yml publishes PostgreSQL on [::]:55432); a host typed
// as ::1 in Host mode prints in brackets in URL mode; a MongoDB seed list of IPv6 hosts keeps
// its brackets and ports both ways.
import assert from "node:assert/strict";
import { fill, formField, needsPort, seedSqlite, testConnection } from "../conn-helpers.mjs";
import { cancel, enterUrl, generalFields, leaveUrl, newUrlForm, toHost, toUrl, url, urlError } from "../connection-url-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newUrlForm(page, "PostgreSQL");
  await enterUrl(page, "postgres://platypus:platypus@[::1]:55432/platypus?sslmode=disable");
  assert.equal(await page.eval(urlError), "");
  assert.equal(await page.eval(`${url}.value`), "postgres://platypus@[::1]:55432/platypus?sslmode=disable");
  assert.match(await testConnection(page), /^Connected/);
  await toHost(page);
  assert.equal((await generalFields(page)).Host, "::1");
  await page.eval(fill(formField("Host"), "2001:db8::5"));
  await toUrl(page);
  assert.equal(await page.eval(`${url}.value`), "postgres://platypus@[2001:db8::5]:55432/platypus?sslmode=disable");
  await cancel(page);

  await newUrlForm(page, "MongoDB");
  await enterUrl(page, "mongodb://u@[::1]:27017,[::2]:27018/shop?replicaSet=rs0");
  assert.equal(await page.eval(urlError), "");
  await leaveUrl(page);
  const printed = await page.eval(`${url}.value`);
  assert.match(printed, /^mongodb:\/\/u@\[::1\]:27017,\[::2\]:27018\/shop\?/);
  await toHost(page);
  assert.equal((await generalFields(page)).Host, "[::1]:27017,[::2]:27018");
  await toUrl(page);
  assert.equal(await page.eval(`${url}.value`), printed);
  await cancel(page);
}
