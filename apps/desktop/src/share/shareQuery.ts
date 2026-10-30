// A query shared with someone else: the `.platypus` file and the `platypus://query?d=…` link.
// Pure helpers (no React, no Tauri) so that node tests can run them. The target is a
// descriptor of where the query ran, never a connection: it carries no user, password, key or
// token, so that sharing cannot leak a secret.

/** The `format` member of the file. */
export const SHARE_FORMAT = "platypus-query";
/** The file layout this code reads and writes; other versions are refused, not guessed at. */
export const SHARE_VERSION = 1;
/** The largest file or link text read, so a hostile file cannot make the app allocate freely. */
export const MAX_SHARE_BYTES = 1024 * 1024;
/** The longest link offered: past it, chat and mail clients start to cut links, so the file is
 *  offered instead. */
export const MAX_LINK_CHARS = 8 * 1024;

/** The extension of a shared query's file. */
export const SHARE_EXTENSION = "platypus";

/** Whether `path` names a shared query's file. */
export const isShareFile = (path: string) => /\.platypus$/i.test(path);

const LINK_PREFIX = "platypus://query";
const MAX_TEXT = 512;

/** Where a shared query ran, as far as it is worth finding again on another machine. */
export interface ShareTarget {
  driver: string;
  host: string;
  port: number | null;
  database: string;
  schema: string | null;
  /** The sender's name for the connection; only a hint for the receiver. */
  name: string;
}

/** A query with what is needed to run it elsewhere. */
export interface SharedQuery {
  title: string;
  sql: string;
  /** Values of the query's variables; left out unless the sender chose to share them. */
  parameters: Record<string, string>;
  target: ShareTarget;
}

export type ShareErrorCode = "format" | "version" | "too_large" | "shape" | "link" | "unsupported";

/** Why a file or link was refused; the UI turns the code into a message in the user's language. */
export class ShareError extends Error {
  readonly code: ShareErrorCode;

  constructor(code: ShareErrorCode, detail = "") {
    super(`${code}${detail ? `: ${detail}` : ""}`);
    this.name = "ShareError";
    this.code = code;
  }
}

/** The file for `query`: a JSON document with a fixed member order, so the same query gives
 *  the same bytes. Members that are not part of the layout are not written. */
export function encodeFile(query: SharedQuery): string {
  const { driver, host, port, database, schema, name } = query.target;
  return `${JSON.stringify(
    {
      format: SHARE_FORMAT,
      version: SHARE_VERSION,
      title: query.title,
      sql: query.sql,
      ...(Object.keys(query.parameters).length > 0 && { parameters: query.parameters }),
      target: { driver, host, port, database, schema, name },
    },
    null,
    2,
  )}\n`;
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

function text(v: unknown, what: string, max = MAX_TEXT): string {
  if (typeof v !== "string") throw new ShareError("shape", what);
  if (v.length > max) throw new ShareError("too_large", what);
  return v;
}

function target(v: unknown): ShareTarget {
  if (!isObject(v)) throw new ShareError("shape", "target");
  const port = v.port ?? null;
  if (port !== null && !(typeof port === "number" && Number.isInteger(port) && port >= 0 && port <= 65535)) {
    throw new ShareError("shape", "target.port");
  }
  return {
    driver: text(v.driver, "target.driver", 64),
    host: text(v.host ?? "", "target.host"),
    port,
    database: text(v.database ?? "", "target.database"),
    schema: v.schema == null ? null : text(v.schema, "target.schema"),
    name: text(v.name ?? "", "target.name"),
  };
}

function parameters(v: unknown): Record<string, string> {
  if (v === undefined) return {};
  if (!isObject(v)) throw new ShareError("shape", "parameters");
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(v)) out[key] = text(value, "parameters", MAX_SHARE_BYTES);
  return out;
}

/** Checks a parsed document and builds the query from it. */
export function validate(doc: unknown): SharedQuery {
  if (!isObject(doc) || doc.format !== SHARE_FORMAT) throw new ShareError("format");
  if (doc.version !== SHARE_VERSION) throw new ShareError("version", String(doc.version));
  return {
    title: text(doc.title ?? "", "title"),
    sql: text(doc.sql, "sql", MAX_SHARE_BYTES),
    parameters: parameters(doc.parameters),
    target: target(doc.target),
  };
}

/** Reads the text of a `.platypus` file; throws a [`ShareError`] for anything else. */
export function decodeFile(content: string): SharedQuery {
  if (content.length > MAX_SHARE_BYTES) throw new ShareError("too_large", "file");
  let doc: unknown;
  try {
    doc = JSON.parse(content.replace(/^﻿/, ""));
  } catch {
    throw new ShareError("format");
  }
  return validate(doc);
}

// Link payload: one marker byte, then the file's JSON (compact), deflated when the webview can.
const RAW = 0;
const DEFLATED = 1;

function toBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(s: string): Uint8Array {
  if (!/^[A-Za-z0-9_-]*$/.test(s)) throw new ShareError("link");
  try {
    return Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
  } catch {
    throw new ShareError("link");
  }
}

async function pipe(bytes: Uint8Array, stream: CompressionStream | DecompressionStream, limit: number): Promise<Uint8Array> {
  const writer = stream.writable.getWriter();
  // The write only settles once the output is read, and a failure shows on the read side too.
  void writer
    .write(bytes as BufferSource)
    .then(() => writer.close())
    .catch(() => undefined);
  const reader = stream.readable.getReader();
  const parts: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read().catch(() => {
      throw new ShareError("link");
    });
    if (done) break;
    size += value.length;
    // A few bytes of input can inflate to gigabytes: stop reading at the limit.
    if (size > limit) {
      await reader.cancel();
      throw new ShareError("too_large", "link");
    }
    parts.push(value);
  }
  const out = new Uint8Array(size);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

const canDeflate = () => typeof CompressionStream === "function" && typeof DecompressionStream === "function";

/** The link for `query`. Throws `too_large` when it would pass [`MAX_LINK_CHARS`]: the caller
 *  offers the file then. */
export async function encodeLink(query: SharedQuery): Promise<string> {
  const json = new TextEncoder().encode(JSON.stringify(JSON.parse(encodeFile(query))));
  const deflated = canDeflate() ? await pipe(json, new CompressionStream("deflate-raw"), MAX_SHARE_BYTES) : null;
  // Tiny payloads can grow when deflated.
  const [marker, body] = deflated && deflated.length < json.length ? [DEFLATED, deflated] : [RAW, json];
  const payload = new Uint8Array(body.length + 1);
  payload[0] = marker;
  payload.set(body, 1);
  const link = `${LINK_PREFIX}?d=${toBase64Url(payload)}`;
  if (link.length > MAX_LINK_CHARS) throw new ShareError("too_large", "link");
  return link;
}

/** Whether `s` looks like a link of this app (for an argument that is a path or a link). */
export const isShareLink = (s: string) => /^platypus:\/\/query[/?]/i.test(s.trim());

/** Reads a `platypus://query?d=…` link. */
export async function decodeLink(link: string): Promise<SharedQuery> {
  const trimmed = link.trim();
  if (trimmed.length > MAX_SHARE_BYTES) throw new ShareError("too_large", "link");
  const match = /^platypus:\/\/query\/?\?([^#]*)/i.exec(trimmed);
  if (!match) throw new ShareError("link");
  const d = new URLSearchParams(match[1]).get("d");
  if (!d) throw new ShareError("link");
  const payload = fromBase64Url(d);
  const body = payload.subarray(1);
  let json: Uint8Array;
  if (payload[0] === RAW) json = body;
  else if (payload[0] === DEFLATED) {
    if (!canDeflate()) throw new ShareError("unsupported");
    json = await pipe(body, new DecompressionStream("deflate-raw"), MAX_SHARE_BYTES);
  } else throw new ShareError("link");
  return decodeFile(new TextDecoder().decode(json));
}
