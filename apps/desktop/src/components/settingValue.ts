/** Values typed into the Settings, checked before they are saved. */

export interface NumberBounds {
  min?: number;
  max?: number;
  /** A whole step takes whole numbers only: the backend reads such settings as integers. */
  step?: number;
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
