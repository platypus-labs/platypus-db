// What the QA scenarios of the less common databases (qa2-dbs-*) share: connections to the
// libSQL, Trino, CockroachDB, YugabyteDB and TimescaleDB containers of docker/compose.yml
// (added to the grid helpers' databases, so `connections`, `expand`, `openEditor`, `run`…
// work on them), and SQL run on them from outside the app.
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DBS, sh, treeRow } from "./qa-grid.mjs";

export * from "./qa-grid.mjs";

const here = dirname(fileURLToPath(import.meta.url));

/** The test-only token of the `libsql` service (crates/platypus-conformance). */
export const LIBSQL_TOKEN =
  "eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9.eyJhIjoicnciLCJpYXQiOjE3NjcyMjU2MDB9." +
  "gcA5ick0W16rnB4ROBvkn896i2bNi3Z80USgUN9a5tEtdq5kigej16LpaCi_sVzxdzVIGUQ1n9eUtayjb2GdAg";

const psql = (container, user, db, host = null) => [
  "exec", "-i", container, ...(host ? ["bin/ysqlsh", "-h", host] : ["psql"]), "-U", user, "-d", db,
  "-v", "ON_ERROR_STOP=1", "-q", "-A", "-t", "-F", "\t",
];

function hostname(container) {
  const r = spawnSync("docker", ["inspect", "-f", "{{.Config.Hostname}}", container], { encoding: "utf8" });
  return r.stdout.trim();
}

Object.assign(DBS, {
  cockroach: {
    port: 56257,
    password: "",
    toml: `driver = "postgres"\nhost = "localhost"\nport = 56257\ndatabase = "platypus"\nuser = "root"\nssl_mode = "disable"\npassword_command = "cmd /c echo."`,
    shell: ["exec", "-i", "platypus-db-cockroach-1", "cockroach", "sql", "--insecure", "-d", "platypus", "--format=tsv"],
    noPrompt: true,
  },
  yugabyte: {
    port: 55434,
    password: "yugabyte",
    toml: `driver = "postgres"\nhost = "localhost"\nport = 55434\ndatabase = "yugabyte"\nuser = "yugabyte"\nssl_mode = "disable"\npassword_command = "echo yugabyte"`,
    get shell() {
      return psql("platypus-db-yugabyte-1", "yugabyte", "yugabyte", hostname("platypus-db-yugabyte-1"));
    },
    noPrompt: true,
  },
  timescale: {
    port: 55433,
    password: "platypus",
    toml: `driver = "postgres"\nhost = "localhost"\nport = 55433\ndatabase = "qa_dbs_ts"\nuser = "platypus"\nssl_mode = "disable"\npassword_command = "echo platypus"`,
    shell: psql("platypus-db-timescale-1", "platypus", "qa_dbs_ts"),
    noPrompt: true,
  },
  timescaleAdmin: {
    port: 55433,
    shell: psql("platypus-db-timescale-1", "platypus", "platypus"),
  },
  libsql: {
    port: 58080,
    password: LIBSQL_TOKEN,
    toml: `driver = "libsql"\nhost = "localhost"\nport = 58080\ndatabase = ""\nuser = ""\nssl_mode = "disable"\npassword_command = "echo ${LIBSQL_TOKEN}"`,
    noPrompt: true,
  },
  trino: {
    port: 58085,
    password: "",
    toml: `driver = "trino"\nhost = "127.0.0.1"\nport = 58085\ndatabase = "memory"\nuser = "platypus"\nssl_mode = "disable"`,
    options: `schema = "qa_dbs"`,
    savePassword: true,
    shell: ["exec", "-i", "platypus-db-trino-1", "trino", "--catalog", "memory", "--output-format", "TSV"],
    noPrompt: true,
  },
});

/** Creates the TimescaleDB database qa_dbs_ts (the shared `platypus` database has no
 *  timescaledb extension) and runs `sql` in it. */
export function timescaleDb(sql) {
  dropTimescaleDb();
  sh("timescaleAdmin", "CREATE DATABASE qa_dbs_ts;");
  sh("timescale", `CREATE EXTENSION IF NOT EXISTS timescaledb;\n${sql}`);
}

export const dropTimescaleDb = () => sh("timescaleAdmin", "DROP DATABASE IF EXISTS qa_dbs_ts WITH (FORCE);");

/** Runs SQL on the libSQL server over its HTTP API; returns the rows of the last statement. */
export function libsql(...statements) {
  const r = spawnSync(
    "curl",
    ["-s", "-H", `Authorization: Bearer ${LIBSQL_TOKEN}`, "--data-binary", "@-", "http://127.0.0.1:58080/"],
    { input: JSON.stringify({ statements }), encoding: "utf8" },
  );
  const out = JSON.parse(r.stdout);
  const failed = out.find((o) => o.error);
  if (failed) throw new Error(`libsql: ${failed.error.message}`);
  return out.at(-1).results.rows;
}

/** A skip() for the services listening on `ports`. */
export const needsPorts =
  (...ports) =>
  async () => {
    for (const port of ports) {
      const up = await new Promise((resolveUp) => {
        const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
        const done = (ok) => {
          socket.destroy();
          resolveUp(ok);
        };
        socket.on("connect", () => done(true));
        socket.on("error", () => done(false));
        socket.on("timeout", () => done(false));
      });
      if (!up) return `nothing on port ${port} (docker compose -f docker/compose.yml --profile <name> up -d)`;
    }
    return null;
  };

/** Copies the Trino ADBC driver the conformance tests downloaded into the profile, so the
 *  scenario does not download it; false when there is none. */
export function seedTrinoDriver(dir) {
  const from = resolve(here, "../../../../../../target/conformance-drivers/drivers/adbc/trino");
  const alt = resolve(here, "../../../target/conformance-drivers/drivers/adbc/trino");
  const source = [from, alt].find((p) => existsSync(p));
  if (!source) return false;
  cpSync(source, join(dir, "drivers", "adbc", "trino"), { recursive: true });
  return true;
}

/** Writes connections.toml with one connection per `[name, key]` (a key of DBS, or
 *  `{ sqlite: path }` / `{ duckdb: path }`), each with its password from a command. */
export function conns(dir, list, extra = "") {
  let text = "version = 7\n";
  list.forEach(([name, db], i) => {
    const id = `00000000-0000-4000-8000-0000000d${String(i).padStart(4, "0")}`;
    const d = typeof db === "string" ? DBS[db] : null;
    const body = d
      ? d.toml + (d.toml.includes("password_command") || !d.password ? "" : `\npassword_command = "echo ${d.password}"`)
      : db.sqlite
        ? `driver = "sqlite"\ndatabase = '${db.sqlite}'`
        : `driver = "duckdb"\ndatabase = '${db.duckdb}'`;
    text += `\n[[connections]]\nid = "${id}"\nname = "${name}"\n${body}\nsave_password = ${d?.savePassword ? "true" : "false"}\n`;
    if (d?.options) text += `\n[connections.options]\n${d.options}\n`;
  });
  writeFileSync(join(dir, "connections.toml"), text + extra);
}

/** The tree row labelled `label` found after the rows of `chain` (labels), each found after
 *  the one before. */
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

/** Expands the tree along `path` (labels, each below the one before) until its last label shows. */
export async function openPath(page, path, timeout = 30_000) {
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

/** Expands the tree row `label` until `child` shows below it (a click while the tree is still
 *  loading may be lost, so the twisty is clicked again while the row stays collapsed). */
export async function openRow(page, label, child, timeout = 30_000) {
  const row = treeRow(label);
  const deadline = Date.now() + timeout;
  await page.waitFor(`${row} !== undefined`, timeout);
  while (Date.now() < deadline) {
    if (await page.eval(`${treeRow(child)} !== undefined`)) return;
    if (await page.eval(`${row}?.getAttribute('aria-expanded') !== 'true'`)) await page.eval(`${row}.querySelector('.twisty').click()`);
    await page.sleep(400);
  }
  throw new Error(`no ${child} under ${label}`);
}
