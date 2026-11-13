// Group S against the dev SQL Server: the Users and roles section of Server activity lists
// the users and roles of the database (a user of a login with its login, one without a login,
// the fixed roles left out of the list but offered when adding to a role), creates a user (the
// login and the user, the password masked in the preview and the log, locked = login
// disabled), a user without a login and a role, edits (lock lifted, new password, rename),
// grants and revokes on a table, a schema and the database (CASCADE offered, only the grant
// option), adds to a fixed role and to a custom one and takes out, drops (with and without the
// server login), shows what the server refuses (a weak password, an existing name, a role that
// does not exist) and offers nothing to change on a read-only connection. SQL Server has no
// host, connection limit or expiry date: the form says nothing of them.
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

const DB = "mssql";
const NAME = "QA S SQL Server";
const RO_NAME = "QA S SQL Server read-only";
const PASSWORD = "Str0ng-Pw#2026";
const TABLE = "dbo.e2e_us_t";

export const skip = needs(DB);

const q = (sql) => sh(DB, `SET NOCOUNT ON; ${sql}`);

const CLEANUP = `
IF SCHEMA_ID('e2e_us_sch') IS NOT NULL DROP SCHEMA e2e_us_sch;
DECLARE @sql nvarchar(max) = N'';
SELECT @sql += N'DROP USER [' + name + N'];' FROM sys.database_principals WHERE name LIKE 'e2e_us[_]%' AND type <> 'R';
SELECT @sql += N'DROP ROLE [' + name + N'];' FROM sys.database_principals WHERE name LIKE 'e2e_us[_]%' AND type = 'R';
EXEC (@sql);
SET @sql = N'';
SELECT @sql += N'DROP LOGIN [' + name + N'];' FROM sys.server_principals WHERE name LIKE 'e2e_us[_]%';
EXEC (@sql);
IF OBJECT_ID('${TABLE}') IS NOT NULL DROP TABLE ${TABLE};`;

export function setup({ dir }) {
  usersConnections(dir, [[NAME, DB], [RO_NAME, DB, "read_only = true\n"]], DBS);
}

const errorText = (page) => page.eval(`${inDialog('[data-field="run-error"]')}?.textContent ?? null`);
const runAndFail = async (page) => {
  await page.eval(`${inDialog('button[data-action="run"]')}.click()`);
  await page.waitFor(`${inDialog('[data-field="run-error"]')} !== null`, 20_000);
  return errorText(page);
};

/** The list: users with and without a login, a role; the fixed roles stay out. */
async function checkList(page) {
  assert.equal(await page.eval(`${principalRow("e2e_us_app")}.dataset.kind`), "user");
  assert.equal(await page.eval(`${principalRow("e2e_us_readers")}.dataset.kind`), "role");
  assert.equal(await page.eval(`${principalRow("db_owner")}`), null, "fixed roles are not listed");
  assert.equal(await page.eval(`${principalRow("dbo")}`), null);

  await selectPrincipal(page, "e2e_us_app");
  assert.match(await page.eval(`${detailField("authentication")}.querySelector('dd').textContent`), /INSTANCE|LOGIN|SQL/i);
  assert.equal(await page.eval(`${detailField("host")}`), null, "no host on SQL Server");
  assert.equal(await page.eval(`${detailField("limit")}`), null);
  assert.deepEqual(await chips(page, "member-of"), ["db_datareader", "e2e_us_readers"]);
  await page.waitFor(`${details}.querySelector('.users-privileges') !== null`, 10_000);
  assert.deepEqual(await privilegeGroups(page), { "": ["CONNECT"], "dbo.e2e_us_t": ["INSERT"] }, "CONNECT on the database is every user's");

  // A user with no login has none to show.
  await selectPrincipal(page, "e2e_us_nolog");
  assert.match(await page.eval(`${detailField("authentication")}.querySelector('dd').textContent`), /NONE/i);

  await selectPrincipal(page, "e2e_us_readers");
  assert.deepEqual(await chips(page, "members"), ["e2e_us_app"]);
  await page.waitFor(`${details}.querySelector('tr[data-object="dbo.e2e_us_t"]') !== null`, 10_000);
  assert.deepEqual(await privilegeGroups(page), { "dbo.e2e_us_t": ["SELECT"] });
}

/** New user, user without a login, role: the form's fields, the masked preview, refusals. */
async function create(page) {
  await openDialog(page, newButton("user"));
  // No host, limit or expiry date; the one attribute is Locked.
  for (const f of ["host", "limit", "valid-until"]) {
    assert.equal(await page.eval(inDialog(`[data-field="${f}"]`)), null, f);
  }
  assert.equal(await page.eval(inDialog('input[data-flag="superuser"]')), null);
  assert.equal(await page.eval(inDialog('input[data-flag="locked"]') + ' !== null'), true);
  // Without a name nothing runs; two different passwords stop it too.
  assert.equal(await canRun(page), false);
  await setField(page, "name", "e2e_us_new");
  await setField(page, "password", PASSWORD);
  await setField(page, "repeat", "other");
  await page.waitFor(`${dialog}.textContent.includes('The two passwords differ.')`, 10_000);
  assert.equal(await canRun(page), false, "the passwords differ");
  await setField(page, "repeat", PASSWORD);
  await tick(page, 'input[data-flag="locked"]');
  const shown = await waitPreview(page, "DISABLE");
  assert.match(shown, /CREATE LOGIN \[?e2e_us_new\]? WITH PASSWORD = N?'\*+'/);
  assert.match(shown, /CREATE USER \[?e2e_us_new\]? FOR LOGIN \[?e2e_us_new\]?/);
  assert.match(shown, /ALTER LOGIN \[?e2e_us_new\]? DISABLE/);
  assert.ok(!shown.includes(PASSWORD), "the preview holds the password");

  // A password the server's policy refuses: the error shows in the dialog and nothing is made.
  await setField(page, "password", "abc");
  await setField(page, "repeat", "abc");
  await waitPreview(page, "CREATE LOGIN");
  assert.match(await runAndFail(page), /CREATE LOGIN|password|policy/i);
  assert.equal(q("SELECT count(*) FROM sys.server_principals WHERE name = 'e2e_us_new'"), "0");

  // A name that exists.
  await setField(page, "password", PASSWORD);
  await setField(page, "repeat", PASSWORD);
  await setField(page, "name", "e2e_us_app");
  await waitPreview(page, "CREATE USER");
  assert.match(await runAndFail(page), /already exists|e2e_us_app/i);

  await setField(page, "name", "e2e_us_new");
  await waitPreview(page, "CREATE LOGIN");
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_new")} !== null`, 15_000);
  assert.equal(q("SELECT is_disabled FROM sys.server_principals WHERE name = 'e2e_us_new'"), "1");
  await checkLogged(page, "PASSWORD = '********'", PASSWORD);
  await selectPrincipal(page, "e2e_us_new");
  assert.match(await page.eval(`${detailField("attributes")}.textContent`), /Locked/);

  // No password: a user without a login, said in the preview.
  await openDialog(page, newButton("user"));
  await setField(page, "name", "e2e_us_nologin2");
  const nologin = await waitPreview(page, "WITHOUT LOGIN");
  assert.match(nologin, /CREATE USER \[?e2e_us_nologin2\]? WITHOUT LOGIN/);
  assert.ok(!nologin.includes("CREATE LOGIN"));
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_nologin2")} !== null`, 15_000);

  // A role has a name and no attributes.
  await openDialog(page, newButton("role"));
  assert.equal(await page.eval(inDialog('[data-field="password"]')), null);
  assert.equal(await page.eval(inDialog('input[data-flag="locked"]')), null);
  await setField(page, "name", "e2e_us_newrole");
  assert.match(await waitPreview(page, "CREATE ROLE"), /CREATE ROLE \[?e2e_us_newrole\]?;?/);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_newrole")}?.dataset.kind === 'role'`, 15_000);
}

/** Edit: the lock is lifted and the login renewed; a rename changes the user, not the login. */
async function edit(page) {
  await selectPrincipal(page, "e2e_us_new");
  await openDialog(page, detailButton("edit"));
  assert.equal(await page.eval(inDialog('input[data-flag="locked"]') + ".checked"), true);
  await page.waitFor(`${inDialog('[data-field="preview-error"]')}?.textContent.includes('nothing to change')`, 10_000);
  assert.equal(await canRun(page), false);
  await tick(page, 'input[data-flag="locked"]', false);
  await setField(page, "password", PASSWORD + "x");
  await setField(page, "repeat", PASSWORD + "x");
  await setField(page, "name", "e2e_us_web");
  const shown = await waitPreview(page, "WITH NAME");
  assert.match(shown, /ALTER LOGIN \[?e2e_us_new\]? WITH PASSWORD = N?'\*+'/);
  assert.match(shown, /ALTER LOGIN \[?e2e_us_new\]? ENABLE/);
  assert.match(shown, /ALTER USER \[?e2e_us_new\]? WITH NAME = \[?e2e_us_web\]?/);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_web")} !== null && ${principalRow("e2e_us_new")} === null`, 15_000);
  assert.equal(q("SELECT is_disabled FROM sys.server_principals WHERE name = 'e2e_us_new'"), "0", "the login keeps its name");

  // A user without a login has no login to lock.
  await selectPrincipal(page, "e2e_us_nologin2");
  await openDialog(page, detailButton("edit"));
  await tick(page, 'input[data-flag="locked"]');
  await page.waitFor(`${inDialog('[data-field="preview-error"]')}?.textContent.includes('no server login')`, 10_000);
  assert.equal(await canRun(page), false);
  await cancelDialog(page);
}

/** Privileges on a table, a schema and the database; revoke with CASCADE; the grant option. */
async function privileges(page) {
  await selectPrincipal(page, "e2e_us_web");
  await openDialog(page, detailButton("privileges"));
  await waitOption(page, "schema", "dbo");
  await setField(page, "schema", "dbo");
  await waitOption(page, "table", "e2e_us_t");
  await setField(page, "table", "e2e_us_t");
  await tick(page, 'input[data-privilege="SELECT"]');
  await tick(page, 'input[data-privilege="UPDATE"]');
  assert.match(await waitPreview(page, "UPDATE"), /GRANT SELECT, UPDATE ON OBJECT::\[?dbo\]?\.\[?e2e_us_t\]? TO \[?e2e_us_web\]?/);
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="dbo.e2e_us_t"]') !== null`, 15_000);
  assert.deepEqual(await privilegeGroups(page), { "": ["CONNECT"], "dbo.e2e_us_t": ["SELECT", "UPDATE"] });
  assert.equal(
    q("SELECT count(*) FROM sys.database_permissions WHERE grantee_principal_id = DATABASE_PRINCIPAL_ID('e2e_us_web') AND class = 1"),
    "2",
  );

  // A schema, with the grant option; SQL Server's schema privileges have no ALL.
  await openDialog(page, detailButton("privileges"));
  await setField(page, "target", "schema");
  await waitOption(page, "schema", "dbo");
  await setField(page, "schema", "dbo");
  assert.equal(await page.eval(inDialog('input[data-privilege="ALL"]')), null, "no ALL on a schema");
  await tick(page, 'input[data-privilege="SELECT"]');
  await tick(page, 'input[data-field="grant-option"]');
  assert.match(await waitPreview(page, "WITH GRANT OPTION"), /GRANT SELECT ON SCHEMA::\[?dbo\]? TO \[?e2e_us_web\]? WITH GRANT OPTION/);
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="dbo"] .users-privilege.grantable') !== null`, 15_000);

  // The database: a user already may CONNECT; CREATE TABLE joins it.
  await openDialog(page, detailButton("privileges"));
  await setField(page, "target", "database");
  await tick(page, 'input[data-privilege="CREATE TABLE"]');
  assert.match(await waitPreview(page, "GRANT CREATE TABLE"), /GRANT CREATE TABLE TO [?e2e_us_web]?/);
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object=""] button[data-action="revoke"]') !== null`, 15_000);
  assert.equal(q("SELECT count(*) FROM sys.database_permissions WHERE grantee_principal_id = DATABASE_PRINCIPAL_ID('e2e_us_web') AND class = 0 AND permission_name = 'CREATE TABLE'"), "1");

  // Revoke the schema's: from the details, with CASCADE and only the grant option first.
  await page.eval(`${details}.querySelector('tr[data-object="dbo"] button[data-action="revoke"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await page.eval(inDialog('input[data-field="cascade"]') + ' !== null'), true, "SQL Server offers CASCADE");
  await tick(page, 'input[data-field="grant-option"]');
  await tick(page, 'input[data-field="cascade"]');
  assert.match(
    await waitPreview(page, "REVOKE GRANT OPTION FOR"),
    /REVOKE GRANT OPTION FOR SELECT ON SCHEMA::\[?dbo\]? FROM \[?e2e_us_web\]? CASCADE/,
  );
  await runDialog(page);
  await page.waitFor(`${details}.querySelectorAll('tr[data-object="dbo"] .users-privilege.grantable').length === 0`, 15_000);
  assert.ok(await page.eval(`${details}.querySelector('tr[data-object="dbo"]') !== null`), "the privilege stays, without the option");

  await page.eval(`${details}.querySelector('tr[data-object="dbo"] button[data-action="revoke"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  await waitPreview(page, "REVOKE SELECT ON SCHEMA::");
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="dbo"]') === null`, 15_000);

  // A grant the server refuses: a table that does not exist.
  await openDialog(page, detailButton("privileges"));
  await waitOption(page, "schema", "dbo");
  await setField(page, "schema", "dbo");
  await page.waitFor(`${inDialog('[data-field="table"]')} !== null`);
  await tick(page, 'input[data-privilege="SELECT"]');
  await page.eval(
    `${inDialog('[data-field="table"]')}.insertAdjacentHTML('beforeend', '<option value="no_such_table">no_such_table</option>')`,
  );
  await setField(page, "table", "no_such_table");
  await waitPreview(page, "no_such_table");
  assert.match(await runAndFail(page), /no_such_table|Cannot find|object/i);
  await cancelDialog(page);
}

/** Roles: a fixed role (not in the list) and a custom one; one that does not exist; taken out. */
async function membership(page) {
  await selectPrincipal(page, "e2e_us_web");
  await openDialog(page, detailButton("roles"));
  await setField(page, "role", "db_datawriter");
  // Admin option is for the databases that have it.
  assert.equal(await page.eval(inDialog('input[data-field="admin-option"]')), null);
  assert.match(await waitPreview(page, "ADD MEMBER"), /ALTER ROLE \[?db_datawriter\]? ADD MEMBER \[?e2e_us_web\]?/);
  await runDialog(page);
  await page.waitFor(`!!${detailField("member-of")}?.querySelector('.users-chip[data-name="db_datawriter"]')`, 15_000);
  const members = "SELECT count(*) FROM sys.database_role_members WHERE member_principal_id = DATABASE_PRINCIPAL_ID('e2e_us_web')";
  assert.equal(q(members), "1");

  await openDialog(page, detailButton("roles"));
  await setField(page, "role", "e2e_us_readers");
  await waitPreview(page, "e2e_us_readers");
  await runDialog(page);
  await page.waitFor(`!!${detailField("member-of")}?.querySelector('.users-chip[data-name="e2e_us_readers"]')`, 15_000);

  await openDialog(page, detailButton("roles"));
  await setField(page, "role", "e2e_us_no_such_role");
  await waitPreview(page, "e2e_us_no_such_role");
  assert.match(await runAndFail(page), /e2e_us_no_such_role|role|exist/i);
  await cancelDialog(page);

  await page.eval(`${detailField("member-of")}.querySelector('.users-chip[data-name="db_datawriter"] .users-chip-remove').click()`);
  await page.waitFor(`${dialog} !== null`);
  await waitPreview(page, "DROP MEMBER");
  await runDialog(page);
  await page.waitFor(`${detailField("member-of")}.querySelector('.users-chip[data-name="db_datawriter"]') === null`, 15_000);
  assert.equal(q(members), "1");
}

/** Drop: with the login (offered, ticked), without it, a user with no login, roles. */
async function drops(page) {
  await selectPrincipal(page, "e2e_us_web");
  await openDialog(page, detailButton("drop"));
  assert.equal(await page.eval(inDialog('[data-field="drop-login"]') + ".checked"), true);
  assert.equal(await page.eval(inDialog('[data-field="drop-owned"]')), null);
  assert.match(await waitPreview(page, "DROP LOGIN"), /DROP USER \[?e2e_us_web\]?;?[\s\S]*DROP LOGIN \[?e2e_us_new\]?/);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_web")} === null`, 15_000);
  assert.equal(q("SELECT count(*) FROM sys.server_principals WHERE name = 'e2e_us_new'"), "0");

  // Keep the login of this one.
  await selectPrincipal(page, "e2e_us_app");
  await openDialog(page, detailButton("drop"));
  await tick(page, '[data-field="drop-login"]', false);
  assert.ok(!(await waitPreview(page, "DROP USER")).includes("DROP LOGIN"));
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_app")} === null`, 15_000);
  assert.equal(q("SELECT count(*) FROM sys.server_principals WHERE name = 'e2e_us_app'"), "1", "the login stays");

  // A user without a login has none to drop.
  await selectPrincipal(page, "e2e_us_nolog");
  await openDialog(page, detailButton("drop"));
  assert.equal(await page.eval(inDialog('[data-field="drop-login"]')), null);
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_nolog")} === null`, 15_000);

  // A role that owns a schema is refused; once the schema goes it is dropped.
  await selectPrincipal(page, "e2e_us_readers");
  await openDialog(page, detailButton("drop"));
  await waitPreview(page, "DROP ROLE");
  assert.match(await runAndFail(page), /e2e_us_readers|owns|schema|member/i);
  await cancelDialog(page);
  sh(DB, "DROP SCHEMA e2e_us_sch");
  await selectPrincipal(page, "e2e_us_newrole");
  await openDialog(page, detailButton("drop"));
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_us_newrole")} === null`, 15_000);
}

export default async function (page) {
  sh(DB, CLEANUP);
  sh(
    DB,
    `CREATE TABLE ${TABLE} (id int PRIMARY KEY, v int);
CREATE LOGIN e2e_us_app WITH PASSWORD = '${PASSWORD}';
CREATE USER e2e_us_app FOR LOGIN e2e_us_app;
CREATE USER e2e_us_nolog WITHOUT LOGIN;
CREATE ROLE e2e_us_readers;
GO
CREATE SCHEMA e2e_us_sch AUTHORIZATION e2e_us_readers;
GO
ALTER ROLE e2e_us_readers ADD MEMBER e2e_us_app;
ALTER ROLE db_datareader ADD MEMBER e2e_us_app;
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
    sh(DB, `CREATE USER e2e_us_ro WITHOUT LOGIN;`);
    await openUsers(page, RO_NAME, DB);
    await page.waitFor(`${principalRow("e2e_us_ro")} !== null`, 15_000);
    assert.equal(await page.eval(`${newButton("user")}.disabled`), true);
    assert.equal(await page.eval(`${newButton("role")}.disabled`), true);
    await selectPrincipal(page, "e2e_us_ro");
    for (const action of ["edit", "privileges", "roles", "drop"]) {
      assert.equal(await page.eval(`${detailButton(action)}.disabled`), true, action);
    }
    assert.equal(await page.eval(`${usersView}.querySelectorAll('button[data-action]:not(:disabled)').length`), 0);
  } finally {
    sh(DB, CLEANUP);
  }
}
