// Group F: connections whose fields read environment variables, `${env:NAME}`, from the
// app's environment or from the connection's `.env` file; a variable that is not set says
// which; and the pooler mode of a PostgreSQL connection, suggested for Supabase's pooler on
// port 6543 and saved in its options. No server needed: SQLite files and a connection that
// is only edited.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expand, treeMenu, treeRow } from "../qa-grid.mjs";

const SUPABASE = "QA F Supabase";

export function setup({ dir }) {
  for (const [file, table] of [["process.db", "from_process"], ["dotenv.db", "from_file"]]) {
    const db = new DatabaseSync(join(dir, file));
    db.exec(`CREATE TABLE ${table} (id INTEGER PRIMARY KEY)`);
    db.close();
  }
  writeFileSync(join(dir, "project.env"), `# the project's variables\nexport QA_F_FILE_DB='${join(dir, "dotenv.db")}'\n`);
  const sqlite = (n, name, database, extra = "") =>
    `[[connections]]\nid = "00000000-0000-4000-8000-0000000f000${n}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${database}'\nsave_password = false\n${extra}\n`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1\n\n${sqlite(1, "QA F env", "${env:QA_F_PROCESS_DB}")}${sqlite(2, "QA F dotenv", "${env:QA_F_FILE_DB}", `[connections.options]\nenv_file = '${join(dir, "project.env")}'\n`)}${sqlite(3, "QA F missing", "${env:QA_F_MISSING}")}` +
      `[[connections]]\nid = "00000000-0000-4000-8000-0000000f0004"\nname = "${SUPABASE}"\ndriver = "postgres"\nhost = "aws-0-eu-central-1.pooler.supabase.com"\nport = 6543\ndatabase = "postgres"\nuser = "postgres.abcdefgh"\nssl_mode = "require"\nsave_password = false\n`,
  );
}

export function env({ dir }) {
  return { QA_F_PROCESS_DB: join(dir, "process.db") };
}

const dialog = `document.querySelector('.modal.connection-dialog')`;

export default async function (page, { dir }) {
  // From the app's environment, and from the connection's .env file.
  await expand(page, "QA F env", null, "from_process");
  await expand(page, "QA F dotenv", null, "from_file");

  // A variable that is not set: the error names it, and nothing opens.
  await page.waitFor(`${treeRow("QA F missing")} !== undefined`);
  await page.eval(`${treeRow("QA F missing")}.querySelector(".twisty").click()`);
  await page.waitFor(`[...document.querySelectorAll('.notice, .toast')].some(n => n.textContent.includes('QA_F_MISSING is not set'))`, 15_000);

  // Supabase's pooler on 6543 suggests transaction mode; taking it saves the option.
  await treeMenu(page, SUPABASE, "Edit…");
  await page.waitFor(`${dialog}?.querySelector('[data-field=pooler]') != null`);
  assert.equal(await page.eval(`${dialog}.querySelector('[data-field=pooler]').value`), "");
  assert.match(await page.eval(`${dialog}.querySelector('.pooler-hint')?.textContent ?? ''`), /Supabase's pooler in transaction mode/);
  await page.eval(`${dialog}.querySelector('.pooler-hint button').click()`);
  await page.waitFor(`${dialog}.querySelector('[data-field=pooler]').value === 'transaction' && ${dialog}.querySelector('.pooler-hint') === null`);
  // The .env field shows its explanation, with the reference as code.
  assert.match(await page.eval(`${dialog}.querySelector('[data-field=env_file]').closest('.field').textContent`), /\$\{env:NAME\}/);
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Save').click()`);
  await page.waitFor(`${dialog} === null`, 10_000);
  const saved = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.match(saved, /pooler = "transaction"/);
  // The other connections keep their references, in the quotes the app writes.
  assert.match(saved, /database = (["'])\$\{env:QA_F_PROCESS_DB\}\1/);
}
