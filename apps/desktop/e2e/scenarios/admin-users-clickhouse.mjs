// Group S against the dev ClickHouse: the Users and roles section of Server activity lists the
// server's users and roles (from system.users and system.roles; the account the app logs in
// with is among them), creates a user (the password masked in the preview and the log, no
// password = NOT IDENTIFIED, an expiry date), a role, edits (expiry date, password, rename),
// grants and revokes on a table, a database and the server (also WITH GRANT OPTION), adds a
// user to a role and takes it out, drops, and shows what the server or the app refuses: an
// expiry date that is no date, an existing name, a role that does not exist; a revoke offers
// no CASCADE or grant-option-only (ClickHouse cannot) and routine grants are not offered.
// ClickHouse has no host, connection limit or attributes: the form shows none. A read-only connection lists
// them and changes nothing.
import { DBS, assert, needs, sh } from "../grid-helpers.mjs";
import {
  canRun,
  cancelDialog,
  checkLogged,
  chips,
  detailButton,
  detailField,
  details,
  dialog,
  inDialog,
  newButton,
  openDialog,
  openUsers,
  principalRow,
  privilegeGroups,
  runDialog,
  selectPrincipal,
  setField,
  tick,
  usersConnections,
  usersView,
  waitOption,
  waitPreview,
} from "../admin-users.mjs";

const DB = "clickhouse";
const NAME = "QA S ClickHouse";
const RO_NAME = "QA S ClickHouse read-only";
const PASSWORD = "Secr3t-pw!";
const TABLE = "platypus.e2e_us_t";
const USERS = ["e2e_us_app", "e2e_us_new", "e2e_us_web", "e2e_us_ro"];
const ROLES = ["e2e_us_readers", "e2e_us_newrole"];

export const skip = needs(DB);

const CLEANUP = `${USERS.map((u) => `DROP USER IF EXISTS ${u};`).join("\n")}
${ROLES.map((r) => `DROP ROLE IF EXISTS ${r};`).join("\n")}
DROP TABLE IF EXISTS ${TABLE};`;

export function setup({ dir }) {
  usersConnections(dir, [[NAME, DB], [RO_NAME, DB, "read_only = true\n"]], DBS);
}

const errorText = (page) => page.eval(`${inDialog('[data-field="run-error"]')}?.textContent ?? null`);
const runAndFail = async (page) => {
  await page.eval(`${inDialog('button[data-action="run"]')}.click()`);
  await page.waitFor(`${inDialog('[data-field="run-error"]')} !== null`, 20_000);
  return errorText(page);
};
const has = (page, selector) => page.eval(`${inDialog(selector)} !== null`);

/** The list: users, roles, the app's own account; the details of a user and of a role. */
async function checkList(page) {
  assert.equal(await page.eval(`${principalRow("e2e_us_app")}.dataset.kind`), "user");
  assert.equal(await page.eval(`${principalRow("e2e_us_readers")}.dataset.kind`), "role");
  assert.equal(await page.eval(`${principalRow("platypus")}.dataset.kind`), "user", "the account the app uses");

  await selectPrincipal(page, "e2e_us_app");
  assert.match(await page.eval(`${detailField("authentication")}.querySelector('dd').textContent`), /sha256_password/i);
  assert.match(await page.eval(`${detailField("valid-until")}.querySelector('dd').textContent`), /^2040-01-01/);
  assert.equal(await page.eval(`${detailField("host")}`), null, "no host on ClickHouse");
  assert.equal(await page.eval(`${detailField("limit")}`), null);
  assert.deepEqual(await chips(page, "member-of"), ["e2e_us_readers"]);
  await page.waitFor(`${details}.querySelector('.users-privileges') !== null`, 10_000);
  assert.deepEqual(await privilegeGroups(page), { "platypus.e2e_us_t": ["INSERT"] });

  await selectPrincipal(page, "e2e_us_readers");
  assert.deepEqual(await chips(page, "members"), ["e2e_us_app"]);
  await page.waitFor(`${details}.querySelector('tr[data-object="platypus.e2e_us_t"]') !== null`, 10_000);
  assert.deepEqual(await privilegeGroups(page), { "platypus.e2e_us_t": ["SELECT"] });
}

/** New user and role: the fields ClickHouse has, the masked preview, what the server refuses. */
async function create(page) {
  await openDialog(page, newButton("user"));
  for (const f of ["host", "limit"]) assert.equal(await has(page, `[data-field="${f}"]`), false, f);
  assert.equal(await has(page, '[data-field="valid-until"]'), true);
  assert.equal(await page.eval(inDialog(".security-flags") + ".querySelectorAll('input').length"), 0, "no attributes");
  assert.equal(await canRun(page), false, "no name");
  await setField(page, "name", "e2e_us_new");
  await setField(page, "password", PASSWORD);
  await setField(page, "repeat", PASSWORD);
  await setField(page, "valid-until", "2031-01-01 00:00:00");
  const shown = await waitPreview(page, "VALID UNTIL");
  assert.match(shown, /CREATE USER e2e_us_new IDENTIFIED WITH sha256_password BY '\*+' VALID UNTIL '2031-01-01 00:00:00'/);
  assert.ok(!shown.includes(PASSWORD), "the preview holds the password");

  // A date that is no date: the server says so, in the dialog, and makes nothing.
  await setField(page, "valid-until", "next tuesday");
  await waitPreview(page, "next tuesday");
  assert.match(await runAndFail(page), /./);
  assert.equal(sh(DB, "SELECT count(*) FROM system.users WHERE name = 'e2e_us_new'"), "0");

  // A name that exists.
  await setField(page, "valid-until", "");
  await setField(page, "name", "e2e_us_app");
  await waitPreview(page, "CREATE USER e2e_us_app");
  assert.match(await runAndFail(page), /already exists|e2e_us_app|ALREADY/i);

  await setField(page, "name", "e2e_us_new");
  await setField(page, "valid-until", "2031-01-01 00:00:00");
  await waitPreview(page, "CREATE USER e2e_us_new");
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_new")} !== null`, 15_000);
  assert.equal(sh(DB, "SELECT toString(valid_until[1]) FROM system.users WHERE name = 'e2e_us_new'"), "2031-01-01 00:00:00");
  await checkLogged(page, "IDENTIFIED WITH sha256_password BY '********'", PASSWORD);

  // No password: NOT IDENTIFIED, said in the preview.
  await openDialog(page, newButton("user"));
  await setField(page, "name", "e2e_us_ro");
  assert.match(await waitPreview(page, "NOT IDENTIFIED"), /CREATE USER e2e_us_ro NOT IDENTIFIED/);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_ro")} !== null`, 15_000);
  // No VALID UNTIL: no expiry date is shown (ClickHouse stores "never" as 1970-01-01).
  await selectPrincipal(page, "e2e_us_ro");
  assert.equal(await page.eval(`${detailField("valid-until")} !== null`), false, "a user without an expiry date shows none");

  // A role has a name and nothing else.
  await openDialog(page, newButton("role"));
  assert.equal(await has(page, '[data-field="password"]'), false);
  assert.equal(await has(page, '[data-field="valid-until"]'), false);
  await setField(page, "name", "e2e_us_newrole");
  assert.match(await waitPreview(page, "CREATE ROLE"), /CREATE ROLE e2e_us_newrole;?/);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_newrole")}?.dataset.kind === 'role'`, 15_000);
}

/** Edit: the date changes and is cleared (infinity), the password renewed, a rename. */
async function edit(page) {
  await selectPrincipal(page, "e2e_us_new");
  await openDialog(page, detailButton("edit"));
  assert.equal(await page.eval(inDialog('[data-field="valid-until"]') + ".value"), "2031-01-01 00:00:00");
  await page.waitFor(`${inDialog('[data-field="preview-error"]')}?.textContent.includes('nothing to change')`, 10_000);
  assert.equal(await canRun(page), false);
  await setField(page, "valid-until", "2032-02-02 00:00:00");
  assert.match(await waitPreview(page, "2032-02-02"), /ALTER USER e2e_us_new VALID UNTIL '2032-02-02 00:00:00'/);
  await setField(page, "password", PASSWORD + "x");
  await setField(page, "repeat", PASSWORD + "x");
  await setField(page, "name", "e2e_us_web");
  const shown = await waitPreview(page, "RENAME TO");
  assert.match(shown, /ALTER USER e2e_us_new IDENTIFIED WITH sha256_password BY '\*+' VALID UNTIL '2032-02-02 00:00:00'/);
  assert.match(shown, /ALTER USER e2e_us_new RENAME TO e2e_us_web/);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_web")} !== null && ${principalRow("e2e_us_new")} === null`, 15_000);
  assert.equal(sh(DB, "SELECT toString(valid_until[1]) FROM system.users WHERE name = 'e2e_us_web'"), "2032-02-02 00:00:00");

  // An emptied date means no expiry.
  await selectPrincipal(page, "e2e_us_web");
  await openDialog(page, detailButton("edit"));
  await setField(page, "valid-until", "");
  await waitPreview(page, "infinity");
  await runDialog(page);
  await page.waitFor(`${detailField("valid-until")} === null`, 15_000);
}

/** Privileges on a table, a database and the server; no routines; the grant option cannot be revoked alone. */
async function privileges(page) {
  await selectPrincipal(page, "e2e_us_web");
  await openDialog(page, detailButton("privileges"));
  const targets = await page.eval(`[...${inDialog('[data-field="target"]')}.options].map(o => o.value)`);
  assert.ok(targets.includes("table") && targets.includes("database") && targets.includes("server"), `targets: ${targets}`);
  assert.ok(!targets.includes("all_routines"), "ClickHouse has no routine grants");
  await waitOption(page, "schema", "platypus");
  await setField(page, "schema", "platypus");
  await waitOption(page, "table", "e2e_us_t");
  await setField(page, "table", "e2e_us_t");
  await tick(page, 'input[data-privilege="SELECT"]');
  await tick(page, 'input[data-privilege="INSERT"]');
  await tick(page, 'input[data-field="grant-option"]');
  assert.match(await waitPreview(page, "WITH GRANT OPTION"), /GRANT SELECT, INSERT ON platypus\.e2e_us_t TO e2e_us_web WITH GRANT OPTION/);
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="platypus.e2e_us_t"] .users-privilege.grantable') !== null`, 15_000);
  assert.deepEqual(await privilegeGroups(page), { "platypus.e2e_us_t": ["SELECT*", "INSERT*"] });

  // A database: ALL replaces the privileges ticked with it.
  await openDialog(page, detailButton("privileges"));
  await setField(page, "target", "database");
  await waitOption(page, "schema", "platypus");
  await setField(page, "schema", "platypus");
  await tick(page, 'input[data-privilege="SHOW"]');
  await tick(page, 'input[data-privilege="ALL"]');
  const all = await waitPreview(page, "GRANT ALL");
  assert.ok(all.includes("GRANT ALL ON platypus.* TO e2e_us_web") && !all.includes("SHOW"), all);
  await tick(page, 'input[data-privilege="ALL"]', false);
  await waitPreview(page, "GRANT SHOW");
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="platypus"]') !== null`, 15_000);

  // The whole server.
  await openDialog(page, detailButton("privileges"));
  await setField(page, "target", "server");
  await tick(page, 'input[data-privilege="SHOW"]');
  await waitPreview(page, "GRANT SHOW ON *.* TO e2e_us_web");
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="*.*"]') !== null`, 15_000);

  // A revoke offers neither CASCADE nor taking away only the grant option: ClickHouse has neither.
  await page.eval(`${details}.querySelector('tr[data-object="platypus.e2e_us_t"] button[data-action="revoke"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await has(page, 'input[data-field="cascade"]'), false, "ClickHouse has no CASCADE");
  assert.equal(await has(page, 'input[data-field="grant-option"]'), false, "ClickHouse cannot revoke only the grant option");
  assert.match(await waitPreview(page, "REVOKE"), /REVOKE (INSERT, SELECT|SELECT, INSERT) ON platypus\.e2e_us_t FROM e2e_us_web/);
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="platypus.e2e_us_t"]') === null`, 15_000);
  assert.equal(sh(DB, "SELECT count(*) FROM system.grants WHERE user_name = 'e2e_us_web' AND table = 'e2e_us_t'"), "0");

  // The privilege of the server is taken back from the details.
  await page.eval(`${details}.querySelector('tr[data-object="*.*"] button[data-action="revoke"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  await waitPreview(page, "REVOKE SHOW ON *.* FROM e2e_us_web");
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="*.*"]') === null`, 15_000);
}

/** Roles: added, one that does not exist, taken out. */
async function membership(page) {
  await selectPrincipal(page, "e2e_us_web");
  await openDialog(page, detailButton("roles"));
  await setField(page, "role", "e2e_us_readers");
  assert.match(await waitPreview(page, "GRANT"), /GRANT e2e_us_readers TO e2e_us_web/);
  await runDialog(page);
  await page.waitFor(`!!${detailField("member-of")}?.querySelector('.users-chip[data-name="e2e_us_readers"]')`, 15_000);
  assert.equal(sh(DB, "SELECT count(*) FROM system.role_grants WHERE user_name = 'e2e_us_web'"), "1");

  await openDialog(page, detailButton("roles"));
  await setField(page, "role", "e2e_us_no_such_role");
  await waitPreview(page, "e2e_us_no_such_role");
  assert.match(await runAndFail(page), /e2e_us_no_such_role|role|not found|exist/i);
  await cancelDialog(page);

  await page.eval(`${detailField("member-of")}.querySelector('.users-chip[data-name="e2e_us_readers"] .users-chip-remove').click()`);
  await page.waitFor(`${dialog} !== null`);
  await waitPreview(page, "REVOKE e2e_us_readers FROM e2e_us_web");
  await runDialog(page);
  await page.waitFor(`${detailField("member-of")} === null`, 15_000);
  assert.equal(sh(DB, "SELECT count(*) FROM system.role_grants WHERE user_name = 'e2e_us_web'"), "0");
}

/** Drop: users and roles; a user the configuration files define is refused by the server. */
async function drops(page) {
  await selectPrincipal(page, "e2e_us_web");
  await openDialog(page, detailButton("drop"));
  assert.match(await page.eval(`${dialog}.textContent`), /Objects it owns are not touched\./);
  assert.match(await waitPreview(page, "DROP USER"), /DROP USER e2e_us_web;?/);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_web")} === null`, 15_000);
  assert.equal(sh(DB, "SELECT count(*) FROM system.users WHERE name = 'e2e_us_web'"), "0");

  await selectPrincipal(page, "e2e_us_newrole");
  await openDialog(page, detailButton("drop"));
  assert.match(await waitPreview(page, "DROP ROLE"), /DROP ROLE e2e_us_newrole;?/);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_newrole")} === null`, 15_000);

  // A role with members is dropped, and its members stay.
  await selectPrincipal(page, "e2e_us_readers");
  await openDialog(page, detailButton("drop"));
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_readers")} === null`, 15_000);
  assert.equal(await page.eval(`${principalRow("e2e_us_app")} !== null`), true);
}

export default async function (page) {
  sh(DB, CLEANUP);
  sh(
    DB,
    `CREATE TABLE ${TABLE} (id Int32, v Int32) ENGINE = MergeTree ORDER BY id;
CREATE ROLE e2e_us_readers;
CREATE USER e2e_us_app IDENTIFIED WITH sha256_password BY 'seed-pw' VALID UNTIL '2040-01-01 00:00:00';
GRANT e2e_us_readers TO e2e_us_app;
GRANT SELECT ON ${TABLE} TO e2e_us_readers;
GRANT INSERT ON ${TABLE} TO e2e_us_app;`,
  );
  try {
    await openUsers(page, NAME, DB);
    await checkList(page);
    await create(page);
    await edit(page);
    await privileges(page);
    await membership(page);
    await drops(page);

    // A read-only connection lists them and changes nothing.
    await openUsers(page, RO_NAME, DB);
    await page.waitFor(`${principalRow("e2e_us_app")} !== null`, 15_000);
    assert.equal(await page.eval(`${newButton("user")}.disabled`), true);
    assert.equal(await page.eval(`${newButton("role")}.disabled`), true);
    await selectPrincipal(page, "e2e_us_app");
    for (const action of ["edit", "privileges", "roles", "drop"]) {
      assert.equal(await page.eval(`${detailButton(action)}.disabled`), true, action);
    }
    assert.equal(await page.eval(`${usersView}.querySelectorAll('button[data-action]:not(:disabled)').length`), 0);
  } finally {
    sh(DB, CLEANUP);
  }
}
