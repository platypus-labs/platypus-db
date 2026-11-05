// Unit tests of telling a file from a cell's bytes (src/grid/fileType.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { IMAGE_TYPES, defaultFileName, fileType, imageType } from "../src/grid/fileType.ts";

const bytes = (...parts) => Uint8Array.from(parts.flatMap((p) => (typeof p === "string" ? [...p].map((c) => c.charCodeAt(0)) : Array.from(p))));
const name = (b) => fileType(b)?.name ?? null;

function zipEntry(entry, data) {
  const out = bytes("PK\x03\x04", new Array(14).fill(0));
  const head = new Uint8Array(12);
  const view = new DataView(head.buffer);
  view.setUint32(0, data.length, true);
  view.setUint32(4, data.length, true);
  view.setUint16(8, entry.length, true);
  return bytes(out, head, entry, data);
}
const zip = (...entries) => bytes(...entries.map(([n, d]) => zipEntry(n, d)));

test("tells the known types by their magic numbers", () => {
  assert.equal(name(bytes("%PDF-1.7\n", [1, 2, 3])), "PDF");
  assert.equal(name(bytes([0x89], "PNG\r\n", [0x1a, 0x0a], [0, 0])), "PNG");
  assert.equal(name(bytes([0xff, 0xd8, 0xff, 0xe0], [0, 16])), "JPEG");
  assert.equal(name(bytes("GIF89a", [1, 0])), "GIF");
  assert.equal(name(bytes("RIFF", [0, 0, 0, 0], "WEBPVP8 ")), "WebP");
  assert.equal(name(bytes("RIFF", [0, 0, 0, 0], "WAVEfmt ")), "WAV");
  assert.equal(name(bytes("II*", [0, 8, 0, 0, 0])), "TIFF");
  assert.equal(name(bytes([0x1f, 0x8b, 8, 0, 0, 0])), "GZIP");
  assert.equal(name(bytes("7z", [0xbc, 0xaf, 0x27, 0x1c, 0, 4])), "7Z");
  assert.equal(name(bytes("Rar!", [0x1a, 7, 1, 0])), "RAR");
  assert.equal(name(bytes("ID3", [3, 0, 0, 0, 0, 0, 0x21])), "MP3");
  assert.equal(name(bytes("OggS", [0, 2, 0, 0])), "OGG");
  assert.equal(name(bytes([0, 0, 0, 0x18], "ftypisom", [0, 0, 2, 0])), "MP4");
  assert.equal(name(bytes([0, 0, 0, 0x14], "ftypqt  ", [0, 0, 0, 0])), "MOV");
  assert.equal(name(bytes([0x1a, 0x45, 0xdf, 0xa3, 0x93, 0x42, 0x82, 0x88], "matroska")), "MKV");
  assert.equal(name(bytes([0x1a, 0x45, 0xdf, 0xa3, 0x93, 0x42, 0x82, 0x84], "webm")), "WebM");
  assert.equal(name(bytes("SQLite format 3\0", [0x10, 0])), "SQLite");
  assert.equal(name(bytes('<?xml version="1.0"?><svg xmlns="x"/>')), "SVG");
  assert.equal(name(bytes('  <svg width="1"></svg>')), "SVG");
  assert.equal(name(bytes("BM", [0x36, 0, 0, 0, 0, 0, 0, 0, 0x36, 0, 0, 0, 0x28, 0, 0, 0], new Array(8).fill(0))), "BMP");
  assert.equal(name(bytes(new Array(257).fill(0), "ustar\0")), "TAR");
});

test("an extension and a MIME type come with the name", () => {
  assert.deepEqual(fileType(bytes("%PDF-1.4")), { name: "PDF", ext: "pdf", mime: "application/pdf" });
  assert.equal(fileType(bytes([0xff, 0xd8, 0xff]))?.ext, "jpg");
});

test("Office and OpenDocument files are told from plain ZIPs by their entries", () => {
  assert.equal(name(zip(["[Content_Types].xml", bytes("<x/>")], ["word/document.xml", bytes("<d/>")])), "DOCX");
  assert.equal(name(zip(["[Content_Types].xml", bytes("<x/>")], ["xl/workbook.xml", bytes("<d/>")])), "XLSX");
  assert.equal(name(zip(["ppt/presentation.xml", bytes("<d/>")])), "PPTX");
  assert.equal(name(zip(["mimetype", bytes("application/vnd.oasis.opendocument.text")], ["content.xml", bytes("<d/>")])), "ODT");
  assert.equal(name(zip(["mimetype", bytes("application/vnd.oasis.opendocument.spreadsheet")])), "ODS");
  assert.equal(name(zip(["a.txt", bytes("hello")])), "ZIP");
});

test("short, plain and random bytes are no file", () => {
  assert.equal(fileType(new Uint8Array()), null);
  assert.equal(fileType(bytes("%PD")), null);
  assert.equal(fileType(bytes("BM")), null);
  assert.equal(fileType(bytes("hello world, this is a text")), null);
  assert.equal(fileType(bytes("<html><body>no image</body></html>")), null);
  assert.equal(fileType(bytes([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])), null);
  let x = 0x12345678;
  for (let len = 1; len < 300; len++) {
    const random = new Uint8Array(len);
    for (let i = 0; i < len; i++) {
      x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
      random[i] = x >>> 24;
    }
    assert.equal(fileType(random), null, `${len} bytes: ${random.join(",")}`);
  }
});

test("images a browser draws are told apart", () => {
  assert.equal(imageType(bytes([0x89], "PNG\r\n", [0x1a, 0x0a])), "image/png");
  assert.equal(imageType(bytes("RIFF", [0, 0, 0, 0], "WEBPVP8 ")), "image/webp");
  assert.equal(imageType(bytes("RIFF", [0, 0, 0, 0], "WAVEfmt ")), null);
  assert.equal(imageType(bytes("%PDF-1.4")), null);
  assert.equal(IMAGE_TYPES.length, 4);
});

test("the default file name is the column and the type's extension", () => {
  assert.equal(defaultFileName("scan", fileType(bytes("%PDF-1.4")), true), "scan.pdf");
  assert.equal(defaultFileName("blob", null, true), "blob.bin");
  assert.equal(defaultFileName("notes", null, false), "notes.txt");
  assert.equal(defaultFileName('a/b:c', null, true), "a_b_c.bin");
  assert.equal(defaultFileName("", null, true), "value.bin");
});
