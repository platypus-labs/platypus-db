// Regression guard: Test connection from a new connection's form, for every database
// container that is up: the right credentials connect (with the server's product), a wrong
// password is refused with the server's message, and that message never repeats the
// password typed. Databases whose container is down are left out.
import assert from "node:assert/strict";
import { connect as tcp } from "node:net";
import { button, dialog, fill, fillForm, formField, newConnection, picker, seedSqlite, testConnection } from "../conn-helpers.mjs";

const TOKEN =
  "eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9.eyJhIjoicnciLCJpYXQiOjE3NjcyMjU2MDB9.gcA5ick0W16rnB4ROBvkn896i2bNi3Z80USgUN9a5tEtdq5kigej16LpaCi_sVzxdzVIGUQ1n9eUtayjb2GdAg";
const WRONG = "qa-conn-WRONG-pw-7731";

// [tile, port, fields, secret field, right secret, product]. The YugabyteDB container runs
// without password authentication: only its connection is checked.
const CASES = [
  ["PostgreSQL", 55432, { Database: "platypus", User: "platypus" }, "Password", "platypus", /^Connected: PostgreSQL 17/],
  ["MySQL", 53306, { User: "platypus" }, "Password", "platypus", /^Connected: MySQL 8/],
  ["MariaDB", 53307, { User: "platypus" }, "Password", "platypus", /^Connected: MariaDB 11/],
  ["ClickHouse", 58123, { User: "platypus" }, "Password", "platypus", /^Connected: ClickHouse/],
  ["SQL Server", 51433, { "Database (optional)": "platypus", User: "sa" }, "Password", "Platypus-2026", /^Connected: SQL Server 16/],
  ["MongoDB", 57017, { "Database (optional)": "platypus?authSource=admin", User: "platypus" }, "Password", "platypus", /^Connected: MongoDB 8/],
  ["TimescaleDB", 55433, { Database: "platypus", User: "platypus" }, "Password", "platypus", /^Connected: PostgreSQL/],
  ["YugabyteDB", 55434, {}, "Password", "yugabyte", /^Connected: YugabyteDB/],
  ["Turso / libSQL", 58080, {}, "Auth token", TOKEN, /^Connected: libSQL/],
];

const up = (port) =>
  new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
    const done = (ok) => {
      socket.destroy();
      resolve(ok);
    };
    socket.on("connect", () => done(true));
    socket.on("error", () => done(false));
    socket.on("timeout", () => done(false));
  });

export const skip = async () =>
  (await Promise.all(CASES.map((c) => up(c[1])))).some(Boolean) ? null : "no database container is running (docker compose -f docker/compose.yml up -d)";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  for (const [tile, port, fields, secretLabel, secret, product] of CASES) {
    if (!(await up(port))) continue;
    await newConnection(page, tile);
    await fillForm(page, { Host: "localhost", Port: port, ...fields });
    if (tile === "Turso / libSQL") {
      await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network')).click()`);
      await page.eval(fill(formField("SSL mode"), "disable"));
      await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('General')).click()`);
    }
    await page.eval(fill(formField(secretLabel), secret));
    assert.match(await testConnection(page), product, tile);
    if (tile === "YugabyteDB") {
      await page.eval(`${button(dialog, "Cancel")}.click()`);
      await page.waitFor(`${dialog} === null && ${picker} === null`);
      continue;
    }
    await page.eval(fill(formField(secretLabel), WRONG));
    const refused = await testConnection(page);
    assert.doesNotMatch(refused, /^Connected/, `${tile} accepted a wrong password`);
    assert.ok(!refused.includes(WRONG), `${tile} shows the password: ${refused}`);
    await page.eval(`${button(dialog, "Cancel")}.click()`);
    await page.waitFor(`${dialog} === null && ${picker} === null`);
  }
}
