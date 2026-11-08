// Manual commit on MongoDB. On the single-node replica set (`mongo-rs`): the toggle switches
// the tab to manual commit, an insert waits for Commit, Rollback leaves nothing, Commit
// keeps it, and a statement MongoDB does not run in a transaction (`drop()`) is refused with
// its reason while the transaction stays open. On the standalone server the toggle is
// disabled and says why, and there is no Commit or Rollback. A replica set marked as production
// opens in manual commit, with Commit and Rollback shown.
// Skipped when the standalone server or the replica set is not running.
import assert from "node:assert/strict";
import { resultError, visibleTab } from "../qa-sql.mjs";
import {
  connection,
  count,
  mongosh,
  needs,
  openEditor,
  runStatement,
  saveConnections,
  toolbarButton,
} from "../mongo-grid-helpers.mjs";

const DEV = "E2E Mongo rs dev";
const STANDALONE = "E2E Mongo standalone";
const PROD = "E2E Mongo rs prod";
const C = "e2e_tx";

export const skip = needs("standalone", "replica");

export function setup({ dir }) {
  saveConnections(
    dir,
    connection(10, DEV, "replica"),
    connection(11, STANDALONE, "standalone"),
    connection(12, PROD, "replica", 'connection_type = "prod"\n'),
  );
  mongosh("replica", `db.${C}.drop(); db.createCollection("${C}")`);
}

const transactionOpen = `${visibleTab}.querySelector('.tx-open') !== null`;
/** Expressions for whether the toolbar has the button. */
const present = (label) => `${toolbarButton(label)} !== null`;
const absent = (label) => `${toolbarButton(label)} === null`;
const rows = () => count("replica", C);

export default async function (page) {
  await developmentReplicaSet(page);
  await standalone(page);
  await production(page);
}

async function developmentReplicaSet(page) {
  await openEditor(page, DEV);
  await runStatement(page, `db.${C}.find()`);
  // Autocommit: no Commit or Rollback yet, and the toggle is available.
  assert.equal(await page.eval(present("Auto-commit")), true);
  assert.equal(await page.eval(absent("Commit")), true);
  assert.equal(await page.eval(`${toolbarButton("Auto-commit")}.disabled`), false);

  await page.eval(`${toolbarButton("Auto-commit")}.click()`);
  await page.waitFor(present("Manual commit"), 10_000);
  await page.waitFor(`${present("Commit")} && ${present("Rollback")}`, 10_000);
  // Nothing open: both are disabled.
  assert.equal(await page.eval(`${toolbarButton("Commit")}.disabled`), true);

  // An insert opens the transaction; another session does not see it; Rollback leaves nothing.
  await runStatement(page, `db.${C}.insertOne({ _id: 1, v: "rolled back" })`);
  await page.waitFor(transactionOpen, 10_000);
  assert.equal(rows(), 0, "an uncommitted insert is visible to others");
  assert.equal(await runStatement(page, `db.${C}.find()`), "1 row", "the transaction reads its own write");
  await page.eval(`${toolbarButton("Rollback")}.click()`);
  await page.waitFor(`!(${transactionOpen})`, 10_000);
  assert.equal(rows(), 0);

  // The same insert, kept with Commit.
  await runStatement(page, `db.${C}.insertOne({ _id: 2, v: "kept" })`);
  await page.waitFor(transactionOpen, 10_000);
  assert.equal(rows(), 0);
  await page.eval(`${toolbarButton("Commit")}.click()`);
  await page.waitFor(`!(${transactionOpen})`, 10_000);
  assert.equal(rows(), 1);
  assert.equal(count("replica", C, '{ _id: 2, v: "kept" }'), 1);

  // The switch to autocommit is refused while a transaction is open, and allowed again once it ends.
  await runStatement(page, `db.${C}.insertOne({ _id: 3 })`);
  await page.waitFor(transactionOpen, 10_000);
  assert.equal(await page.eval(`${toolbarButton("Manual commit")}.disabled`), true);

  // DDL does not run in a transaction: refused, with the reason, and the transaction stays.
  await runStatement(page, `db.${C}.drop()`);
  const refusal = await page.eval(resultError);
  assert.match(refusal, /dropping a collection in manual commit/);
  assert.match(refusal, /does not run it in a transaction/);
  assert.match(refusal, /open transaction is unchanged/);
  assert.equal(await page.eval(transactionOpen), true);
  assert.equal(count("replica", C), 1, "the collection was dropped");
  // A rename and an index drop are refused the same way; an index creation runs inside.
  for (const [statement, what] of [
    [`db.${C}.renameCollection("${C}_renamed")`, /renaming a collection/],
    [`db.${C}.dropIndex("v_1")`, /dropping an index/],
  ]) {
    await runStatement(page, statement);
    assert.match(await page.eval(resultError), what, statement);
  }
  assert.equal(await page.eval(transactionOpen), true);

  await page.eval(`${toolbarButton("Rollback")}.click()`);
  await page.waitFor(`!(${transactionOpen})`, 10_000);
  assert.equal(count("replica", C, "{ _id: 3 }"), 0);
  assert.equal(rows(), 1);

  // Back to autocommit: statements commit on their own, and DDL runs.
  await page.eval(`${toolbarButton("Manual commit")}.click()`);
  await page.waitFor(`${present("Auto-commit")} && ${absent("Commit")}`, 10_000);
  await runStatement(page, `db.${C}.insertOne({ _id: 4 })`);
  assert.equal(await page.eval(transactionOpen), false);
  assert.equal(rows(), 2);
  await runStatement(page, `db.${C}.drop()`);
  assert.equal(mongosh("replica", `print(db.getCollectionNames().includes("${C}"))`), "false");
}

async function standalone(page) {
  await openEditor(page, STANDALONE);
  await runStatement(page, "db.getCollectionNames()");
  // The toggle is there, disabled, with the reason; nothing to commit.
  await page.waitFor(present("Auto-commit"), 10_000);
  await page.waitFor(`${toolbarButton("Auto-commit")}.disabled === true`, 10_000);
  const title = await page.eval(`${toolbarButton("Auto-commit")}.title`);
  assert.match(title, /replica set or a sharded cluster/);
  assert.match(title, /standalone/);
  assert.equal(await page.eval(absent("Commit")), true);
  assert.equal(await page.eval(absent("Rollback")), true);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.tx-open') === null`), true);
}

async function production(page) {
  await openEditor(page, PROD);
  await runStatement(page, "db.getCollectionNames()");
  // A production connection opens in manual commit: Commit and Rollback are there from the start.
  await page.waitFor(present("Manual commit"), 15_000);
  await page.waitFor(`${present("Commit")} && ${present("Rollback")}`, 10_000);
  assert.equal(await page.eval(`${toolbarButton("Commit")}.disabled`), true);
  // A read opens a transaction of its own that nothing needs to commit; end it.
  if (await page.eval(transactionOpen)) {
    await page.eval(`${toolbarButton("Rollback")}.click()`);
    await page.waitFor(`!(${transactionOpen})`, 10_000);
  }
}
