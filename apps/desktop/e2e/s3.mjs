// A minimal S3 client for the scenarios that check what an export upload left in the bucket of
// docker/compose.yml (profile `s3`, SeaweedFS on 49000, keys platypus / platypus-secret):
// requests signed with AWS Signature Version 4, path style, with nothing but node.
import { createHash, createHmac } from "node:crypto";
import { connect as tcp } from "node:net";

export const S3 = {
  endpoint: "http://127.0.0.1:49000",
  region: "us-east-1",
  bucket: "platypus",
  accessKey: "platypus",
  secretKey: "platypus-secret",
};

const sha256 = (data) => createHash("sha256").update(data).digest("hex");
const hmac = (key, data) => createHmac("sha256", key).update(data).digest();
const encode = (text) => encodeURIComponent(text).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
const encodePath = (path) => path.split("/").map(encode).join("/");

/** A reason to skip unless the store accepts connections. */
export function skipUnlessS3() {
  return new Promise((resolve) => {
    const { hostname, port } = new URL(S3.endpoint);
    const socket = tcp({ host: hostname, port: Number(port), timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolve(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done(`no S3 store on ${S3.endpoint} (docker compose -f docker/compose.yml --profile s3 up -d --wait)`));
    socket.on("timeout", () => done(`no S3 store on ${S3.endpoint}`));
  });
}

/** A signed request: `key` is the object key under the bucket (empty for the bucket itself),
 *  `query` a list of [name, value]. Returns `{ status, body (Buffer), text }`. */
export async function request(method, key = "", { query = [], body = Buffer.alloc(0), secretKey = S3.secretKey, accessKey = S3.accessKey } = {}) {
  const { host } = new URL(S3.endpoint);
  const path = `/${encode(S3.bucket)}${key ? `/${encodePath(key)}` : ""}`;
  const canonicalQuery = [...query]
    .map(([k, v]) => [encode(k), encode(v)])
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([k, v]) => `${k}=${v}`)
    .join("&");
  const now = new Date().toISOString().replace(/[-:]|\.\d{3}/g, "");
  const day = now.slice(0, 8);
  const payload = sha256(body);
  const headers = `host:${host}\nx-amz-content-sha256:${payload}\nx-amz-date:${now}\n`;
  const signed = "host;x-amz-content-sha256;x-amz-date";
  const canonical = `${method}\n${path}\n${canonicalQuery}\n${headers}\n${signed}\n${payload}`;
  const scope = `${day}/${S3.region}/s3/aws4_request`;
  const toSign = `AWS4-HMAC-SHA256\n${now}\n${scope}\n${sha256(canonical)}`;
  let signingKey = hmac(`AWS4${secretKey}`, day);
  for (const part of [S3.region, "s3", "aws4_request"]) signingKey = hmac(signingKey, part);
  const signature = createHmac("sha256", signingKey).update(toSign).digest("hex");
  const response = await fetch(`${S3.endpoint}${path}${canonicalQuery ? `?${canonicalQuery}` : ""}`, {
    method,
    headers: {
      "x-amz-date": now,
      "x-amz-content-sha256": payload,
      Authorization: `AWS4-HMAC-SHA256 Credential=${accessKey}/${scope}, SignedHeaders=${signed}, Signature=${signature}`,
    },
    body: method === "PUT" ? body : undefined,
  });
  const buffer = Buffer.from(await response.arrayBuffer());
  return { status: response.status, body: buffer, text: buffer.toString("utf8") };
}

/** The object `key` as a Buffer, or `null` when it is not there. */
export async function getObject(key) {
  const r = await request("GET", key);
  if (r.status === 404) return null;
  if (r.status !== 200) throw new Error(`GET ${key}: ${r.status} ${r.text}`);
  return r.body;
}

/** The keys of the objects whose key starts with `prefix`. */
export async function listKeys(prefix) {
  const r = await request("GET", "", { query: [["list-type", "2"], ["prefix", prefix]] });
  if (r.status !== 200) throw new Error(`list ${prefix}: ${r.status} ${r.text}`);
  return [...r.text.matchAll(/<Key>(.*?)<\/Key>/g)].map((m) => m[1].replaceAll("&amp;", "&"));
}

/** Deletes the object `key` (cleanup; failures are ignored). */
export async function deleteObject(key) {
  try {
    await request("DELETE", key);
  } catch {
    // Nothing to clean.
  }
}
