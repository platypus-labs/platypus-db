/** Values typed into the Settings, checked before they are saved. */

interface NumberBounds {
  min?: number;
  max?: number;
  /** A whole step takes whole numbers only: the backend reads such settings as integers. */
  step?: number;
}

/** What a rejected number should tell: the range it must be in, only one bound, or just that it is no number. */
export function numberRule({ min, max }: NumberBounds): "between" | "min" | "max" | "number" {
  if (min !== undefined && max !== undefined) return "between";
  return min !== undefined ? "min" : max !== undefined ? "max" : "number";
}

/** The number `text` holds when the setting accepts it; `null` otherwise. */
export function settingNumber(text: string, { min, max, step }: NumberBounds): number | null {
  if (text.trim() === "") return null;
  const n = Number(text);
  if (!Number.isFinite(n)) return null;
  if (min !== undefined && n < min) return null;
  if (max !== undefined && n > max) return null;
  if (step !== undefined && Number.isInteger(step) && !Number.isInteger(n)) return null;
  return n;
}
