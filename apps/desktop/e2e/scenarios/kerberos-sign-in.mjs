// Signing in to PostgreSQL with a Kerberos ticket (Connection › Password from). What the form
// offers and keeps, and what a connection test says. The server is a small one here, which only
// answers the login's first message: the real login, with a real ticket from a real KDC, is in
// crates/platypus-conformance/tests/kerberos.rs. The test machine has no ticket for it.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { button, dialog, editConnection, fill, formField, saveForm, testConnection } from "../conn-helpers.mjs";

const NAME = "E2E kerberos";
const field = (name) => `document.querySelector('.connection-dialog [data-field=${name}]')`;
const source = field("password_source");
const service = field("krb_service");
const note = `(${field("sign_in_note")}?.textContent ?? '')`;
const passwordInput = `document.querySelector('.connection-dialog input[type=password]')`;

/** The authentication request `code` of the protocol: 0 is OK, 3 a password, 7 Kerberos. */
const request = (code) => {
  const message = Buffer.alloc(9);
  message.write("R");
  message.writeInt32BE(8, 1);
  message.writeInt32BE(code, 5);
  return message;
};

/** A server that answers every login with `code()`, and keeps what clients send after that. */
async function loginServer(code) {
  const received = [];
  const server = createServer((socket) => {
    socket.on("error", () => {});
    let startup = false;
    socket.on("data", (data) => {
      if (startup) return received.push(data);
      startup = true;
      socket.write(request(code()));
    });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { port: server.address().port, received, close: () => server.close() };
}

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000f0309"
name = "${NAME}"
driver = "postgres"
host = "localhost"
port = 1
database = "shop"
user = "alice"
ssl_mode = "disable"
save_password = true
`,
  );
}

const saved = (dir) => readFileSync(join(dir, "connections.toml"), "utf8");
const optionsOf = (page) => page.eval(`[...${source}.options].map(o => o.value)`);

const test = (page) => testConnection(page, 90_000);

export default async function (page, { dir }) {
  let answer = 7;
  const server = await loginServer(() => answer);
  try {
    await editConnection(page, NAME);

    // Offered for PostgreSQL, under the password's source, and only for PostgreSQL.
    assert.ok((await optionsOf(page)).includes("kerberos"));
    for (const other of ["mysql", "sqlserver", "clickhouse"]) {
      await page.eval(fill(formField("Driver"), other));
      await page.waitFor(`![...${source}.options].some(o => o.value === 'kerberos')`);
    }
    await page.eval(fill(formField("Driver"), "postgres"));
    await page.waitFor(`[...${source}.options].some(o => o.value === 'kerberos')`);
    await page.eval(fill(formField("Port"), String(server.port)));

    // Chosen: the password field goes, the service name comes, and the note says no password
    // is sent.
    assert.ok(await page.eval(`${passwordInput} !== null`));
    await page.eval(fill(source, "kerberos"));
    await page.waitFor(`${service} !== null`);
    await page.waitFor(`${passwordInput} === null`);
    assert.equal(await page.eval(`${service}.placeholder`), "postgres");
    await page.waitFor(`${note}.includes('No password is sent')`);

    // A test with no ticket says what to do, not only what the system's Kerberos said: `klist`
    // on Windows, `kinit` elsewhere. A build without the feature says that instead.
    const noTicket = await test(page);
    assert.match(noTicket, /klist|kinit|no Kerberos/, noTicket);
    assert.match(noTicket, /Kerberos/, noTicket);
    assert.equal(server.received.length, 0, "no ticket, nothing sent");
    assert.ok(await page.eval(`document.querySelector('.modal input[type=password]') === null`), "no password is asked");

    // The ticket asked for is for the service name in the field, on the connection's host.
    await page.eval(fill(service, "pg-prod"));
    await page.waitFor(`${service}.value === 'pg-prod'`);
    const named = await test(page);
    if (!/no Kerberos/.test(named)) assert.match(named, /pg-prod\/localhost/, named);

    // A service name that would change the principal is refused before anything happens.
    await page.eval(fill(service, "pg/evil"));
    await page.waitFor(`${service}.value === 'pg/evil'`);
    assert.match(await test(page), /service name/);
    await page.eval(fill(service, "pg-prod"));
    await page.waitFor(`${service}.value === 'pg-prod'`);

    // A server that asks for a password gets none; one that lets anybody in is refused.
    answer = 3;
    const asked = await test(page);
    assert.match(asked, /none was sent/, asked);
    assert.equal(server.received.length, 0, "the password stays here");
    answer = 0;
    const open = await test(page);
    assert.match(open, /without asking/, open);
    answer = 7;

    // Saved as the connection's way to sign in, with the service name, and no password.
    await saveForm(page);
    const toml = saved(dir);
    assert.match(toml, /auth = "kerberos"/);
    assert.match(toml, /krb_service = "pg-prod"/);
    await editConnection(page, NAME);
    assert.equal(await page.eval(`${source}.value`), "kerberos");
    await page.waitFor(`${service}?.value === 'pg-prod'`);
    assert.ok(await page.eval(`${passwordInput} === null`));

    // Another way to sign in takes the service name with it.
    await page.eval(fill(source, "keychain"));
    await page.waitFor(`${service} === null`);
    await page.waitFor(`${passwordInput} !== null`);
    await saveForm(page);
    const back = saved(dir);
    assert.doesNotMatch(back, /auth = /);
    assert.doesNotMatch(back, /krb_service/);

    // A password connection is told where the server wants Kerberos, and sends nothing.
    await editConnection(page, NAME);
    await page.eval(fill(source, "ask"));
    await page.waitFor(`${passwordInput} !== null`);
    await page.eval(fill(passwordInput, "typed-password"));
    const wants = await test(page);
    assert.match(wants, /Choose Kerberos/, wants);
    assert.equal(server.received.length, 0);
    await page.eval(`${button(dialog, "Cancel")}.click()`);
    await page.waitFor(`${dialog} === null`);
  } finally {
    server.close();
  }
}
