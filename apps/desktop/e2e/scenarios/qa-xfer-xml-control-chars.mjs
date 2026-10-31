// XFER-16 (medium). The XML export is not well-formed when a value holds a control character.
// Steps: run `SELECT 'a' || char(1) || 'b' AS t` (any text with U+0001, e.g. pasted from a
// terminal); Export… › XML.
// Expected: a well-formed XML file (the character escaped away, replaced, or the row in a
// CDATA/base64 form) that XML parsers read.
// Actual: the byte 0x01 is written as is inside <field>; XML 1.0 forbids it and parsers stop
// ("not well-formed (invalid token)"). The CSV, JSON and Excel exports handle it.
// Suspected: crates/platypus-transfer/src/text.rs:47-52 (xml_escape() only escapes & < > ").
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { exportResult, openEditor, runScript, sqliteOnly } from "../qa-xfer.mjs";

export const setup = ({ dir }) => sqliteOnly(dir);

export default async function (page, { dir }) {
  await openEditor(page, "QA SQLite");
  assert.equal(await runScript(page, "SELECT 'a' || char(1) || 'b' AS t"), "1 row");
  assert.match(await exportResult(page, join(dir, "out.xml"), { format: "xml", source: "loaded" }), /^Exported 1 row\b/);
  const xml = readFileSync(join(dir, "out.xml"), "utf8");
  assert.doesNotMatch(xml, /[\x00-\x08\x0B\x0C\x0E-\x1F]/, JSON.stringify(xml));
}
