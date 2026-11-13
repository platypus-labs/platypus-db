// Regression guard (passes): the Oracle connection form. Test connection succeeds with host,
// port and service name (in any case), reports ORA-01017 for a wrong password, ORA-12514 for
// an unknown service and ORA-12541 for a closed port; with no host the service field takes an
// Easy Connect string or a tnsnames.ora alias (TNS_ADMIN); a pasted JDBC thin URL fills the
// form for Oracle.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { assert, needsOracle, oracleSetup } from "../oracle-helpers.mjs";
import { button, dialog, fillForm, formField, newConnection, openPicker, paste, picker, testConnection, urlField, newFromUrl } from "../conn-helpers.mjs";

export const skip = needsOracle();

export function setup({ dir }) {
  oracleSetup(dir);
  writeFileSync(join(dir, "tnsnames.ora"), "QAORA =\n  (DESCRIPTION =\n    (ADDRESS = (PROTOCOL = TCP)(HOST = 127.0.0.1)(PORT = 51521))\n    (CONNECT_DATA = (SERVICE_NAME = FREEPDB1))\n  )\n");
}
export const env = ({ dir }) => ({ TNS_ADMIN: dir });

export default async function (page) {
  await newConnection(page, "Oracle");
  const check = async (fields, pattern) => {
    await fillForm(page, fields);
    const notice = await testConnection(page, 30_000);
    assert.match(notice, pattern, `${JSON.stringify(fields)}: ${notice}`);
  };
  await check({ Host: "127.0.0.1", Port: "51521", "Service name": "FREEPDB1", User: "platypus", Password: "platypus" }, /^Connected: Oracle 23/);
  await check({ "Service name": "freepdb1" }, /^Connected/);
  await check({ Password: "wrong" }, /ORA-01017/);
  await check({ "Service name": "NO_SUCH_SERVICE", Password: "platypus" }, /ORA-12514/);
  await check({ "Service name": "FREEPDB1", Port: "51599" }, /ORA-12541/);
  await check({ Host: "", Port: "", "Service name": "127.0.0.1:51521/FREEPDB1" }, /^Connected/);
  await check({ "Service name": "QAORA" }, /^Connected/);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null`);

  await newFromUrl(page, "jdbc:oracle:thin:@//127.0.0.1:51521/FREEPDB1");
  assert.deepEqual(
    await page.eval(`[${formField("Host")}.value, ${formField("Port")}.value, ${formField("Service name")}.value]`),
    ["127.0.0.1", "51521", "FREEPDB1"],
  );
  await fillForm(page, { User: "platypus", Password: "platypus" });
  assert.match(await testConnection(page, 30_000), /^Connected/);
}
