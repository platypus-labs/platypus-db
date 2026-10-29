// Signing in with a cloud's token and reaching the server through a cloud's port forward
// (Connection › Password from, Network › Port forward). The tokens come from the CLIs of
// AWS, Google Cloud and Azure, run at each connection; the forwards from `kubectl` and
// `aws ssm`. What is checked is what the form offers, keeps and shows, and that a test runs
// the CLI and says how it ended: none of them is signed in on the test machine.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, editConnection, fill, formField, saveForm, testConnection } from "../conn-helpers.mjs";

const NAME = "E2E cloud";
const field = (name) => `document.querySelector('.connection-dialog [data-field=${name}]')`;
const source = field("password_source");
const note = `(${field("sign_in_note")}?.textContent ?? '')`;
const networkTab = `[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network'))`;
const generalTab = `[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('General'))`;

export function setup({ dir }) {
  // A closed port: when a CLI gives a token, the connection fails there.
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000f0301"
name = "${NAME}"
driver = "postgres"
host = "127.0.0.1"
port = 1
database = "x"
user = "app"
ssl_mode = "disable"
save_password = true
`,
  );
}

const saved = (dir) => readFileSync(join(dir, "connections.toml"), "utf8");
const optionsOf = (page) => page.eval(`[...${source}.options].map(o => o.value)`);

export default async function (page, { dir }) {
  await editConnection(page, NAME);

  // PostgreSQL offers the three tokens; Windows' sign-in is SQL Server's.
  const offered = await optionsOf(page);
  for (const id of ["aws_iam", "gcp_iam", "entra_id"]) assert.ok(offered.includes(id), `${id} in ${offered}`);
  assert.ok(!offered.includes("windows"), "no Windows sign-in for PostgreSQL");

  // AWS: the command names the host, port, user and region; no password field is left.
  await page.eval(fill(source, "aws_iam"));
  await page.waitFor(`${field("aws_region")} !== null`);
  await page.eval(fill(field("aws_region"), "eu-west-1"));
  await page.waitFor(`${note}.includes('aws rds generate-db-auth-token --hostname 127.0.0.1 --port 1 --username app --region eu-west-1')`);
  assert.equal(await page.eval(`document.querySelector('.connection-dialog input[type=password]')`), null);
  // The test runs the CLI: missing, or not signed in, it says so before any connection.
  assert.match(await testConnection(page, 90_000), /\baws\b/);

  // Entra ID asks Azure for the token of Azure's PostgreSQL; the region field goes.
  await page.eval(fill(source, "entra_id"));
  await page.waitFor(`${note}.includes('az account get-access-token --resource https://ossrdbms-aad.database.windows.net')`);
  assert.equal(await page.eval(`${field("aws_region")}`), null);
  await page.eval(fill(source, "gcp_iam"));
  await page.waitFor(`${note}.includes('gcloud sql generate-login-token')`);

  // Saved as the connection's way to sign in, and chosen again when the form opens.
  await saveForm(page);
  assert.match(saved(dir), /auth = "gcp_iam"/);
  await editConnection(page, NAME);
  assert.equal(await page.eval(`${source}.value`), "gcp_iam");

  // SQL Server offers Entra ID and Windows, and keeps the token it had for another database.
  await page.eval(fill(formField("Driver"), "sqlserver"));
  await page.waitFor(`[...${source}.options].some(o => o.value === 'windows')`);
  const sqlServer = await optionsOf(page);
  assert.ok(sqlServer.includes("entra_id") && sqlServer.includes("gcp_iam"), `${sqlServer}`);
  assert.ok(!sqlServer.includes("aws_iam"), `${sqlServer}`);
  await page.eval(fill(source, "windows"));
  await page.waitFor(`${note}.includes('SSPI')`);
  await page.eval(fill(formField("Driver"), "postgres"));

  // A port forward: kubectl's fields, alone without a proxy.
  await page.eval(`${networkTab}.click()`);
  await page.waitFor(`${field("forward")} !== null`);
  await page.eval(fill(field("forward"), "kubernetes"));
  await page.waitFor(`${field("k8s_target")} !== null`);
  assert.equal(await page.eval(`${field("k8s_target")}.required`), true);
  await page.eval(fill(field("k8s_target"), "svc/postgres"));
  await page.eval(fill(field("k8s_namespace"), "data"));
  await page.eval(fill(field("proxy"), "socks5://127.0.0.1:9"));
  await page.waitFor(`${field("forward_alone")} !== null`);
  await page.eval(fill(field("proxy"), ""));
  await page.waitFor(`${field("forward_alone")} === null`);

  // AWS SSM: the instance, and the region and profile it shares with IAM.
  await page.eval(fill(field("forward"), "ssm"));
  await page.waitFor(`${field("ssm_target")} !== null`);
  assert.equal(await page.eval(`${field("k8s_target")}`), null);
  await page.eval(fill(field("ssm_target"), "i-0abc"));
  await page.waitFor(`${field("forward_note")}?.textContent.includes('aws ssm start-session')`);

  // Back to kubectl, with the password from the keychain: the test starts kubectl, which
  // fails or never forwards, and says so.
  await page.eval(fill(field("forward"), "kubernetes"));
  await page.eval(`${generalTab}.click()`);
  await page.eval(fill(source, "keychain"));
  await page.waitFor(`${note} === ''`);
  assert.match(await testConnection(page, 90_000), /kubectl/);
  await saveForm(page);
  const toml = saved(dir);
  assert.match(toml, /forward = "kubernetes"/);
  assert.match(toml, /k8s_target = "svc\/postgres"/);
  assert.doesNotMatch(toml, /auth = /, "the keychain leaves no auth option");

  // None: the forward and its fields go.
  await editConnection(page, NAME);
  await page.eval(`${networkTab}.click()`);
  await page.waitFor(`${field("forward")}?.value === 'kubernetes'`);
  await page.eval(fill(field("forward"), ""));
  await page.waitFor(`${field("k8s_target")} === null`);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null`);
}
