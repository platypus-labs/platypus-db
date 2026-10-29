// Passwords from a password manager (Connection › Password from): 1Password, Bitwarden,
// HashiCorp Vault and AWS Secrets Manager write their CLI's command from the secret's
// reference, saved as the connection's password command and recognized when the form opens
// again; a reference cannot carry what the shell would run. A command that prints a JSON
// secret gives its `password` field, and one without it says so. No manager is installed
// for the test: what is checked is what the app runs and reads.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, editConnection, fill, saveForm, testConnection } from "../conn-helpers.mjs";

const NAME = "E2E managers";
const source = `document.querySelector('.connection-dialog [data-field=password_source]')`;
const reference = `document.querySelector('.connection-dialog [data-field=secret_reference]')`;
const command = `document.querySelector('.connection-dialog [data-field=password_command]')`;
const shown = `(document.querySelector('.connection-dialog .manager-command code')?.textContent ?? '')`;

export function setup({ dir }) {
  // A closed port: the password is read first, then the connection fails.
  const toml = `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000f0201"
name = "${NAME}"
driver = "postgres"
host = "127.0.0.1"
port = 1
database = "x"
user = "x"
ssl_mode = "disable"
save_password = true
`;
  writeFileSync(join(dir, "connections.toml"), toml);
}

const saved = (dir) => readFileSync(join(dir, "connections.toml"), "utf8");

/** Picks `value` in the Password from list. */
const pick = (page, value) => page.eval(fill(source, value));

export default async function (page, { dir }) {
  await editConnection(page, NAME);
  const options = await page.eval(`[...${source}.options].map(o => o.textContent)`);
  for (const name of ["1Password", "Bitwarden", "HashiCorp Vault", "AWS Secrets Manager"]) {
    assert.ok(options.includes(name), `${name} in ${options}`);
  }

  // Each manager writes its command from the reference; switching keeps the reference.
  await pick(page, "1password");
  await page.waitFor(`${reference} !== null`);
  await page.eval(fill(reference, "op://Work/prod db/password"));
  await page.waitFor(`${shown} === 'op read "op://Work/prod db/password"'`);
  await pick(page, "vault");
  await page.waitFor(`${reference}.value === 'op://Work/prod db/password'`);
  await page.eval(fill(reference, "secret/prod-db#pass"));
  await page.waitFor(`${shown} === 'vault kv get -field="pass" "secret/prod-db"'`);
  await pick(page, "bitwarden");
  await page.waitFor(`${shown} === 'bw get password "secret/prod-db#pass"'`);
  await pick(page, "aws");
  await page.eval(fill(reference, "prod/db"));
  await page.waitFor(`${shown}.startsWith('aws secretsmanager get-secret-value --secret-id "prod/db"')`);

  // What a shell would run is left out of the reference.
  await page.eval(fill(reference, 'prod/db" & calc & "'));
  await page.waitFor(`!${shown}.includes('" & calc')`);
  assert.equal(await page.eval(`${reference}.value`), "prod/db & calc & ");
  await page.eval(fill(reference, "prod/db"));

  // Saved as the password command, and recognized when the form opens again.
  await saveForm(page);
  assert.ok(
    saved(dir).includes('aws secretsmanager get-secret-value --secret-id "prod/db" --query SecretString --output text'),
    "saved as the password command",
  );
  await editConnection(page, NAME);
  assert.equal(await page.eval(`${source}.value`), "aws");
  assert.equal(await page.eval(`${reference}.value`), "prod/db");

  // A hand-written command stays a shell command.
  await pick(page, "command");
  await page.waitFor(`${command} !== null`);

  // A JSON secret without a password field: the test says so before connecting.
  await page.eval(fill(command, 'echo {"username":"ada"}'));
  assert.match(await testConnection(page), /JSON object without a `password` text field/);
  // With one, the password is read and the connection is tried (and refused: closed port).
  await page.eval(fill(command, 'echo {"username":"ada","password":"s3cret"}'));
  const tried = await testConnection(page);
  assert.doesNotMatch(tried, /JSON|password command/, tried);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null`);
}
