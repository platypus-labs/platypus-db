/** Masking for screen sharing: while it is on, the values of the columns its rules name show
 *  as dots in the grid and the panels beside it. Only what is shown changes: copies, exports
 *  and edits keep the real values. */
import { useMemo } from "react";
import { registerAction } from "../actions";
import { defineSetting, getSetting, setSetting, useSetting } from "../settings";
import { DEFAULT_NAMES, MASK, masksColumn, patterns } from "./maskRules";

export { MASK };

export const maskSetting = defineSetting<boolean>({
  key: "safety.mask",
  section: "Safety",
  label: "Mask sensitive data (for screen sharing)",
  description: "Values of the columns below show as dots. Copies and exports keep the real values.",
  default: false,
  control: { type: "boolean" },
});

export const maskNamesSetting = defineSetting<string>({
  key: "safety.mask_names",
  section: "Safety",
  label: "Columns to mask, by name",
  description: "Comma-separated; * is any characters, case is ignored.",
  default: DEFAULT_NAMES,
  control: { type: "text" },
});

export const maskTypesSetting = defineSetting<string>({
  key: "safety.mask_types",
  section: "Safety",
  label: "Columns to mask, by type",
  description: "Comma-separated, e.g. inet, bytea; * is any characters.",
  default: "",
  control: { type: "text" },
});

/** Whether a column's values show masked now; `null` while masking is off. */
export function useMask(): ((column: { name: string; db_type: string }) => boolean) | null {
  const on = useSetting(maskSetting);
  const names = useSetting(maskNamesSetting);
  const types = useSetting(maskTypesSetting);
  return useMemo(() => {
    if (!on) return null;
    const [n, t] = [patterns(names), patterns(types)];
    return (column) => masksColumn(column, n, t);
  }, [on, names, types]);
}

/** `value` as shown: dots where `masked`; NULL stays NULL, which tells nothing. */
export const shown = <T extends string | null | undefined>(value: T, masked: boolean): T | string => (masked && value != null ? MASK : value);

registerAction({
  id: "safety.mask",
  scope: "global",
  title: "Mask sensitive data",
  shortcut: "Mod+Alt+M",
  run: () => void setSetting(maskSetting, !getSetting(maskSetting)),
});
