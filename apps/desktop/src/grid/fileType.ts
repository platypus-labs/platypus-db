// What the frontend needs of telling a file from a cell's bytes: the images a browser draws
// (the value panel shows these) and the default name of a saved cell. Which cells hold a file,
// and of what type, is told by the backend (`filetype.rs`, command `cell_file`).

/** The image types a browser draws, by their first bytes. */
const IMAGE_TYPES: [string, number[]][] = [
  ["image/png", [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]],
  ["image/jpeg", [0xff, 0xd8, 0xff]],
  ["image/gif", [0x47, 0x49, 0x46, 0x38]],
  ["image/bmp", [0x42, 0x4d]],
];

const startsWith = (bytes: Uint8Array, magic: number[]) => magic.every((b, i) => bytes[i] === b);

/** The MIME type of an image a browser draws, from its first bytes; `null` for anything else. */
export function imageType(bytes: Uint8Array): string | null {
  const known = IMAGE_TYPES.find(([, magic]) => startsWith(bytes, magic));
  if (known) return known[0];
  // RIFF....WEBP
  return startsWith(bytes, [0x52, 0x49, 0x46, 0x46]) && startsWith(bytes.subarray(8), [0x57, 0x45, 0x42, 0x50]) ? "image/webp" : null;
}

/** The name a saved cell gets by default: the column, with the extension of its type. */
export function defaultFileName(column: string, file: { ext: string } | null, binary: boolean): string {
  const stem = column.replace(/[<>:"/\|?*\x00-\x1f]/g, "_").replace(/[. ]+$/, "") || "value";
  return `${stem}.${file?.ext ?? (binary ? "bin" : "txt")}`;
}
