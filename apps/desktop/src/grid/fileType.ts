// Tells a file from a cell's bytes by its magic numbers, never by the column's type. Pure, so
// that it is tested on its own. The backend (`filetype.rs`) does the same for the grid's
// labels, for text cells that hold a file coded, and for saving.

export interface FileType {
  /** How the UI names it: `PDF`. */
  name: string;
  /** The extension a saved file gets, without the dot. */
  ext: string;
  mime: string;
}

const type = (name: string, ext: string, mime: string): FileType => ({ name, ext, mime });

const ZIP = type("ZIP", "zip", "application/zip");
const DOCX = type("DOCX", "docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document");
const XLSX = type("XLSX", "xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet");
const PPTX = type("PPTX", "pptx", "application/vnd.openxmlformats-officedocument.presentationml.presentation");
const ODT = type("ODT", "odt", "application/vnd.oasis.opendocument.text");
const ODS = type("ODS", "ods", "application/vnd.oasis.opendocument.spreadsheet");

/** The image types a browser draws; the value panel shows these. */
export const IMAGE_TYPES: [string, number[]][] = [
  ["image/png", [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]],
  ["image/jpeg", [0xff, 0xd8, 0xff]],
  ["image/gif", [0x47, 0x49, 0x46, 0x38]],
  ["image/bmp", [0x42, 0x4d]],
];

/** The ASCII bytes of `text`. */
const ascii = (text: string) => Array.from(text, (c) => c.charCodeAt(0));

function at(bytes: Uint8Array, offset: number, magic: number[] | string): boolean {
  const m = typeof magic === "string" ? ascii(magic) : magic;
  return offset + m.length <= bytes.length && m.every((b, i) => bytes[offset + i] === b);
}

const le16 = (b: Uint8Array, at: number) => (at + 2 <= b.length ? b[at]! | (b[at + 1]! << 8) : -1);
const le32 = (b: Uint8Array, at: number) => (at + 4 <= b.length ? (b[at]! | (b[at + 1]! << 8) | (b[at + 2]! << 16) | (b[at + 3]! << 24)) >>> 0 : -1);

/** What a ZIP is, from the names of its entries (Office) or its `mimetype` entry (OpenDocument). */
function zipType(bytes: Uint8Array): FileType {
  const sig = ascii("PK\x03\x04");
  const text = (from: number, to: number) => String.fromCharCode(...bytes.subarray(from, to));
  let pos = 0;
  while (at(bytes, pos, sig)) {
    const flags = le16(bytes, pos + 6);
    const size = le32(bytes, pos + 18);
    const nameLen = le16(bytes, pos + 26);
    const extraLen = le16(bytes, pos + 28);
    if (flags < 0 || size < 0 || nameLen < 0 || extraLen < 0) break;
    const start = pos + 30;
    if (start + nameLen > bytes.length) break;
    const name = text(start, start + nameLen);
    const data = start + nameLen + extraLen;
    if (name === "mimetype") {
      const mime = text(data, Math.min(data + Math.min(size, 80), bytes.length));
      if (mime.startsWith("application/vnd.oasis.opendocument.text")) return ODT;
      if (mime.startsWith("application/vnd.oasis.opendocument.spreadsheet")) return ODS;
    }
    if (name.startsWith("word/")) return DOCX;
    if (name.startsWith("xl/")) return XLSX;
    if (name.startsWith("ppt/")) return PPTX;
    // With a data descriptor the size is not known here: the next signature is searched for.
    if (size > 0 && (flags & 8) === 0) pos = data + size;
    else {
      let next = -1;
      for (let i = data; i + 4 <= bytes.length; i++) {
        if (at(bytes, i, sig)) {
          next = i;
          break;
        }
      }
      if (next < 0) break;
      pos = next;
    }
  }
  return ZIP;
}

/** XML text with an `<svg` element near its start. */
function isSvg(bytes: Uint8Array): boolean {
  const head = new TextDecoder("utf-8", { fatal: false }).decode(bytes.subarray(0, 2048)).replace(/^﻿/, "").trimStart();
  return /^(<svg|<\?xml|<!--|<!DOCTYPE svg)/.test(head) && head.includes("<svg");
}

/** The kind of file `bytes` start like (the first 64 KB are enough), or `null`. Short or
 *  random bytes are none. */
export function fileType(bytes: Uint8Array): FileType | null {
  const b = bytes;
  if (at(b, 0, "%PDF-")) return type("PDF", "pdf", "application/pdf");
  if (at(b, 0, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) return type("PNG", "png", "image/png");
  if (at(b, 0, [0xff, 0xd8, 0xff])) return type("JPEG", "jpg", "image/jpeg");
  if (at(b, 0, "GIF87a") || at(b, 0, "GIF89a")) return type("GIF", "gif", "image/gif");
  if (at(b, 0, "RIFF") && at(b, 8, "WEBP")) return type("WebP", "webp", "image/webp");
  if (at(b, 0, "RIFF") && at(b, 8, "WAVE")) return type("WAV", "wav", "audio/wav");
  if (at(b, 0, "BM") && b.length >= 18 && at(b, 6, [0, 0, 0, 0]) && [12, 40, 52, 56, 64, 108, 124].includes(le32(b, 14))) {
    return type("BMP", "bmp", "image/bmp");
  }
  if (at(b, 0, [0x49, 0x49, 0x2a, 0x00]) || at(b, 0, [0x4d, 0x4d, 0x00, 0x2a])) return type("TIFF", "tif", "image/tiff");
  if (at(b, 0, "PK\x03\x04")) return zipType(b);
  if (at(b, 0, [0x1f, 0x8b, 0x08])) return type("GZIP", "gz", "application/gzip");
  if (at(b, 0, [0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c])) return type("7Z", "7z", "application/x-7z-compressed");
  if (at(b, 0, [0x52, 0x61, 0x72, 0x21, 0x1a, 0x07])) return type("RAR", "rar", "application/vnd.rar");
  if (at(b, 257, "ustar")) return type("TAR", "tar", "application/x-tar");
  if (at(b, 0, "ID3") && b.length >= 10 && b[3]! < 5) return type("MP3", "mp3", "audio/mpeg");
  if (at(b, 0, "OggS")) return type("OGG", "ogg", "audio/ogg");
  if (at(b, 4, "ftyp")) return at(b, 8, "qt  ") ? type("MOV", "mov", "video/quicktime") : type("MP4", "mp4", "video/mp4");
  if (at(b, 0, [0x1a, 0x45, 0xdf, 0xa3])) {
    const head = String.fromCharCode(...b.subarray(0, 64));
    return head.includes("webm") ? type("WebM", "webm", "video/webm") : type("MKV", "mkv", "video/x-matroska");
  }
  if (at(b, 0, "SQLite format 3\0")) return type("SQLite", "sqlite", "application/vnd.sqlite3");
  if (isSvg(b)) return type("SVG", "svg", "image/svg+xml");
  return null;
}

/** The MIME type of an image a browser draws, from its first bytes; `null` for anything else. */
export function imageType(bytes: Uint8Array): string | null {
  for (const [mime, magic] of IMAGE_TYPES) {
    if (at(bytes, 0, magic)) return mime;
  }
  return at(bytes, 0, "RIFF") && at(bytes, 8, "WEBP") ? "image/webp" : null;
}

/** The name a saved cell gets by default: the column, with the extension of its type. */
export function defaultFileName(column: string, file: FileType | null, binary: boolean): string {
  const stem = column.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").replace(/[. ]+$/, "") || "value";
  return `${stem}.${file?.ext ?? (binary ? "bin" : "txt")}`;
}
