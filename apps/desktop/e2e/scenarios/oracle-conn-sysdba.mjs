// Bug ORA-12: there is no way to connect to Oracle as SYS (or any user with an
// administrative privilege): the form has no role (SYSDBA / SYSOPER / SYSBACKUP…), and the
// usual "sys as sysdba" user, which SQL*Plus and SQL Developer accept, is sent as the user
// name. "sys" alone gives ORA-28009 "connection as SYS should be as SYSDBA or SYSOPER",
// "sys as sysdba" ORA-01017. (crates/platypus-driver-oracle/src/session.rs `open`: the
// Connector never gets a privilege; DBeaver has a Role list.)
// Steps: New connection › Oracle, host 127.0.0.1, port 51521, service FREEPDB1, user
// "sys as sysdba", password of the container (ORACLE_PASSWORD); Test connection.
// Expected: Connected. Actual: ORA-01017.
import { assert, needsOracle, oracleSetup } from "../oracle-helpers.mjs";
import { fillForm, newConnection, testConnection } from "../conn-helpers.mjs";

export const skip = needsOracle();
export const setup = ({ dir }) => oracleSetup(dir);

export default async function (page) {
  await newConnection(page, "Oracle");
  await fillForm(page, { Host: "127.0.0.1", Port: "51521", "Service name": "FREEPDB1", User: "sys as sysdba", Password: "platypus" });
  const notice = await testConnection(page, 30_000);
  assert.match(notice, /^Connected/, notice);
}
