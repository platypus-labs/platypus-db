// CONN-19 (low). Connection errors show the drivers' raw text: ClickHouse with SSL require
// against its HTTP port prints the whole request URL with its query string (session_id,
// query_id, output format settings…); MySQL/MariaDB repeat "Input/output error: Input/output
// error:"; SQL Server repeats "An error occured during the attempt of performing I/O" (sic).
// Steps: New connection to the ClickHouse container (HTTP port) with SSL mode require, Test;
// MySQL with verify-full against its self-signed certificate, Test.
// Expected: a short message (TLS handshake failed: the server does not speak TLS on this
// port; certificate not trusted). Actual: the raw messages above.
// Skipped when ClickHouse or MySQL is not running.
import assert from "node:assert/strict";
import { button, dialog, fill, fillForm, formField, needsPort, newConnection, picker, seedSqlite, testConnection } from "../conn-helpers.mjs";

const clickhouseUp = needsPort(58123, "ClickHouse");
const mysqlUp = needsPort(53306, "MySQL");
export const skip = async () => (await clickhouseUp()) ?? (await mysqlUp());

export function setup({ dir }) {
  seedSqlite(dir);
}

async function testWith(page, tile, fields, ssl) {
  await newConnection(page, tile);
  await fillForm(page, fields);
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network')).click()`);
  await page.eval(fill(formField("SSL mode"), ssl));
  const message = await testConnection(page);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null && ${picker} === null`);
  return message;
}

export default async function (page) {
  const clickhouse = await testWith(page, "ClickHouse", { Host: "localhost", Port: 58123, User: "platypus", Password: "platypus" }, "require");
  const mysql = await testWith(page, "MySQL", { Host: "localhost", Port: 53306, User: "platypus", Password: "platypus" }, "verify-full");
  assert.doesNotMatch(clickhouse, /session_id=|query_id=|default_format=/, clickhouse);
  assert.doesNotMatch(mysql, /Input\/output error: Input\/output error/, mysql);
}
