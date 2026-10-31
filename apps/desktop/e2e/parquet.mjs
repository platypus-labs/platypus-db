// Reads the footer of a Parquet file with nothing but node: the Thrift compact protocol the
// footer is written in, enough to tell the row count, the columns (name and physical type) and
// the compression of each column chunk. The values stay unread: scenarios that need them import
// the file into the app.
import { readFileSync } from "node:fs";

/** The physical types of the Parquet format, by the number in the schema. */
export const TYPES = ["BOOLEAN", "INT32", "INT64", "INT96", "FLOAT", "DOUBLE", "BYTE_ARRAY", "FIXED_LEN_BYTE_ARRAY"];
/** The compression codecs, by the number in a column chunk. */
export const CODECS = ["UNCOMPRESSED", "SNAPPY", "GZIP", "LZO", "BROTLI", "LZ4", "ZSTD", "LZ4_RAW"];

/** A Thrift compact struct at `at` of `buf`: `{ fieldId: value }` (lists as arrays, structs as
 *  objects, binary as buffers); returns the struct and where it ends. */
function readStruct(buf, at) {
  const fields = {};
  let id = 0;
  let pos = at;
  const varint = () => {
    let result = 0n;
    let shift = 0n;
    for (;;) {
      const byte = buf[pos++];
      result |= BigInt(byte & 0x7f) << shift;
      if (!(byte & 0x80)) return result;
      shift += 7n;
    }
  };
  const zigzag = () => {
    const v = varint();
    return Number((v >> 1n) ^ -(v & 1n));
  };
  const value = (type) => {
    switch (type) {
      case 1:
        return true;
      case 2:
        return false;
      case 3:
        return buf[pos++];
      case 4:
      case 5:
      case 6:
        return zigzag();
      case 7:
        return buf.readDoubleLE((pos += 8) - 8);
      case 8: {
        const length = Number(varint());
        return buf.subarray(pos, (pos += length));
      }
      case 9:
      case 10: {
        const head = buf[pos++];
        let size = head >> 4;
        const element = head & 0x0f;
        if (size === 15) size = Number(varint());
        // A boolean in a list takes a byte of its own.
        return Array.from({ length: size }, () => (element === 1 || element === 2 ? buf[pos++] === 1 : value(element)));
      }
      case 12: {
        const [struct, end] = readStruct(buf, pos);
        pos = end;
        return struct;
      }
      default:
        throw new Error(`Thrift type ${type} at ${pos}`);
    }
  };
  for (;;) {
    const head = buf[pos++];
    if (head === 0) return [fields, pos];
    const type = head & 0x0f;
    const delta = head >> 4;
    id = delta === 0 ? zigzag() : id + delta;
    fields[id] = value(type);
  }
}

/** What the footer of the Parquet file `path` says: `{ rows, columns: [{ name, type, codec }] }`.
 *  Throws when the file is not Parquet (the magic `PAR1` at both ends). */
export function parquetInfo(path) {
  const buf = readFileSync(path);
  const magic = (from) => buf.subarray(from, from + 4).toString("latin1");
  if (magic(0) !== "PAR1" || magic(buf.length - 4) !== "PAR1") throw new Error(`${path} is not a Parquet file`);
  const footer = buf.readUInt32LE(buf.length - 8);
  const [meta] = readStruct(buf, buf.length - 8 - footer);
  // The first schema element is the root; the others are the columns.
  const schema = meta[2].slice(1);
  const chunks = (meta[4] ?? []).flatMap((group) => group[1]);
  const columns = schema.map((element, i) => ({
    name: element[4].toString("utf8"),
    type: TYPES[element[1]],
    // The compression of the column's first chunk.
    codec: CODECS[chunks[i]?.[3]?.[4] ?? 0],
  }));
  return { rows: meta[3], groups: (meta[4] ?? []).length, columns };
}
