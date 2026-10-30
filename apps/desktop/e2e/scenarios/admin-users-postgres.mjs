// Group J against the dev PostgreSQL: the Users and roles section of Server activity lists
// users and roles with their attributes, memberships and privileges, filters them, creates a
// user (the password masked in the preview and the log), edits and renames it, grants and
// revokes privileges on a table and a schema (also only the grant option), adds it to a role
// and takes it out, drops roles with and without what they own, shows what the server
// refuses, and offers nothing to change on a read-only connection.
import { DBS, assert, needs, sh } from "../qa-grid.mjs";
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
  setValue,
  tick,
  usersConnections,
  usersView,
  waitOption,
  waitPreview,
} from "../admin-users.mjs";

const DB = "pg";
const NAME = "QA J PostgreSQL";
const RO_NAME = "QA J PostgreSQL read-only";
const TABLE = "demo.e2e_users_t";
const ROLES = ["e2e_u_new", "e2e_u_web", "e2e_u_app", "e2e_u_tmp", "e2e_u_readers"];
const PASSWORD = "Secr3t-pw!";

export const skip = needs(DB);

// Roles with privileges cannot be dropped: what they were given goes first.
const cleanup = () =>
  sh(
    DB,
    `DROP TABLE IF EXISTS ${TABLE}, demo.e2e_users_owned, demo.e2e_users_owned2;
     ${ROLES.map((r) => `DO $$ BEGIN IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '${r}') THEN DROP OWNED BY ${r}; END IF; END $$;`).join("\n")}
     ${ROLES.map((r) => `DROP ROLE IF EXISTS ${r};`).join("\n")}`,
  );

export function setup({ dir }) {
  usersConnections(dir, [[NAME, DB], [RO_NAME, DB, "read_only = true\n"]], DBS);
}

const errorText = (page) => page.eval(`${inDialog('[data-field="run-error"]')}?.textContent ?? null`);
const rowCount = (page) => page.eval(`${usersView}.querySelectorAll('tr.admin-principal').length`);

/** The list, its counts and its filters; the details of a user and of a role. */
async function checkList(page) {
  const [users, roles] = sh(DB, "SELECT count(*) FILTER (WHERE rolcanlogin), count(*) FILTER (WHERE NOT rolcanlogin) FROM pg_roles WHERE rolname !~ '^pg_'")
    .split("\t");
  assert.equal(await page.eval(`${usersView}.querySelector('.admin-filter[data-kind="user"] strong').textContent`), users);
  assert.equal(await page.eval(`${usersView}.querySelector('.admin-filter[data-kind="role"] strong').textContent`), roles);
  assert.equal(await page.eval(`${principalRow("e2e_u_app")}.dataset.kind`), "user");
  assert.equal(await page.eval(`${principalRow("e2e_u_readers")}.dataset.kind`), "role");

  // The kind chips filter the list, and all shows everything again.
  await page.eval(`${usersView}.querySelector('.admin-filter[data-kind="role"]').click()`);
  await page.waitFor(`${principalRow("e2e_u_app")} === null`);
  assert.equal(String(await rowCount(page)), roles);
  await page.eval(`${usersView}.querySelector('.admin-filter[data-kind="all"]').click()`);
  await page.waitFor(`${principalRow("e2e_u_app")} !== null`);

  // The text filter looks at names and roles; nothing matching says so.
  // The three seeded accounts: e2e_u_app, e2e_u_readers and e2e_u_tmp.
  const search = '[data-view="users"] input[type=search]';
  await setValue(page, search, "e2e_u_");
  await page.waitFor(`${usersView}.querySelectorAll('tr.admin-principal').length === 3`);
  await setValue(page, search, "nothing-like-this");
  await page.waitFor(`${usersView}.querySelector('.admin-empty')?.textContent === 'No users or roles match.'`);
  await setValue(page, search, "");
  await page.waitFor(`${usersView}.querySelectorAll('tr.admin-principal').length > 2`);

  // A user: its limit and the role it belongs to; no host on PostgreSQL.
  await selectPrincipal(page, "e2e_u_app");
  assert.equal(await page.eval(`${detailField("limit")}.querySelector('dd').textContent`), "7");
  assert.equal(await page.eval(detailField("host")), null);
  assert.deepEqual(await chips(page, "member-of"), ["e2e_u_readers"]);
  // A role: its members and what it was given, the grant option marked.
  await selectPrincipal(page, "e2e_u_readers");
  assert.deepEqual(await chips(page, "members"), ["e2e_u_app"]);
  await page.waitFor(`${details}.querySelector('.users-privileges') !== null`, 10_000);
  assert.deepEqual(await privilegeGroups(page), { demo: ["USAGE*"], [TABLE]: ["SELECT"] });
}

/** New user: what stops it, the masked preview, what the server refuses, and the result. */
async function createUser(page) {
  await openDialog(page, newButton("user"));
  assert.match(await waitPreview(page, "Type a name."), /Type a name\./);
  assert.equal(await canRun(page), false);

  await setField(page, "name", "e2e_u_new");
  await setField(page, "password", PASSWORD);
  await setField(page, "repeat", "something else");
  await waitPreview(page, "The two passwords differ.");
  assert.equal(await canRun(page), false);
  await setField(page, "repeat", PASSWORD);
  await setField(page, "limit", "abc");
  await waitPreview(page, "The connection limit is a whole number.");
  await setField(page, "limit", "3");
  await tick(page, 'input[data-flag="create_db"]');
  await tick(page, 'input[data-flag="inherit"]', false);
  const shown = await waitPreview(page, "NOINHERIT");
  assert.ok(
    shown.includes("CREATE USER e2e_u_new WITH LOGIN CREATEDB NOINHERIT CONNECTION LIMIT 3 PASSWORD '********'"),
    shown,
  );
  assert.ok(!shown.includes(PASSWORD), "the preview holds the password");
  // Statements with a masked password are not for the editor.
  assert.equal(await page.eval(`${inDialog("button.link")}.disabled`), true);

  // A name that exists: the server's refusal shows in the dialog, which stays open.
  await setField(page, "name", "e2e_u_app");
  await waitPreview(page, "CREATE USER e2e_u_app");
  await page.eval(`${inDialog('button[data-action="run"]')}.click()`);
  await page.waitFor(`${inDialog('[data-field="run-error"]')} !== null`, 15_000);
  assert.match(await errorText(page), /already exists/);
  await setField(page, "name", "e2e_u_new");
  await waitPreview(page, "CREATE USER e2e_u_new");
  await runDialog(page);

  await page.waitFor(`${principalRow("e2e_u_new")} !== null`, 15_000);
  assert.equal(
    sh(DB, "SELECT rolconnlimit, rolcreatedb, rolinherit, left(rolpassword, 14) FROM pg_authid WHERE rolname = 'e2e_u_new'"),
    "3\tt\tf\tSCRAM-SHA-256$",
  );
  await checkLogged(page, "PASSWORD '********'", PASSWORD);
}

/** Edit: the form starts from the user, nothing to change is not runnable, and a rename
 *  comes after the attributes. */
async function editUser(page) {
  await selectPrincipal(page, "e2e_u_new");
  await openDialog(page, detailButton("edit"));
  assert.equal(await page.eval(inDialog('[data-field="name"]') + ".value"), "e2e_u_new");
  assert.equal(await page.eval(inDialog('[data-field="limit"]') + ".value"), "3");
  assert.equal(await page.eval(inDialog('input[data-flag="create_db"]') + ".checked"), true);
  assert.equal(await page.eval(inDialog('input[data-flag="inherit"]') + ".checked"), false);
  await page.waitFor(`${inDialog('[data-field="preview-error"]')}?.textContent.includes('nothing to change')`, 10_000);
  assert.equal(await canRun(page), false);

  await setField(page, "limit", "");
  await tick(page, 'input[data-flag="inherit"]');
  await tick(page, 'input[data-flag="create_db"]', false);
  await setField(page, "valid-until", "2031-05-06 07:08:09");
  await setField(page, "name", "e2e_u_web");
  await setField(page, "password", "New-Pw-1");
  await setField(page, "repeat", "New-Pw-1");
  const shown = await waitPreview(page, "RENAME TO e2e_u_web");
  assert.ok(
    shown.includes(
      "ALTER ROLE e2e_u_new WITH NOCREATEDB INHERIT CONNECTION LIMIT -1 PASSWORD '********' VALID UNTIL '2031-05-06 07:08:09'",
    ),
    shown,
  );
  await runDialog(page);

  await page.waitFor(`${principalRow("e2e_u_web")} !== null && ${principalRow("e2e_u_new")} === null`, 15_000);
  assert.equal(
    sh(DB, "SELECT rolconnlimit, rolcreatedb, rolinherit, rolvaliduntil::date FROM pg_roles WHERE rolname = 'e2e_u_web'"),
    "-1\tf\tt\t2031-05-06",
  );
  await checkLogged(page, "RENAME TO e2e_u_web", "New-Pw-1");
}

/** Privileges: nothing chosen is not runnable; a grant on a table and a schema; a revoke from
 *  the details that starts from what is given, of the grant option only, then of the rest. */
async function privileges(page) {
  await selectPrincipal(page, "e2e_u_web");
  await openDialog(page, detailButton("privileges"));
  await waitPreview(page, "Choose at least one privilege.");
  assert.equal(await canRun(page), false);
  await waitOption(page, "schema", "demo");
  await setField(page, "schema", "demo");
  await waitOption(page, "table", "e2e_users_t");
  await setField(page, "table", "e2e_users_t");
  await tick(page, 'input[data-privilege="SELECT"]');
  await tick(page, 'input[data-privilege="INSERT"]');
  await tick(page, 'input[data-field="grant-option"]');
  const shown = await waitPreview(page, "WITH GRANT OPTION");
  assert.ok(shown.includes(`GRANT SELECT, INSERT ON TABLE ${TABLE} TO e2e_u_web WITH GRANT OPTION`), shown);
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="${TABLE}"]') !== null`, 15_000);
  assert.deepEqual(await privilegeGroups(page), { [TABLE]: ["INSERT*", "SELECT*"] });
  assert.equal(sh(DB, `SELECT has_table_privilege('e2e_u_web', '${TABLE}', 'INSERT')`), "t");

  // A schema: another kind of object, other privileges.
  await openDialog(page, detailButton("privileges"));
  await setField(page, "target", "schema");
  await page.waitFor(`${inDialog('input[data-privilege="USAGE"]')} !== null`);
  await waitOption(page, "schema", "demo");
  await setField(page, "schema", "demo");
  await tick(page, 'input[data-privilege="USAGE"]');
  assert.ok((await waitPreview(page, "ON SCHEMA demo")).includes("GRANT USAGE ON SCHEMA demo TO e2e_u_web"));
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="demo"]') !== null`, 15_000);

  // Revoke from the details: the dialog starts from the grant; only the grant option first.
  await page.eval(`${details}.querySelector('tr[data-object="${TABLE}"] button[data-action="revoke"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  await waitPreview(page, `REVOKE INSERT, SELECT ON TABLE ${TABLE} FROM e2e_u_web`);
  await tick(page, 'input[data-field="grant-option"]');
  await waitPreview(page, "REVOKE GRANT OPTION FOR INSERT, SELECT");
  await runDialog(page);
  await page.waitFor(`${details}.querySelectorAll('tr[data-object="${TABLE}"] .users-privilege.grantable').length === 0`, 15_000);
  assert.deepEqual((await privilegeGroups(page))[TABLE], ["INSERT", "SELECT"]);
  assert.equal(sh(DB, `SELECT has_table_privilege('e2e_u_web', '${TABLE}', 'INSERT')`), "t");

  await page.eval(`${details}.querySelector('tr[data-object="${TABLE}"] button[data-action="revoke"]').click()`);
  await page.waitFor(`${dialog} !== null`);
  await waitPreview(page, `REVOKE INSERT, SELECT ON TABLE ${TABLE} FROM e2e_u_web`);
  await runDialog(page);
  await page.waitFor(`${details}.querySelector('tr[data-object="${TABLE}"]') === null`, 15_000);
  assert.equal(sh(DB, `SELECT has_table_privilege('e2e_u_web', '${TABLE}', 'INSERT')`), "f");
  assert.deepEqual(Object.keys(await privilegeGroups(page)), ["demo"]);
}

/** Roles: add with the admin option, the error of a role that does not exist, take out from
 *  the chip's ×. */
async function membership(page) {
  await openDialog(page, detailButton("roles"));
  await setField(page, "role", "e2e_u_readers");
  await tick(page, 'input[data-field="admin-option"]');
  const shown = await waitPreview(page, "WITH ADMIN OPTION");
  assert.ok(shown.includes("GRANT e2e_u_readers TO e2e_u_web WITH ADMIN OPTION"), shown);
  await runDialog(page);
  await page.waitFor(`!!${detailField("member-of")}?.querySelector('.users-chip[data-name="e2e_u_readers"]')`, 15_000);
  assert.equal(sh(DB, "SELECT pg_has_role('e2e_u_web', 'e2e_u_readers', 'MEMBER')"), "t");

  await openDialog(page, detailButton("roles"));
  await setField(page, "role", "e2e_u_no_such_role");
  await waitPreview(page, "e2e_u_no_such_role");
  await page.eval(`${inDialog('button[data-action="run"]')}.click()`);
  await page.waitFor(`${inDialog('[data-field="run-error"]')} !== null`, 15_000);
  assert.match(await errorText(page), /does not exist/);
  await cancelDialog(page);

  await page.eval(`${detailField("member-of")}.querySelector('.users-chip[data-name="e2e_u_readers"] .users-chip-remove').click()`);
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await page.eval(inDialog('[data-field="role"]') + ".value"), "e2e_u_readers");
  await waitPreview(page, "REVOKE e2e_u_readers FROM e2e_u_web");
  await runDialog(page);
  await page.waitFor(`${detailField("member-of")} === null`, 15_000);
  assert.equal(sh(DB, "SELECT pg_has_role('e2e_u_web', 'e2e_u_readers', 'MEMBER')"), "f");
}

/** Drop: PostgreSQL refuses a role that owns objects; handing them over, or dropping them,
 *  lets it through. */
async function drops(page) {
  sh(
    DB,
    `CREATE TABLE demo.e2e_users_owned (id int); ALTER TABLE demo.e2e_users_owned OWNER TO e2e_u_web;
     CREATE TABLE demo.e2e_users_owned2 (id int); ALTER TABLE demo.e2e_users_owned2 OWNER TO e2e_u_tmp;`,
  );
  await page.eval(`${usersView}.querySelector('button.admin-filter[data-kind="all"]').click()`);

  await selectPrincipal(page, "e2e_u_web");
  await openDialog(page, detailButton("drop"));
  assert.match(await page.eval(`${dialog}.textContent`), /PostgreSQL refuses to drop a role that owns objects/);
  assert.ok((await waitPreview(page, "DROP ROLE e2e_u_web")).includes("DROP ROLE e2e_u_web;"));
  await page.eval(`${inDialog('button[data-action="run"]')}.click()`);
  await page.waitFor(`${inDialog('[data-field="run-error"]')} !== null`, 15_000);
  assert.match(await errorText(page), /cannot be dropped because some objects depend on it/);
  await tick(page, 'input[data-field="drop-owned"]');
  assert.ok((await waitPreview(page, "DROP OWNED BY e2e_u_web")).includes("DROP OWNED BY e2e_u_web;\nDROP ROLE e2e_u_web;"));
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_u_web")} === null`, 15_000);
  assert.equal(sh(DB, "SELECT count(*) FROM pg_tables WHERE tablename = 'e2e_users_owned'"), "0");

  // Handing over: the table stays, owned by the role chosen.
  await selectPrincipal(page, "e2e_u_tmp");
  await openDialog(page, detailButton("drop"));
  await waitOption(page, "reassign", "e2e_u_app");
  await setField(page, "reassign", "e2e_u_app");
  await waitPreview(page, "REASSIGN OWNED BY e2e_u_tmp TO e2e_u_app");
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_u_tmp")} === null`, 15_000);
  assert.equal(sh(DB, "SELECT tableowner FROM pg_tables WHERE tablename = 'e2e_users_owned2'"), "e2e_u_app");

  // The role with a grant on a table: dropped together with its privileges.
  await selectPrincipal(page, "e2e_u_readers");
  await openDialog(page, detailButton("drop"));
  await tick(page, 'input[data-field="drop-owned"]');
  await waitPreview(page, "DROP OWNED BY e2e_u_readers");
  await runDialog(page);
  await page.waitFor(`${principalRow("e2e_u_readers")} === null`, 15_000);
  assert.equal(sh(DB, "SELECT count(*) FROM pg_roles WHERE rolname = 'e2e_u_readers'"), "0");
}

export default async function (page) {
  cleanup();
  sh(
    DB,
    `CREATE TABLE ${TABLE} (id int);
     CREATE ROLE e2e_u_readers NOLOGIN;
     CREATE ROLE e2e_u_app LOGIN CONNECTION LIMIT 7 IN ROLE e2e_u_readers;
     CREATE ROLE e2e_u_tmp NOLOGIN;
     GRANT SELECT ON ${TABLE} TO e2e_u_readers;
     GRANT USAGE ON SCHEMA demo TO e2e_u_readers WITH GRANT OPTION;`,
  );
  try {
    await openUsers(page, NAME, DB);
    await checkList(page);
    await createUser(page);
    await editUser(page);
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
      assert.equal(await page.eval(`${detailButton(action)}.title`), "The connection is read-only");
    }
    assert.equal(await page.eval(`${dialog}`), null);
  } finally {
    cleanup();
  }
}
