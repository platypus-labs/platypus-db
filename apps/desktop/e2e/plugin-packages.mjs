// Plugin packages for the scenarios: a `.zip` with its files stored as they are, and a minisign key
// that signs as `minisign -S` does (a pre-hashed Ed25519 signature and a signed comment).
import { createHash, generateKeyPairSync, randomBytes, sign } from "node:crypto";
import { crc32 } from "node:zlib";

/** A `.zip` of `files`, `{ name: bytes or text }`, without compression. */
export function storedZip(files) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const [name, content] of Object.entries(files)) {
    const nameBytes = Buffer.from(name);
    const bytes = Buffer.from(content);
    const crc = crc32(bytes);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(bytes.length, 18);
    local.writeUInt32LE(bytes.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(20, 4);
    entry.writeUInt16LE(20, 6);
    entry.writeUInt32LE(crc, 16);
    entry.writeUInt32LE(bytes.length, 20);
    entry.writeUInt32LE(bytes.length, 24);
    entry.writeUInt16LE(nameBytes.length, 28);
    entry.writeUInt32LE(offset, 42);
    locals.push(local, nameBytes, bytes);
    central.push(entry, nameBytes);
    offset += local.length + nameBytes.length + bytes.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(Object.keys(files).length, 8);
  end.writeUInt16LE(Object.keys(files).length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}

/** A new minisign key: `publicKey` as the app takes it, and `sign(bytes)` the text of a signature. */
export function minisignKey() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const id = randomBytes(8);
  // The last 32 bytes of the DER form are the raw key.
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  return {
    publicKey: Buffer.concat([Buffer.from("Ed"), id, raw]).toString("base64"),
    sign(bytes) {
      const signature = sign(null, createHash("blake2b512").update(bytes).digest(), privateKey);
      const comment = "timestamp:1 file:package.zip";
      const global = sign(null, Buffer.concat([signature, Buffer.from(comment)]), privateKey);
      const line = Buffer.concat([Buffer.from("ED"), id, signature]).toString("base64");
      return `untrusted comment: signature\n${line}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`;
    },
  };
}

export const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
