// Unit tests of the images and file names of cells (src/grid/fileType.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { defaultFileName, imageType } from "../src/grid/fileType.ts";

const bytes = (...parts) => Uint8Array.from(parts.flatMap((p) => (typeof p === "string" ? [...p].map((c) => c.charCodeAt(0)) : Array.from(p))));

test("images a browser draws are told apart", () => {
  assert.equal(imageType(bytes([0x89], "PNG\r\n", [0x1a, 0x0a])), "image/png");
  assert.equal(imageType(bytes("RIFF", [0, 0, 0, 0], "WEBPVP8 ")), "image/webp");
  assert.equal(imageType(bytes("RIFF", [0, 0, 0, 0], "WAVEfmt ")), null);
  assert.equal(imageType(bytes("%PDF-1.4")), null);
});

test("the default file name is the column and the type's extension", () => {
  assert.equal(defaultFileName("scan", { ext: "pdf" }, true), "scan.pdf");
  assert.equal(defaultFileName("blob", null, true), "blob.bin");
  assert.equal(defaultFileName("notes", null, false), "notes.txt");
  assert.equal(defaultFileName('a/b:c', null, true), "a_b_c.bin");
  assert.equal(defaultFileName("", null, true), "value.bin");
});

test("a selection of more than 20,000 rows is not read for files", async () => {
  const { FILE_LOOKUP_ROWS, lookupFiles } = await import("../src/grid/fileType.ts");
  assert.equal(FILE_LOOKUP_ROWS, 20_000);
  assert.equal(lookupFiles(20_000), true);
  assert.equal(lookupFiles(20_001), false);
});
