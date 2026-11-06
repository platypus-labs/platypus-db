/**
 * Whether a theme stays legible: the contrast of the pairs of colors the app draws text with,
 * by the WCAG ratio. Only ever a warning: a theme is its author's choice.
 */
import { luminance } from "./color.ts";

type Rgb = [number, number, number];

/** A color as red, green and blue from 0 to 1, when it is opaque and in a form read here; `null`
 *  otherwise, as a translucent color's contrast depends on what is behind it. */
export function opaqueRgb(value: string): Rgb | null {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value)?.[1];
  if (hex) {
    const full = hex.length === 3 ? [...hex].map((c) => c + c).join("") : hex;
    return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16) / 255) as Rgb;
  }
  const rgb = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)\s*(?:[,/]\s*([\d.]+)(%?)\s*)?\)$/i.exec(value);
  if (!rgb) return null;
  if (rgb[4] !== undefined && Number(rgb[4]) < (rgb[5] ? 100 : 1)) return null;
  return [Number(rgb[1]) / 255, Number(rgb[2]) / 255, Number(rgb[3]) / 255];
}

/** The WCAG contrast of two colors, from 1 to 21; `null` when either cannot be told. */
export function contrastRatio(a: string, b: string): number | null {
  const [first, second] = [opaqueRgb(a), opaqueRgb(b)];
  if (!first || !second) return null;
  const [la, lb] = [luminance(...first), luminance(...second)];
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** The pairs that carry text, each with the least contrast it should have: 4.5 for reading, 3 for
 *  what is large or secondary. */
const PAIRS: { fg: string; bg: string; min: number }[] = [
  { fg: "fg", bg: "bg", min: 4.5 },
  { fg: "fg", bg: "bg-sidebar", min: 4.5 },
  { fg: "fg", bg: "grid-bg", min: 4.5 },
  { fg: "fg-muted", bg: "bg", min: 3 },
  { fg: "accent-fg", bg: "accent", min: 3 },
  { fg: "syn-keyword", bg: "bg", min: 3 },
  { fg: "syn-string", bg: "bg", min: 3 },
];

export interface LowContrast {
  fg: string;
  bg: string;
  ratio: number;
}

/** The pairs of `colors` below their contrast, as a list to warn about. */
export function lowContrast(colors: Record<string, string>): LowContrast[] {
  return PAIRS.flatMap(({ fg, bg, min }) => {
    const ratio = contrastRatio(colors[fg] ?? "", colors[bg] ?? "");
    return ratio !== null && ratio < min ? [{ fg, bg, ratio: Math.round(ratio * 10) / 10 }] : [];
  });
}
