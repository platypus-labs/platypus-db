// A connection through a JDBC driver the app downloads from Maven Central: the "JDBC driver"
// tile, the driver and its version (H2's jar is downloaded when the version is chosen, with
// its SHA-256 checked), the URL the host, port and database make until it is written by hand,
// the license shown before a driver that is not open source is downloaded (SAP HANA, and IBM's
// in Db2's own block, which shares the component), and an in-memory H2 opened and queried.
// The errors: no version chosen, a URL no driver takes, a Java that is not there.
// Needs the network and a Java 11 or later (PLATYPUS_TEST_JAVA, JAVA_HOME or the PATH).
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import {
  button,
  dialog,
  editConnection,
  fill,
  formField,
  gridRows,
  newConnection,
  openQueryTab,
  run,
  saveForm,
  seedSqlite,
  testConnection,
} from "../conn-helpers.mjs";

const NAME = "H2 in memory";
const URL_MEM = "jdbc:h2:mem:e2e;DB_CLOSE_DELAY=-1";
const field = (name) => `document.querySelector('.connection-dialog [data-field="${name}"]')`;
const licenseNotice = `document.querySelector('.connection-dialog [data-notice="jdbc_license"]')`;

/** The Java home the test may use: the variable of the other JDBC tests, or the system's. */
const javaHome = process.env.PLATYPUS_TEST_JAVA ?? process.env.JAVA_HOME;

export function env() {
  return javaHome ? { JAVA_HOME: javaHome } : {};
}

export async function skip() {
  const java = javaHome
    ? existsSync(join(javaHome, "bin", "java.exe")) || existsSync(javaHome)
    : spawnSync("java", ["-version"]).status === 0;
  if (!java) return "no Java to run the JDBC bridge (set PLATYPUS_TEST_JAVA)";
  try {
    await fetch("https://repo1.maven.org/maven2/", { method: "HEAD", signal: AbortSignal.timeout(5000) });
    return null;
  } catch {
    return "no network to download the JDBC driver from Maven Central";
  }
}

export function setup({ dir }) {
  seedSqlite(dir);
}

/** Sets the driver's `select` (data-field) and tells React. */
const choose = (page, name, value) => page.eval(fill(field(name), value));

/** Waits for the select `name` to hold `value`. */
const waitSelect = (page, name, value, ms = 20_000) => page.waitFor(`${field(name)}?.value === ${JSON.stringify(value)}`, ms);

export default async function (page) {
  await newConnection(page, "JDBC driver");

  // The tile starts on H2, with its port, and no version yet.
  assert.equal(await page.eval(`${field("jdbc_package")}.value`), "h2");
  assert.equal(await page.eval(`${field("jdbc_version")}.value`), "");
  assert.equal(await page.eval(`${field("jdbc_port")}.value`), "9092");
  assert.equal(await page.eval(`${field("jdbc_url")}.value`), "");
  const drivers = await page.eval(`[...${field("jdbc_package")}.options].map(o => o.textContent)`);
  for (const wanted of ["H2", "HSQLDB", "Firebird (Jaybird)", "Apache Derby (network client)", "SAP HANA", "Exasol", "Vertica", "JDBC driver files…"]) {
    assert.ok(drivers.includes(wanted), `${wanted} in ${drivers}`);
  }
  assert.ok(!drivers.some((d) => d.startsWith("IBM")), "IBM's driver is Db2's own choice");
  // Every version of H2 is offered as a download, with its size.
  const versions = await page.eval(`[...${field("jdbc_version")}.options].map(o => o.textContent)`);
  assert.match(versions[1], /^2\.\d+\.\d+ \(to download, \d(\.\d)? MB\)$/, versions.join("|"));

  // The URL follows the host, port and database.
  await page.eval(fill(field("jdbc_host"), "db.example.com"));
  assert.equal(await page.eval(`${field("jdbc_url")}.value`), "jdbc:h2:tcp://db.example.com:9092/");
  await page.eval(fill(field("jdbc_database"), "sales"));
  await page.eval(fill(field("jdbc_port"), "9999"));
  assert.equal(await page.eval(`${field("jdbc_url")}.value`), "jdbc:h2:tcp://db.example.com:9999/sales");
  // Another driver: its template, and the port stays when it was changed by hand.
  await choose(page, "jdbc_package", "firebird");
  assert.equal(await page.eval(`${field("jdbc_url")}.value`), "jdbc:firebirdsql://db.example.com:9999/sales");
  await page.eval(fill(field("jdbc_port"), "3051"));
  assert.equal(await page.eval(`${field("jdbc_url")}.value`), "jdbc:firebirdsql://db.example.com:3051/sales");
  await choose(page, "jdbc_package", "hana");
  assert.equal(await page.eval(`${field("jdbc_url")}.value`), "jdbc:sap://db.example.com:3051/?databaseName=sales");
  await page.eval(fill(field("jdbc_database"), ""));
  assert.equal(await page.eval(`${field("jdbc_url")}.value`), "jdbc:sap://db.example.com:3051/");
  await page.eval(fill(field("jdbc_database"), "HXE"));

  // SAP HANA is not open source: choosing a version shows its license and downloads nothing
  // until the button is pressed.
  assert.equal(await page.eval(`${licenseNotice} === null`), true);
  await page.eval(fill(field("jdbc_version"), await page.eval(`${field("jdbc_version")}.options[1].value`)));
  await page.waitFor(`${licenseNotice} !== null`);
  const license = await page.eval(`${licenseNotice}.textContent`);
  assert.match(license, /SAP HANA is not open source/);
  assert.match(license, /License: SAP Developer License Agreement/);
  assert.match(license, /Accept and download 2\.30\.12/);
  // Nothing was chosen: the connection still has no version, and testing it says so.
  const hana = await testConnection(page);
  assert.match(hana, /choose a version of the hana JDBC driver/, hana);
  // Another driver takes the license notice away.
  await choose(page, "jdbc_package", "h2");
  await page.waitFor(`${licenseNotice} === null`);
  assert.equal(await page.eval(`${field("jdbc_version")}.value`), "");

  // Error: no version chosen.
  await page.eval(fill(field("jdbc_url"), URL_MEM));
  const none = await testConnection(page);
  assert.match(none, /choose a version of the h2 JDBC driver/, none);

  // A version is downloaded when it is chosen, checked with its SHA-256.
  await choose(page, "jdbc_version", "2.3.232");
  await waitSelect(page, "jdbc_version", "2.3.232", 120_000);
  assert.equal(await page.eval(`${field("jdbc_package")}.disabled`), false);
  const installed = await page.eval(`[...${field("jdbc_version")}.options].map(o => o.textContent)`);
  assert.ok(installed.includes("2.3.232"), installed.join("|"));
  // A URL written by hand is kept when the host changes.
  await page.eval(fill(field("jdbc_host"), "other.example.com"));
  assert.equal(await page.eval(`${field("jdbc_url")}.value`), URL_MEM);

  // Error: a URL no driver takes.
  await page.eval(fill(field("jdbc_url"), "jdbc:nothing://x/y"));
  const bad = await testConnection(page, 60_000);
  assert.match(bad, /No suitable driver|nothing/i, bad);

  // Error: a Java that is not there.
  await page.eval(fill(field("jdbc_url"), URL_MEM));
  await page.eval(fill(field("jdbc_java"), join(process.env.TEMP ?? "C:\\", "no-such-java")));
  const noJava = await testConnection(page);
  assert.match(noJava, /no-such-java/, noJava);
  await page.eval(fill(field("jdbc_java"), ""));

  // The database opens.
  const ok = await testConnection(page, 60_000);
  assert.match(ok, /H2/, ok);
  await page.eval(fill(formField("Name"), NAME));
  await saveForm(page);

  // It is saved with its driver and version; the form shows them again.
  await editConnection(page, NAME);
  assert.equal(await page.eval(`${formField("Driver")}.value`), "jdbc");
  assert.equal(await page.eval(`${field("jdbc_package")}.value`), "h2");
  assert.equal(await page.eval(`${field("jdbc_version")}.value`), "2.3.232");
  assert.equal(await page.eval(`${field("jdbc_url")}.value`), URL_MEM);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null`);

  // And it is queried.
  await openQueryTab(page, NAME);
  await run(page, "select 41 + 1 as answer", 60_000);
  assert.deepEqual(await page.eval(gridRows), [["42"]]);

  // Db2 shares the component: IBM's driver is not open source either, so the license shows
  // before anything is downloaded, and the ODBC choice takes it away.
  await newConnection(page, "IBM Db2");
  await page.waitFor(`${field("jdbc_driver")}?.options.length > 2`);
  await choose(page, "jdbc_driver", "v:12.1.5.0");
  await page.waitFor(`${licenseNotice} !== null`);
  assert.match(await page.eval(`${licenseNotice}.textContent`), /IBM Data Server Driver for JDBC and SQLJ is not open source/);
  assert.equal(await page.eval(`${field("jdbc_java")} === null`), true);
  await choose(page, "jdbc_driver", "odbc");
  await page.waitFor(`${licenseNotice} === null`);
  assert.equal(await page.eval(`${field("jdbc_driver")}.value`), "odbc");
}
