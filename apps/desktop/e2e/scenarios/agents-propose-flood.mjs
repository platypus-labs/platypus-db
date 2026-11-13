// QA5 mcp (medium, agent flood): `propose_sql` has no limit on how many proposals wait for the app.
//
// Steps: with the app closed, an agent (or a prompt-injected one) calls propose_sql 300 times
// ("Nothing ran: the user reviews and runs it", each up to 1 MB); the user then opens PlatypusDB.
// Expected: the queue is bounded (a call is refused once N proposals wait: "the user has not looked at
// the last N yet"), or the app opens a few tabs and lists the rest, so the user can still use the
// window and the agent cannot fill the disk or the tab bar.
// Actual: every call is accepted and the app opens 300 editor tabs at start (a thousand calls, a
// thousand tabs, each with a full SQL text restored by the workspace on the next start).
// Code: crates/platypus-mcp (the proposals file the server appends to, `propose_sql`) and the app's
// consumer of it in apps/desktop/src (opens one tab per proposal).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";

export const skip = noMcp;
export const ready = `document.querySelector('.shell') !== null`;

const state = { refused: 0 };

export async function setup({ dir }) {
  new DatabaseSync(join(dir, "a.db")).close();
  writeFileSync(join(dir, "connections.toml"), `version = 4\n[[connections]]\nid = "00000000-0000-4000-8000-000000000e55"\nname = "Scratch"\ndriver = "sqlite"\ndatabase = '${join(dir, "a.db")}'\n`);
  const mcp = await startMcp(dir, "flood");
  try {
    for (let i = 0; i < 300; i++) {
      const r = await mcp.call("propose_sql", { connection: "Scratch", sql: `SELECT ${i}`, title: `p${i}` });
      if (r.error) state.refused++;
    }
  } finally {
    mcp.close();
  }
}

export default async function (page) {
  await page.sleep(5000);
  const tabs = await page.eval(`document.querySelectorAll('.tab').length`);
  assert.ok(state.refused > 0 || tabs <= 50, `300 proposals accepted, ${tabs} tabs opened at start`);
}
