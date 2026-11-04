// What the schema tools QA scenarios (scenarios/qa3-schema-*.mjs) share: two schemas (or two
// databases, or two files) per engine seeded from the shell, saved connections to them with the
// password from a command, the backend called straight for setup checks, and the dialogs of
// Check schema… and Compare with… driven from the navigator.
import { spawnSync } from "./proc.mjs";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { clientRoot, installClient } from "./qa-ora.mjs";
import { revealItem } from "./menu-sub.mjs";

export const SRC = "qa3s_src";
export const DST = "qa3s_dst";

function docker(args, input) {
  const r = spawnSync("docker", args, { input, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`docker ${args.slice(0, 3).join(" ")}: ${r.stderr || r.stdout}`);
  return (r.stdout ?? "").replace(/\r/g, "").trim();
}

/** Runs `sql` with psql in the database `db` of the PostgreSQL container. */
export const psql = (sql, db = "platypus") =>
  docker(["exec", "-i", "platypus-db-postgres-1", "psql", "-v", "ON_ERROR_STOP=1", "-q", "-At", "-U", "platypus", "-d", db], sql);
const mysqlRoot = (container, client) => (sql) =>
  docker(["exec", "-i", container, client, "-uroot", "-pplatypus", "--default-character-set=utf8mb4", "-N", "-B"], sql);
const sqlcmd = (sql) =>
  docker(
    ["exec", "-i", "platypus-db-sqlserver-1", "/opt/mssql-tools18/bin/sqlcmd", "-C", "-b", "-S", "localhost", "-U", "sa", "-P", "Platypus-2026", "-d", "platypus", "-h", "-1", "-W", "-f", "65001"],
    sql,
  );
const sqlplus = (user, sql) =>
  docker(
    ["exec", "-i", "platypus-db-oracle-1", "sqlplus", "-s", "-l", user],
    `set define off\nset pagesize 0 feedback off heading off linesize 32767\nwhenever sqlerror exit failure\n${sql}\nexit\n`,
  );

/** `sql` with `S.` before a name standing for `schema.` (or nothing, where the session is in it). */
const at = (sql, prefix) => sql.replaceAll("S.", prefix);

const mssqlDrop = (s) => `IF SCHEMA_ID('${s}') IS NOT NULL BEGIN
DECLARE @f nvarchar(max) = N'';
SELECT @f += N'ALTER TABLE ${s}.' + QUOTENAME(OBJECT_NAME(parent_object_id)) + N' DROP CONSTRAINT ' + QUOTENAME(name) + N';' FROM sys.foreign_keys WHERE schema_id = SCHEMA_ID('${s}');
EXEC (@f);
DECLARE @v nvarchar(max) = N'';
SELECT @v += N'DROP VIEW ${s}.' + QUOTENAME(name) + N';' FROM sys.views WHERE schema_id = SCHEMA_ID('${s}');
EXEC (@v);
DECLARE @t nvarchar(max) = N'';
SELECT @t += N'DROP TABLE ${s}.' + QUOTENAME(name) + N';' FROM sys.tables WHERE schema_id = SCHEMA_ID('${s}');
EXEC (@t);
DROP SCHEMA ${s};
END;
GO`;

/** Drops the user `u` with its objects, ending its sessions first (the app's included). */
const oracleUserDrop = (u) => `BEGIN
  FOR s IN (SELECT sid, serial# AS serial FROM v$session WHERE username = '${u}') LOOP
    EXECUTE IMMEDIATE 'ALTER SYSTEM KILL SESSION ''' || s.sid || ',' || s.serial || ''' IMMEDIATE';
  END LOOP;
  EXECUTE IMMEDIATE 'DROP USER ${u} CASCADE';
EXCEPTION WHEN OTHERS THEN IF SQLCODE != -1918 THEN RAISE; END IF;
END;
/`;

/** The engines: how to seed a schema, the saved connections and the schema label of each. */
export const ENGINES = {
  sqlite: {
    title: "SQLite",
    port: null,
    files: {},
    /** One file per schema name; the schema of both is `main`. */
    seed(dir, name, sql) {
      // Emptied rather than deleted: the app may have the file open.
      const db = new DatabaseSync(join(dir, `${name}.db`));
      db.exec("PRAGMA foreign_keys = OFF");
      for (const { type, name: n } of db.prepare("SELECT type, name FROM sqlite_master WHERE type IN ('view', 'table') AND name NOT LIKE 'sqlite_%' ORDER BY type = 'table'").all()) {
        db.exec(`DROP ${type.toUpperCase()} "${n.replaceAll('"', '""')}"`);
      }
      db.exec(at(sql, ""));
      db.close();
    },
    exec(dir, name, sql) {
      const db = new DatabaseSync(join(dir, `${name}.db`));
      db.exec(at(sql, ""));
      db.close();
    },
    connections: (dir) => [
      { id: "00000000-0000-4000-8000-0000000c3001", name: "QA3 SQLite src", body: `driver = "sqlite"\ndatabase = '${join(dir, `${SRC}.db`)}'` },
      { id: "00000000-0000-4000-8000-0000000c3002", name: "QA3 SQLite dst", body: `driver = "sqlite"\ndatabase = '${join(dir, `${DST}.db`)}'` },
    ],
    schema: () => "main",
  },
  pg: {
    title: "PostgreSQL",
    port: 55432,
    seed: (_dir, name, sql) => psql(`DROP SCHEMA IF EXISTS ${name} CASCADE; CREATE SCHEMA ${name}; SET search_path TO ${name};\n${at(sql, `${name}.`)}`),
    exec: (_dir, name, sql) => psql(`SET search_path TO ${name};\n${at(sql, `${name}.`)}`),
    drop: (name) => psql(`DROP SCHEMA IF EXISTS ${name} CASCADE;`),
    query: (sql) => psql(sql),
    connections: () => [
      {
        id: "00000000-0000-4000-8000-0000000c3101",
        name: "QA3 PG",
        body: `driver = "postgres"\nhost = "127.0.0.1"\nport = 55432\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\nshow_all_databases = true\npassword_command = "echo platypus"`,
      },
    ],
    schema: (name) => name,
  },
  mysql: {
    title: "MySQL",
    port: 53306,
    root: mysqlRoot("platypus-db-mysql-1", "mysql"),
    seed(_dir, name, sql) {
      this.root(`DROP DATABASE IF EXISTS ${name}; CREATE DATABASE ${name}; GRANT ALL PRIVILEGES ON \`qa3s\\_%\`.* TO 'platypus'@'%'; USE ${name};\n${at(sql, `${name}.`)}`);
    },
    exec(_dir, name, sql) {
      this.root(`USE ${name};\n${at(sql, `${name}.`)}`);
    },
    drop(name) {
      this.root(`DROP DATABASE IF EXISTS ${name};`);
    },
    connections: () => [
      {
        id: "00000000-0000-4000-8000-0000000c3201",
        name: "QA3 MySQL",
        body: `driver = "mysql"\nhost = "127.0.0.1"\nport = 53306\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\npassword_command = "echo platypus"`,
      },
    ],
    schema: (name) => name,
  },
  mariadb: {
    title: "MariaDB",
    port: 53307,
    root: mysqlRoot("platypus-db-mariadb-1", "mariadb"),
    seed(_dir, name, sql) {
      this.root(`DROP DATABASE IF EXISTS ${name}; CREATE DATABASE ${name}; GRANT ALL PRIVILEGES ON \`qa3s\\_%\`.* TO 'platypus'@'%'; USE ${name};\n${at(sql, `${name}.`)}`);
    },
    exec(_dir, name, sql) {
      this.root(`USE ${name};\n${at(sql, `${name}.`)}`);
    },
    drop(name) {
      this.root(`DROP DATABASE IF EXISTS ${name};`);
    },
    connections: () => [
      {
        id: "00000000-0000-4000-8000-0000000c3301",
        name: "QA3 MariaDB",
        body: `driver = "mysql"\nhost = "127.0.0.1"\nport = 53307\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\npassword_command = "echo platypus"`,
      },
    ],
    schema: (name) => name,
  },
  mssql: {
    title: "SQL Server",
    port: 51433,
    seed: (_dir, name, sql) => sqlcmd(`${mssqlDrop(name)}\nCREATE SCHEMA ${name};\nGO\n${at(sql, `${name}.`)}\nGO`),
    exec: (_dir, name, sql) => sqlcmd(`${at(sql, `${name}.`)}\nGO`),
    drop: (name) => sqlcmd(mssqlDrop(name)),
    query: (sql) => sqlcmd(`SET NOCOUNT ON;\n${sql}`),
    connections: () => [
      {
        id: "00000000-0000-4000-8000-0000000c3401",
        name: "QA3 MSSQL",
        body: `driver = "sqlserver"\nhost = "127.0.0.1"\nport = 51433\ndatabase = "platypus"\nuser = "sa"\nssl_mode = "disable"\nshow_all_databases = true\npassword_command = "echo Platypus-2026"`,
      },
    ],
    schema: (name) => name,
  },
  oracle: {
    title: "Oracle",
    port: 51521,
    /** One user per schema, each connection logged in as its own. */
    seed(_dir, name, sql) {
      const u = name.toUpperCase();
      sqlplus(
        "system/platypus@FREEPDB1",
        `${oracleUserDrop(u)}\nCREATE USER ${u} IDENTIFIED BY platypus QUOTA UNLIMITED ON USERS;\nGRANT CONNECT, RESOURCE, CREATE VIEW TO ${u};`,
      );
      if (sql.trim()) sqlplus(`${u}/platypus@FREEPDB1`, at(sql, ""));
    },
    exec: (_dir, name, sql) => sqlplus(`${name.toUpperCase()}/platypus@FREEPDB1`, at(sql, "")),
    drop: (name) => sqlplus("system/platypus@FREEPDB1", oracleUserDrop(name.toUpperCase())),
    connections: () =>
      [SRC, DST].map((n, i) => ({
        id: `00000000-0000-4000-8000-0000000c350${i + 1}`,
        name: `QA3 Oracle ${n === SRC ? "src" : "dst"}`,
        body: `driver = "oracle"\nhost = "127.0.0.1"\nport = 51521\ndatabase = "FREEPDB1"\nuser = "${n.toUpperCase()}"\nssl_mode = "disable"\npassword_command = "echo platypus"`,
      })),
    schema: (name) => name.toUpperCase(),
    install: (dir) => installClient(dir),
  },
};

/** A skip() unless the engines in `keys` accept connections (Oracle also needs Instant Client). */
export function needsEngines(...keys) {
  return async () => {
    for (const key of keys) {
      const { port, title } = ENGINES[key];
      if (port === null) continue;
      const up = await new Promise((resolve) => {
        const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
        const done = (ok) => {
          socket.destroy();
          resolve(ok);
        };
        socket.on("connect", () => done(true));
        socket.on("error", () => done(false));
        socket.on("timeout", () => done(false));
      });
      if (!up) return `no ${title} on port ${port} (docker compose -f docker/compose.yml up -d)`;
      if (key === "oracle" && !clientRoot()) return "no Oracle Instant Client (set PLATYPUS_E2E_ORACLE_DRIVERS)";
    }
    return null;
  };
}

/** Saves the connections of `keys` in the profile `dir` (and Instant Client for Oracle). */
export function saveEngines(dir, keys, extra = []) {
  const list = [...keys.flatMap((k) => ENGINES[k].connections(dir)), ...extra];
  for (const k of keys) ENGINES[k].install?.(dir);
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n${list.map((c) => `\n[[connections]]\nid = "${c.id}"\nname = "${c.name}"\n${c.body}\nsave_password = false\n`).join("")}`,
  );
}

/** The connection used for `name` (SRC or DST) of `key`: its id and its label in the tree. */
export const connectionOf = (dir, key, name) => {
  const all = ENGINES[key].connections(dir);
  return all.length === 1 ? all[0] : all[name === SRC ? 0 : 1];
};

// ---- the backend, called straight (session ids are its own, not the navigator's) ----

/** Calls the command `cmd`; resolves to `{ ok }` or `{ err }`. */
export const invoke = (page, cmd, args) =>
  page.eval(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)}).then(ok => ({ ok }), err => ({ err: typeof err === 'string' ? err : (err?.message ?? JSON.stringify(err)) }))`,
  );

export async function session(page, connectionId) {
  const r = await invoke(page, "open_session", { connectionId, password: null, database: null, side: true });
  if (r.err) throw new Error(`open_session ${connectionId}: ${r.err}`);
  return r.ok.session_id;
}

// ---- the UI ----

export const tree = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
export const compareDialog = `document.querySelector('.modal.schema-compare')`;
export const lintDialog = `document.querySelector('.modal.schema-lint')`;
export const visibleQuery = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
export const visibleErd = `[...document.querySelectorAll('.erd')].find(t => t.style.display !== 'none')`;
/** The whole text of the visible query editor (CodeMirror draws only the lines in view). */
export const queryText = `(() => {
  const c = ${visibleQuery}?.querySelector('.editor .cm-content');
  const v = c?.cmTile?.root?.view ?? c?.cmView?.view ?? c?.cmView?.rootView?.view;
  return v ? v.state.doc.toString() : [...(c?.querySelectorAll('.cm-line') ?? [])].map(l => l.textContent).join('\\n');
})()`;
export const buttonIn = (scope, text) =>
  `[...(${scope}).querySelectorAll('button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(text)}) || b.getAttribute('aria-label') === ${JSON.stringify(text)})`;

/** The row `label` found after the rows of `chain`, each after the one before. */
export const rowIn = (chain, label) => `(() => {
  const rows = [...document.querySelectorAll('.tree-row')];
  const text = (r) => r.querySelector('.tree-label')?.textContent;
  let at = -1;
  for (const w of ${JSON.stringify(chain)}) {
    at = rows.findIndex((r, i) => i > at && text(r) === w);
    if (at < 0) return undefined;
  }
  return rows.find((r, i) => i > at && text(r) === ${JSON.stringify(label)});
})()`;

/** Expands the tree along `path` until its last label shows. */
export async function openPath(page, path, timeout = 60_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await page.eval(`${rowIn(path.slice(0, -1), path.at(-1))} !== undefined`)) return;
    for (let i = 0; i < path.length - 1; i++) {
      const r = rowIn(path.slice(0, i), path[i]);
      if (await page.eval(`${r} !== undefined && ${r}.getAttribute('aria-expanded') !== 'true'`)) {
        await page.eval(`${r}.querySelector('.twisty').click()`);
        break;
      }
    }
    await page.sleep(400);
  }
  throw new Error(`no tree row ${path.join(" > ")}`);
}

/** Runs the context-menu entry starting with `title` on the row `last` found along `chain`. */
export async function rowMenu(page, chain, last, title) {
  const r = rowIn(chain, last);
  await page.eval(`document.getElementById('qa3-row')?.removeAttribute('id'); ${r}.scrollIntoView({ block: 'center' }); ${r}.querySelector('.tree-label').id = 'qa3-row'`);
  const box = await page.rect("#qa3-row");
  await page.eval(`document.getElementById('qa3-row')?.removeAttribute('id')`);
  await page.click(box.x + 5, box.y + box.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await revealItem(page, title);
  const found = `[...document.querySelectorAll('.context-menu button')].find(b => (b.querySelector('span')?.textContent ?? '').startsWith(${JSON.stringify(title)}))`;
  if (!(await page.eval(`!!${found}`))) {
    const items = await page.eval(`[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`);
    throw new Error(`no menu entry ${title} on ${last}: ${items.join(", ")}`);
  }
  await page.eval(`${found}.click()`);
}

/** Opens Compare with… on the schema `chain`/`last`, picks the target and compares. Resolves
 *  to `{ same: true }`, `{ error }` or `{ sql }` (the editor text of the target's new tab). */
export async function compareUi(page, chain, last, targetConnection, targetSchema, timeout = 60_000) {
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await rowMenu(page, chain, last, "Compare with");
  await page.waitFor(`${compareDialog} !== null`);
  const conn = `${compareDialog}.querySelectorAll('select')[0]`;
  const schemaSelect = `${compareDialog}.querySelectorAll('select')[1]`;
  await page.eval(`(() => {
    const s = ${conn};
    const o = [...s.options].find(o => o.textContent === ${JSON.stringify(targetConnection)});
    if (!o) throw new Error('no target connection ' + ${JSON.stringify(targetConnection)} + ' in ' + [...s.options].map(o => o.textContent));
    if (s.value !== o.value) {
      const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set;
      set.call(s, o.value);
      s.dispatchEvent(new Event('change', { bubbles: true }));
    }
  })()`);
  await page.waitFor(`[...${schemaSelect}.options].some(o => o.textContent === ${JSON.stringify(targetSchema)})`, timeout);
  await page.eval(`(() => {
    const s = ${schemaSelect};
    const set = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set;
    set.call(s, ${JSON.stringify(targetSchema)});
    s.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await page.waitFor(`${schemaSelect}.value === ${JSON.stringify(targetSchema)}`);
  await page.eval(`${buttonIn(compareDialog, "Compare")}.click()`);
  await page.waitFor(
    `${compareDialog} === null || ${compareDialog}.querySelector('.notice') !== null`,
    timeout,
  );
  if (await page.eval(`${compareDialog} !== null`)) {
    const notice = await page.eval(`${compareDialog}.querySelector('.notice').textContent`);
    const error = await page.eval(`${compareDialog}.querySelector('.notice.error') !== null`);
    await page.eval(`${buttonIn(compareDialog, "Cancel")}.click()`);
    await page.waitFor(`${compareDialog} === null`);
    return error ? { error: notice } : { same: true, notice };
  }
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${tabs} && (${queryText}).length > 0`, 15_000);
  return { sql: await page.eval(queryText) };
}

const cancelOf = `[...(${visibleQuery}?.querySelectorAll('.toolbar button') ?? [])].find(b => b.getAttribute('aria-label') === 'Cancel')`;
export const resultError = `(${visibleQuery}?.querySelector('.result-error')?.textContent ?? null)`;

/** Runs the whole editor of the visible query tab as a script; resolves to the error shown or null. */
export async function runEditorScript(page, timeout = 60_000) {
  // Shown nodes, not their texts: another tab's earlier run may read the same ("0 rows affected").
  const shown = `[...document.querySelectorAll('.notice, .error, .result-tab, .result-error')]`;
  await page.eval(`window.__qa3Before = new Set(${shown})`);
  await page.eval(`${buttonIn(visibleQuery, "Script")}.click()`);
  // The end of the run: something new in the results and Cancel disabled again.
  const fresh = `${shown}.filter(n => !window.__qa3Before.has(n)).map(n => n.textContent).filter(t => !t.includes('Executing'))`;
  await page.waitFor(`${fresh}.length > 0 && ${cancelOf}?.disabled === true`, timeout);
  await page.sleep(500);
  await page.waitFor(`${cancelOf}?.disabled === true`, timeout);
  // An error shows in the result, or as a notice when the script stops before any result.
  return page.eval(`${resultError} ?? (${fresh}.find(t => /error|^\\d+:|ORA-/i.test(t)) ?? null)`);
}

/** Compares `last` (found along `chain`) with `targetSchema` of `target`, runs the script
 *  opened on the target and compares again. Resolves to the script, the error of its run
 *  (null when it ran) and what the second comparison gave. */
export async function roundTrip(page, chain, last, target, targetSchema) {
  const first = await compareUi(page, chain, last, target, targetSchema);
  if (!first.sql) return { first };
  const runError = await runEditorScript(page);
  const second = await compareUi(page, chain, last, target, targetSchema);
  return { first, sql: first.sql, runError, second };
}

/** Where the source schema is in the tree of each engine, and the target picked. */
export const PLACES = {
  sqlite: { chain: ["QA3 SQLite src"], last: "main", target: "QA3 SQLite dst", targetSchema: "main" },
  pg: { chain: ["QA3 PG", "platypus"], last: SRC, target: "QA3 PG", targetSchema: DST },
  mysql: { chain: ["QA3 MySQL"], last: SRC, target: "QA3 MySQL", targetSchema: DST },
  mariadb: { chain: ["QA3 MariaDB"], last: SRC, target: "QA3 MariaDB", targetSchema: DST },
  mssql: { chain: ["QA3 MSSQL", "platypus"], last: SRC, target: "QA3 MSSQL", targetSchema: DST },
  oracle: { chain: ["QA3 Oracle src"], last: "QA3S_SRC", target: "QA3 Oracle dst", targetSchema: "QA3S_DST" },
};

/** The regression round trip of `key`: identical schemas need nothing; BASE over VARIANT gives
 *  a script that runs and leaves nothing to change. */
export async function okRoundTrip(page, dir, key) {
  const e = ENGINES[key];
  const p = PLACES[key];
  e.seed(dir, SRC, BASE[key]);
  e.seed(dir, DST, BASE[key]);
  await openPath(page, [...p.chain, p.last]);
  const same = await compareUi(page, p.chain, p.last, p.target, p.targetSchema);
  if (same.same !== true) throw new Error(`identical schemas get a script:\n${same.sql ?? same.error}`);
  e.seed(dir, DST, VARIANT[key]);
  const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
  if (!r.sql) throw new Error(`no script: ${told(r)}`);
  if (r.runError !== null) throw new Error(`the script fails: ${r.runError}\n${r.sql}`);
  if (r.second.same !== true) throw new Error(`a second comparison still finds changes: ${told(r)}`);
  return r;
}

/** A short text of a round trip for assertion messages. */
export const told = (r) => JSON.stringify({ sql: r.sql, runError: r.runError, second: r.second ?? r.first }, null, 1);

// ---- the schemas compared ----

/** A schema with what a comparison must carry over: defaults, NOT NULL, a unique column, a
 *  check, a composite key and a foreign key on it, a self-reference, two tables referencing
 *  each other, odd names and a view. `S.` stands for the schema prefix. */
export const BASE = {
  pg: `CREATE TABLE S.customers (id serial PRIMARY KEY, email varchar(100) NOT NULL UNIQUE, name text DEFAULT 'anon', created timestamp DEFAULT now());
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int NOT NULL REFERENCES S.customers(id) ON DELETE CASCADE, total numeric(10,2) DEFAULT 0, status varchar(20) CHECK (status IN ('new','paid')));
CREATE INDEX orders_cust_status ON S.orders(customer_id, status);
CREATE TABLE S.employees (id int PRIMARY KEY, manager_id int REFERENCES S.employees(id));
CREATE TABLE S.parts (a int, b int, PRIMARY KEY (a, b));
CREATE TABLE S.part_refs (id int PRIMARY KEY, a int, b int, FOREIGN KEY (a, b) REFERENCES S.parts(a, b));
CREATE TABLE S.ca (id int PRIMARY KEY, cb_id int);
CREATE TABLE S.cb (id int PRIMARY KEY, ca_id int REFERENCES S.ca(id));
ALTER TABLE S.ca ADD CONSTRAINT ca_cb FOREIGN KEY (cb_id) REFERENCES S.cb(id);
CREATE TABLE S."Odd Table" ("Weird Col" int PRIMARY KEY, "select" text, "è" int);
CREATE VIEW S.v_orders AS SELECT id, total FROM S.orders;`,
  mysql: `CREATE TABLE customers (id int AUTO_INCREMENT PRIMARY KEY, email varchar(100) NOT NULL UNIQUE, name varchar(50) DEFAULT 'anon', created datetime DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE orders (id bigint PRIMARY KEY, customer_id int NOT NULL, total decimal(10,2) DEFAULT 0, status varchar(20), CONSTRAINT orders_status CHECK (status IN ('new','paid')), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES customers(id) ON DELETE CASCADE);
CREATE INDEX orders_cust_status ON orders(customer_id, status);
CREATE TABLE employees (id int PRIMARY KEY, manager_id int, CONSTRAINT employees_manager FOREIGN KEY (manager_id) REFERENCES employees(id));
CREATE TABLE parts (a int, b int, PRIMARY KEY (a, b));
CREATE TABLE part_refs (id int PRIMARY KEY, a int, b int, CONSTRAINT part_refs_ab FOREIGN KEY (a, b) REFERENCES parts(a, b));
CREATE TABLE ca (id int PRIMARY KEY, cb_id int);
CREATE TABLE cb (id int PRIMARY KEY, ca_id int, CONSTRAINT cb_ca FOREIGN KEY (ca_id) REFERENCES ca(id));
ALTER TABLE ca ADD CONSTRAINT ca_cb FOREIGN KEY (cb_id) REFERENCES cb(id);
CREATE TABLE \`Odd Table\` (\`Weird Col\` int PRIMARY KEY, \`select\` text, \`è\` int);
CREATE VIEW v_orders AS SELECT id, total FROM orders;`,
  mssql: `CREATE TABLE S.customers (id int IDENTITY(1,1) PRIMARY KEY, email varchar(100) NOT NULL UNIQUE, name nvarchar(50) DEFAULT 'anon', created datetime2 DEFAULT sysdatetime());
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int NOT NULL, total decimal(10,2) DEFAULT 0, status varchar(20), CONSTRAINT orders_status CHECK (status IN ('new','paid')), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES S.customers(id) ON DELETE CASCADE);
CREATE INDEX orders_cust_status ON S.orders(customer_id, status);
CREATE TABLE S.employees (id int PRIMARY KEY, manager_id int, CONSTRAINT employees_manager FOREIGN KEY (manager_id) REFERENCES S.employees(id));
CREATE TABLE S.parts (a int, b int, PRIMARY KEY (a, b));
CREATE TABLE S.part_refs (id int PRIMARY KEY, a int, b int, CONSTRAINT part_refs_ab FOREIGN KEY (a, b) REFERENCES S.parts(a, b));
CREATE TABLE S.ca (id int PRIMARY KEY, cb_id int);
CREATE TABLE S.cb (id int PRIMARY KEY, ca_id int, CONSTRAINT cb_ca FOREIGN KEY (ca_id) REFERENCES S.ca(id));
ALTER TABLE S.ca ADD CONSTRAINT ca_cb FOREIGN KEY (cb_id) REFERENCES S.cb(id);
CREATE TABLE S.[Odd Table] ([Weird Col] int PRIMARY KEY, [select] nvarchar(max), [è] int);
GO
CREATE VIEW S.v_orders AS SELECT id, total FROM S.orders;`,
  sqlite: `CREATE TABLE customers (id INTEGER PRIMARY KEY, email varchar(100) NOT NULL UNIQUE, name TEXT DEFAULT 'anon', created TEXT DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE CASCADE, total NUMERIC DEFAULT 0, status TEXT CHECK (status IN ('new','paid')));
CREATE INDEX orders_cust_status ON orders(customer_id, status);
CREATE TABLE employees (id INTEGER PRIMARY KEY, manager_id INTEGER REFERENCES employees(id));
CREATE TABLE parts (a INTEGER, b INTEGER, PRIMARY KEY (a, b));
CREATE TABLE part_refs (id INTEGER PRIMARY KEY, a INTEGER, b INTEGER, FOREIGN KEY (a, b) REFERENCES parts(a, b));
CREATE TABLE ca (id INTEGER PRIMARY KEY, cb_id INTEGER REFERENCES cb(id));
CREATE TABLE cb (id INTEGER PRIMARY KEY, ca_id INTEGER REFERENCES ca(id));
CREATE TABLE "Odd Table" ("Weird Col" INTEGER PRIMARY KEY, "select" TEXT, "è" INTEGER);
CREATE VIEW v_orders AS SELECT id, total FROM orders;`,
  oracle: `CREATE TABLE customers (id NUMBER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, email VARCHAR2(100) NOT NULL UNIQUE, name VARCHAR2(50) DEFAULT 'anon', created TIMESTAMP DEFAULT SYSTIMESTAMP);
CREATE TABLE orders (id NUMBER(19) PRIMARY KEY, customer_id NUMBER NOT NULL, total NUMBER(10,2) DEFAULT 0, status VARCHAR2(20), CONSTRAINT orders_status CHECK (status IN ('new','paid')), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES customers(id) ON DELETE CASCADE);
CREATE INDEX orders_cust_status ON orders(customer_id, status);
CREATE TABLE employees (id NUMBER PRIMARY KEY, manager_id NUMBER, CONSTRAINT employees_manager FOREIGN KEY (manager_id) REFERENCES employees(id));
CREATE TABLE parts (a NUMBER, b NUMBER, PRIMARY KEY (a, b));
CREATE TABLE part_refs (id NUMBER PRIMARY KEY, a NUMBER, b NUMBER, CONSTRAINT part_refs_ab FOREIGN KEY (a, b) REFERENCES parts(a, b));
CREATE TABLE ca (id NUMBER PRIMARY KEY, cb_id NUMBER);
CREATE TABLE cb (id NUMBER PRIMARY KEY, ca_id NUMBER, CONSTRAINT cb_ca FOREIGN KEY (ca_id) REFERENCES ca(id));
ALTER TABLE ca ADD CONSTRAINT ca_cb FOREIGN KEY (cb_id) REFERENCES cb(id);
CREATE TABLE "Odd Table" ("Weird Col" NUMBER PRIMARY KEY, "select" VARCHAR2(100), "è" NUMBER);
CREATE VIEW v_orders AS SELECT id, total FROM orders;`,
};
BASE.mariadb = BASE.mysql;

/** The same tables changed: types, NULL, defaults, an index, a check gone, a column more,
 *  a table missing and two tables more (one referencing the other). */
export const VARIANT = {
  pg: `CREATE TABLE S.customers (id serial PRIMARY KEY, email varchar(50), name text, legacy int);
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int REFERENCES S.customers(id), total numeric(8,2), status varchar(20));
CREATE INDEX orders_cust ON S.orders(customer_id);
CREATE TABLE S.gone_parent (id int PRIMARY KEY);
CREATE TABLE S.gone_child (id int PRIMARY KEY, p int REFERENCES S.gone_parent(id));
CREATE TABLE S.parts (a int, b int, c int, PRIMARY KEY (a, b));`,
  mysql: `CREATE TABLE customers (id int AUTO_INCREMENT PRIMARY KEY, email varchar(50), name varchar(50), legacy int);
CREATE TABLE orders (id bigint PRIMARY KEY, customer_id int, total decimal(8,2), status varchar(20), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES customers(id));
CREATE INDEX orders_cust ON orders(customer_id);
CREATE TABLE gone_parent (id int PRIMARY KEY);
CREATE TABLE gone_child (id int PRIMARY KEY, p int, CONSTRAINT gone_child_p FOREIGN KEY (p) REFERENCES gone_parent(id));
CREATE TABLE parts (a int, b int, c int, PRIMARY KEY (a, b));`,
  mssql: `CREATE TABLE S.customers (id int IDENTITY(1,1) PRIMARY KEY, email varchar(50), name nvarchar(50), legacy int);
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int, total decimal(8,2), status varchar(20), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES S.customers(id));
CREATE INDEX orders_cust ON S.orders(customer_id);
CREATE TABLE S.gone_parent (id int PRIMARY KEY);
CREATE TABLE S.gone_child (id int PRIMARY KEY, p int, CONSTRAINT gone_child_p FOREIGN KEY (p) REFERENCES S.gone_parent(id));
CREATE TABLE S.parts (a int, b int, c int, PRIMARY KEY (a, b));`,
  sqlite: `CREATE TABLE customers (id INTEGER PRIMARY KEY, email varchar(50), name TEXT, legacy INTEGER);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id), total NUMERIC, status TEXT);
CREATE INDEX orders_cust ON orders(customer_id);
CREATE TABLE gone_parent (id INTEGER PRIMARY KEY);
CREATE TABLE gone_child (id INTEGER PRIMARY KEY, p INTEGER REFERENCES gone_parent(id));
CREATE TABLE parts (a INTEGER, b INTEGER, c INTEGER, PRIMARY KEY (a, b));`,
  oracle: `CREATE TABLE customers (id NUMBER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, email VARCHAR2(50), name VARCHAR2(50), legacy NUMBER);
CREATE TABLE orders (id NUMBER(19) PRIMARY KEY, customer_id NUMBER, total NUMBER(8,2), status VARCHAR2(20), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES customers(id));
CREATE INDEX orders_cust ON orders(customer_id);
CREATE TABLE gone_parent (id NUMBER PRIMARY KEY);
CREATE TABLE gone_child (id NUMBER PRIMARY KEY, p NUMBER, CONSTRAINT gone_child_p FOREIGN KEY (p) REFERENCES gone_parent(id));
CREATE TABLE parts (a NUMBER, b NUMBER, c NUMBER, PRIMARY KEY (a, b));`,
};
VARIANT.mariadb = VARIANT.mysql;

/** A new table referencing a column the comparison adds to a table that exists. */
export const NEW_REF = {
  pg: [
    `CREATE TABLE S.accounts (id int PRIMARY KEY, code varchar(10) NOT NULL UNIQUE);
CREATE TABLE S.vouchers (id int PRIMARY KEY, account_code varchar(10) REFERENCES S.accounts(code));`,
    `CREATE TABLE S.accounts (id int PRIMARY KEY);`,
  ],
  mysql: [
    `CREATE TABLE accounts (id int PRIMARY KEY, code varchar(10) NOT NULL UNIQUE);
CREATE TABLE vouchers (id int PRIMARY KEY, account_code varchar(10), CONSTRAINT vouchers_account FOREIGN KEY (account_code) REFERENCES accounts(code));`,
    `CREATE TABLE accounts (id int PRIMARY KEY);`,
  ],
  mssql: [
    `CREATE TABLE S.accounts (id int PRIMARY KEY, code varchar(10) NOT NULL CONSTRAINT accounts_code UNIQUE);
CREATE TABLE S.vouchers (id int PRIMARY KEY, account_code varchar(10), CONSTRAINT vouchers_account FOREIGN KEY (account_code) REFERENCES S.accounts(code));`,
    `CREATE TABLE S.accounts (id int PRIMARY KEY);`,
  ],
  sqlite: [
    `CREATE TABLE accounts (id INTEGER PRIMARY KEY, code TEXT NOT NULL UNIQUE);
CREATE TABLE vouchers (id INTEGER PRIMARY KEY, account_code TEXT REFERENCES accounts(code));`,
    `CREATE TABLE accounts (id INTEGER PRIMARY KEY);`,
  ],
  oracle: [
    `CREATE TABLE accounts (id NUMBER PRIMARY KEY, code VARCHAR2(10) NOT NULL CONSTRAINT accounts_code UNIQUE);
CREATE TABLE vouchers (id NUMBER PRIMARY KEY, account_code VARCHAR2(10), CONSTRAINT vouchers_account FOREIGN KEY (account_code) REFERENCES accounts(code));`,
    `CREATE TABLE accounts (id NUMBER PRIMARY KEY);`,
  ],
};
NEW_REF.mariadb = NEW_REF.mysql;
