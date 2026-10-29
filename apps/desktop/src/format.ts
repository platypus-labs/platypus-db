/** A size in bytes in the largest unit under 1024 of it: `512 B`, `1.5 KB`, `23 MB`. */
export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${unit === 0 ? value : value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** `name-2026-10-29.ext`, without the characters file systems refuse in a name. */
export function datedFileName(name: string, ext: string, today = new Date()): string {
  const day = today.toISOString().slice(0, 10);
  return `${name.replace(/[<>:"/\\|?*\s]+/g, "_") || "backup"}-${day}.${ext}`;
}
