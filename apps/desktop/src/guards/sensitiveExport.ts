/** What an export does with the sensitive columns of its result: keeps their values, masks
 *  them or leaves them out of the file. */
import type { ExportOptions } from "../api";

/** What an export does with the sensitive columns. */
export type SensitiveChoice = "keep" | "mask" | "omit";

/** `options` with the `sensitive` columns masked or left out as `choice` says. */
export function withSensitive(options: ExportOptions, sensitive: string[], choice: SensitiveChoice): ExportOptions {
  return { ...options, masked: choice === "mask" ? sensitive : [], omitted: choice === "omit" ? sensitive : [] };
}

/** The choice a saved profile made, or the default: masked while masking is on. */
export function firstChoice(options: ExportOptions | undefined, masking: boolean): SensitiveChoice {
  if (options?.omitted?.length) return "omit";
  if (options?.masked?.length) return "mask";
  return options || !masking ? "keep" : "mask";
}
