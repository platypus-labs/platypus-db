/** A CSS color as `#rrggbb` (what `<input type="color">` takes), or `null` if it is not a plain hex or rgb() color. */
export function toHex(css: string): string | null {
  const v = css.trim().toLowerCase();
  if (/^#[0-9a-f]{6}$/.test(v)) return v;
  if (/^#[0-9a-f]{3}$/.test(v)) return `#${[...v.slice(1)].map((c) => c + c).join("")}`;
  const rgb = /^rgba?\(\s*(\d{1,3})[\s,]+(\d{1,3})[\s,]+(\d{1,3})/.exec(v);
  if (!rgb) return null;
  return `#${rgb.slice(1, 4).map((n) => Math.min(255, Number(n)).toString(16).padStart(2, "0")).join("")}`;
}
