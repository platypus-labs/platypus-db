// Group J against the dev MySQL: the Users and roles section lists accounts (`user@host`),
// roles, locked accounts and what each was given, creates a user with a host, a limit, a lock
// and an expired password (the password masked in the preview and the log) and a role,
// edits and renames an account, grants and revokes privileges on a table, a database and the
// server (also only the grant option), adds an account to a role and takes it out, drops
// accounts and roles, shows what the server refuses, and offers nothing to change on a
// read-only connection. The app's user is given the rights for the run and loses them after.
import { DBS, assert, needs } from "../qa-grid.mjs";
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
  mysqlRoot,
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

const DB = "mysql";
const NAME = "QA J MySQL";
const RO_NAME = "QA J MySQL read-only";
const PASSWORD = "Secr3t-pw!";
const ACCOUNTS = ["'e2e_u_new'@'10.%'", "'e2e_u_web'@'10.%'", "'e2e_u_app'@'%'"];
const ROLES = ["e2e_u_readers", "e2e_u_newrole"];

export const skip = needs(DB);

/** What the app's user needs to read and change accounts. */
const GRANT_APP = `GRANT CREATE USER, CREATE ROLE, DROP ROLE, ROLE_ADMIN ON *.* TO 'platypus'@'%';
GRANT SELECT ON mysql.* TO 'platypus'@'%';
GRANT ALL ON platypus.* TO 'platypus'@'%' WITH GRANT OPTION;`;

const REVOKE_APP = `REVOKE CREATE USER, CREATE ROLE, DROP ROLE, ROLE_ADMIN ON *.* FROM 'platypus'@'%';
REVOKE SELECT ON mysql.* FROM 'platypus'@'%';
REVOKE GRANT OPTION ON platypus.* FROM 'platypus'@'%';`;

const DROP_ALL = `${ACCOUNTS.map((a) => `DROP USER IF EXISTS ${a};`).join("\n")}
${ROLES.map((r) => `DROP ROLE IF EXISTS '${r}';`).join("\n")}`;

export function setup({ dir }) {
  usersConnections(dir, [[NAME, DB], [RO_NAME, DB, "read_only = true\n"]], DBS);
  mysqlRoot(GRANT_APP);
}

const errorText = (page) => page.eval(`${inDialog('[data-field="run-error"]')}?.textContent ?? null`);

/** The account list: users with their hosts, roles, a locked system account; the details of a
 *  user and a role. */
async function checkList(page) {
  assert.equal(await page.eval(`${principalRow("e2e_u_app")}.dataset.kind`), "user");
  assert.match(await page.eval(`${principalRow("e2e_u_app")}.children[0].textContent`), /^e2e_u_app@%$/);
  assert.equal(await page.eval(`${principalRow("e2e_u_readers")}.dataset.kind`), "role");
  // MySQL's own accounts are locked, and say so.
  assert.match(await page.eval(`${principalRow("mysql.sys")}.textContent`), /Locked/);

  await selectPrincipal(page, "e2e_u_app");
  assert.equal(await page.eval(`${detailField("host")}.querySelector('dd').textContent`), "%");
  assert.equal(await page.eval(`${detailField("limit")}.querySelector('dd').textContent`), "7");
  assert.deepEqual(await chips(page, "member-of"), ["e2e_u_readers"]);
  await page.waitFor(`${details}.querySelector('.users-privileges') !== null`, 10_000);
  assert.deepEqual(await privilegeGroups(page), { platypus: ["INSERT*"] });

  await selectPrincipal(page, "e2e_u_readers");
  assert.deepEqual(await chips(page, "members"), ["e2e_u_app"]);
  await page.waitFor(`${details}.querySelector('tr[data-object="platypus.customers"]') !== null`, 10_000);
  assert.deepEqual(await privilegeGroups(page), { "platypus.customers": ["SELECT"] });
}

/** New user and new role: the fields MySQL has, the masked preview, what the server refuses. */
async function create(page) {
  await openDialog(page, newButton("user"));
  assert.equal(await page.eval(inDialog('[data-field="host"]') + ".value"), "%");
  // MySQL has no expiry date for a password here, and no attributes beyond these two.
  assert.equal(await page.eval(inDialog('[data-field="valid-until"]')), null);
  assert.equal(await page.eval(inDialog('input[data-flag="superuser"]')), null);
  await setField(page, "name", "e2e_u_new");
  await setField(page, "host", "10.%");
  await setField(page, "password", PASSWORD);
  await setField(page, "repeat", PASSWORD);
  await setField(page, "limit", "2");
  await tick(page, 'input[data-flag="locked"]');
  await tick(page, 'input[data-flag="password_expired"]');
  const shown = await waitPreview(page, "PASSWORD EXPIRE");
  assert.ok(
    shown.includes(
      "CREATE USER 'e2e_u_new'@'10.%' IDENTIFIED BY '********' WITH MAX_USER_CONNECTIONS 2 ACCOUNT LOCK PASSWORD EXPIRE",
    ),
    shown,
  );
  assert.ok(!shown.includes(PASSWORD), "the preview holds the password");

  // An account that exists: the server's refusal shows in the dialog.
  await setField(page, "name", "e2e_u_app");
  await setField(page, "host", "%");
  await waitPreview(page, "CREATE USER 'e2e_u_app'@'%'");
  await page.eval(`${inDialog('button[data-action="run"]')}.click()`);
  await page.waitFor(`${inDialog('[data-field="run-error"]')} !== null`, 15_000);
  assert.match(await errorText(page), /CREATE USER failed/);
  await setField(page, "name", "e2e_u_new");
  await setField(page, "host", "10.%");
  await waitPreview(page, "CREATE USER 'e2e_u_new'@'10.%'");
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_u_new")} !== null`, 15_000);
  assert.equal(
    mysqlRoot("SELECT account_locked, password_expired, max_user_connections FROM mysql.user WHERE User = 'e2e_u_new' AND Host = '10.%'"),
    "Y\tY\t2",
  );
  await checkLogged(page, "IDENTIFIED BY '********'", PASSWORD);

  // A role has a name and nothing else.
  await openDialog(page, newButton("role"));
  assert.equal(await page.eval(inDialog('[data-field="host"]')), null);
  assert.equal(await page.eval(inDialog('[data-field="password"]')), null);
  await setField(page, "name", "e2e_u_newrole");
  assert.ok((await waitPreview(page, "CREATE ROLE")).includes("CREATE ROLE 'e2e_u_newrole';"));
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_u_newrole")}?.dataset.kind === 'role'`, 15_000);
}

/** Edit: the host cannot change, a cleared limit is 0, the lock is lifted, and a rename keeps the host. */
async function edit(page) {
  await selectPrincipal(page, "e2e_u_new");
  await openDialog(page, detailButton("edit"));
  assert.equal(await page.eval(inDialog('[data-field="host"]') + ".disabled"), true);
  assert.equal(await page.eval(inDialog('input[data-flag="locked"]') + ".checked"), true);
  await page.waitFor(`${inDialog('[data-field="preview-error"]')}?.textContent.includes('nothing to change')`, 10_000);
  assert.equal(await canRun(page), false);
  await setField(page, "limit", "");
  await tick(page, 'input[data-flag="locked"]', false);
  await setField(page, "name", "e2e_u_web");
  const shown = await waitPreview(page, "RENAME USER");
  assert.ok(shown.includes("ALTER USER 'e2e_u_new'@'10.%' WITH MAX_USER_CONNECTIONS 0 ACCOUNT UNLOCK"), shown);
  assert.ok(shown.includes("RENAME USER 'e2e_u_new'@'10.%' TO 'e2e_u_web'@'10.%'"), shown);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_u_web")} !== null && ${principalRow("e2e_u_new")} === null`, 15_000);
  assert.equal(
    mysqlRoot("SELECT account_locked, max_user_connections FROM mysql.user WHERE User = 'e2e_u_web' AND Host = '10.%'"),
    "N\t0",
  );
}

/** Privileges on a table, a database and the server; ALL says it all; revoking from the details. */
async function privileges(page) {
  await selectPrincipal(page, "e2e_u_web");
  await openDialog(page, detailButton("privileges"));
  await waitOption(page, "schema", "platypus");
  await setField(page, "schema", "platypus");
  await waitOption(page, "table", "customers");
  await setField(page, "table", "customers");
  await tick(page, 'input[data-privilege="SELECT"]');
  await tick(page, 'input[data-privilege="UPDATE"]');
  assert.ok(
    (await waitPreview(page, "UPDATE ON")).includes("GRANT SELECT, UPDATE ON platypus.customers TO 'e2e_u_web'@'10.%';"),
  );
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="platypus.customers"]') !== null`, 15_000);
  assert.deepEqual(await privilegeGroups(page), { "platypus.customers": ["SELECT", "UPDATE"] });

  // The whole server: a global privilege.
  await openDialog(page, detailButton("privileges"));
  await setField(page, "target", "server");
  await tick(page, 'input[data-privilege="PROCESS"]');
  await waitPreview(page, "GRANT PROCESS ON *.* TO 'e2e_u_web'@'10.%'");
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="*.*"]') !== null`, 15_000);

  // A database: ALL replaces the privileges ticked with it.
  await openDialog(page, detailButton("privileges"));
  await setField(page, "target", "database");
  await waitOption(page, "schema", "platypus");
  await setField(page, "schema", "platypus");
  await tick(page, 'input[data-privilege="SELECT"]');
  await tick(page, 'input[data-privilege="ALL"]');
  const all = await waitPreview(page, "GRANT ALL");
  assert.ok(all.includes("GRANT ALL ON platypus.* TO 'e2e_u_web'@'10.%'") && !all.includes("SELECT"), all);
  await cancelDialog(page);

  // The privilege of the server is taken back from the details, starting from what is given.
  await page.eval(`${details}.querySelector('tr[data-object="*.*"] button[data-action="revoke"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  await waitPreview(page, "REVOKE PROCESS ON *.* FROM 'e2e_u_web'@'10.%'");
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="*.*"]') === null`, 15_000);
  assert.equal(mysqlRoot("SHOW GRANTS FOR 'e2e_u_web'@'10.%'").includes("PROCESS"), false);

  // MySQL takes the grant option away from everything on the object: the dialog says so.
  await selectPrincipal(page, "e2e_u_app");
  await page.waitFor(`${details}.querySelector('tr[data-object="platypus"] button[data-action="revoke"]') !== null`, 10_000);
  await page.eval(`${details}.querySelector('tr[data-object="platypus"] button[data-action="revoke"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  await tick(page, 'input[data-field="grant-option"]');
  assert.equal(await page.eval(inDialog('input[data-field="cascade"]')), null, "MySQL has no CASCADE");
  await waitPreview(page, "REVOKE GRANT OPTION ON platypus.* FROM 'e2e_u_app'@'%'");
  await runDialog(page);
  await page.waitFor(`${details}.querySelectorAll('tr[data-object="platypus"] .users-privilege.grantable').length === 0`, 15_000);
  assert.deepEqual(await privilegeGroups(page), { platypus: ["INSERT"] });
  assert.match(mysqlRoot("SHOW GRANTS FOR 'e2e_u_app'@'%'"), /GRANT INSERT ON `platypus`\.\* TO `e2e_u_app`@`%`\n/);
}

/** Roles: added with the admin option, which a revoke cannot take away alone; taken out. */
async function membership(page) {
  await selectPrincipal(page, "e2e_u_web");
  await openDialog(page, detailButton("roles"));
  await setField(page, "role", "e2e_u_readers");
  await tick(page, 'input[data-field="admin-option"]');
  assert.ok(
    (await waitPreview(page, "ADMIN OPTION")).includes("GRANT 'e2e_u_readers' TO 'e2e_u_web'@'10.%' WITH ADMIN OPTION;"),
  );
  await runDialog(page);
  await page.waitFor(`!!${detailField("member-of")}?.querySelector('.users-chip[data-name="e2e_u_readers"]')`, 15_000);
  assert.equal(mysqlRoot("SELECT WITH_ADMIN_OPTION FROM mysql.role_edges WHERE FROM_USER = 'e2e_u_readers' AND TO_USER = 'e2e_u_web'"), "Y");

  // A role that does not exist.
  await openDialog(page, detailButton("roles"));
  await setField(page, "role", "e2e_u_no_such_role");
  await waitPreview(page, "e2e_u_no_such_role");
  await page.eval(`${inDialog('button[data-action="run"]')}.click()`);
  await page.waitFor(`${inDialog('[data-field="run-error"]')} !== null`, 15_000);
  assert.match(await errorText(page), /Unknown authorization ID|e2e_u_no_such_role/);
  await cancelDialog(page);

  await page.eval(`${detailField("member-of")}.querySelector('.users-chip[data-name="e2e_u_readers"] .users-chip-remove').click()`);
  await page.waitFor(`${dialog} !== null`);
  // Taking out has no admin option to keep apart.
  assert.equal(await page.eval(inDialog('input[data-field="admin-option"]')), null);
  await waitPreview(page, "REVOKE 'e2e_u_readers' FROM 'e2e_u_web'@'10.%'");
  await runDialog(page);
  await page.waitFor(`${detailField("member-of")} === null`, 15_000);
  assert.equal(mysqlRoot("SELECT count(*) FROM mysql.role_edges WHERE TO_USER = 'e2e_u_web'"), "0");
}

/** Drop: an account, then the roles; nothing is said of owned objects. */
async function drops(page) {
  await selectPrincipal(page, "e2e_u_web");
  await openDialog(page, detailButton("drop"));
  assert.match(await page.eval(`${dialog}.textContent`), /Objects it owns are not touched\./);
  assert.equal(await page.eval(inDialog('[data-field="drop-owned"]')), null);
  assert.ok((await waitPreview(page, "DROP USER")).includes("DROP USER 'e2e_u_web'@'10.%';"));
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_u_web")} === null`, 15_000);
  assert.equal(mysqlRoot("SELECT count(*) FROM mysql.user WHERE User = 'e2e_u_web'"), "0");

  await selectPrincipal(page, "e2e_u_newrole");
  await openDialog(page, detailButton("drop"));
  assert.ok((await waitPreview(page, "DROP ROLE")).includes("DROP ROLE 'e2e_u_newrole';"));
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_u_newrole")} === null`, 15_000);

  // A role with members is dropped, and its members stay.
  await selectPrincipal(page, "e2e_u_readers");
  await openDialog(page, detailButton("drop"));
  await waitPreview(page, "DROP ROLE 'e2e_u_readers'");
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_u_readers")} === null`, 15_000);
  assert.notEqual(await page.eval(principalRow("e2e_u_app")), null);
}

export default async function (page) {
  mysqlRoot(DROP_ALL);
  mysqlRoot(`CREATE ROLE 'e2e_u_readers';
CREATE USER 'e2e_u_app'@'%' IDENTIFIED BY 'seed-pw' WITH MAX_USER_CONNECTIONS 7;
GRANT 'e2e_u_readers' TO 'e2e_u_app'@'%';
GRANT SELECT ON platypus.customers TO 'e2e_u_readers';
GRANT INSERT ON platypus.* TO 'e2e_u_app'@'%' WITH GRANT OPTION;`);
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
    await page.waitFor(`${principalRow("e2e_u_app")} !== null`, 15_000);
    assert.equal(await page.eval(`${newButton("user")}.disabled`), true);
    assert.equal(await page.eval(`${newButton("role")}.disabled`), true);
    await selectPrincipal(page, "e2e_u_app");
    for (const action of ["edit", "privileges", "roles", "drop"]) {
      assert.equal(await page.eval(`${detailButton(action)}.disabled`), true, action);
    }
    assert.equal(await page.eval(`${usersView}.querySelectorAll('button[data-action]:not(:disabled)').length`), 0);
  } finally {
    mysqlRoot(DROP_ALL);
    mysqlRoot(REVOKE_APP);
  }
}
