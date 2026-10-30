/** Sensitive columns and masking for screen sharing. A column is sensitive when the user marked
 *  it so in its table's tab, or, unmarked, when the rules on names and types below name it.
 *  While masking is on, the values of sensitive columns show as dots in the grid, the panels
 *  beside it and the column profile (in MongoDB documents, the fields of those names). Only
 *  what is shown changes: copies and edits keep the real values (an export can mask or leave
 *  them out); charts draw them as they are. */
import { createContext, useContext, useMemo } from "react";
import { catalogKey, sensitiveMark, useCatalogVersion } from "../catalog/catalog";
import { registerAction } from "../actions";
import { defineSetting, getSetting, setSetting, useSetting } from "../settings";
import { DEFAULT_NAMES, MASK, isSensitive, patterns } from "./maskRules";

export { MASK };

export const maskSetting = defineSetting<boolean>({
  key: "safety.mask",
  section: "Safety",
  label: "Mask sensitive data (for screen sharing)",
  description: "Values of the sensitive columns show as dots. Copies keep the real values; an export asks.",
  default: false,
  control: { type: "boolean" },
});

export const maskNamesSetting = defineSetting<string>({
  key: "safety.mask_names",
  section: "Safety",
  label: "Sensitive columns, by name",
  description: "Comma-separated; * is any characters, case is ignored. A column marked in its table's tab counts over these rules.",
  default: DEFAULT_NAMES,
  control: { type: "text" },
});

export const maskTypesSetting = defineSetting<string>({
  key: "safety.mask_types",
  section: "Safety",
  label: "Sensitive columns, by type",
  description: "Comma-separated, e.g. inet, bytea; * is any characters.",
  default: "",
  control: { type: "text" },
});

/** The table the columns shown below belong to, when known (a data viewer, a table's profile):
 *  the user's marks on its columns count over the rules. */
export interface TableScope {
  connectionId: string;
  database: string | null | undefined;
  schema: string;
  table: string;
}

export const SensitiveScope = createContext<TableScope | null>(null);

type Column = { name: string; db_type: string };

/** Whether a column is sensitive: the user's mark when the table is known (`table`, or the
 *  `SensitiveScope` around), else the rules. */
export function useSensitive(table?: TableScope | null): (column: Column) => boolean {
  const around = useContext(SensitiveScope);
  const scope = table ?? around;
  const names = useSetting(maskNamesSetting);
  const types = useSetting(maskTypesSetting);
  // The marks load and change in the catalog.
  const marks = useCatalogVersion();
  return useMemo(() => {
    const [n, t] = [patterns(names), patterns(types)];
    return (column) => {
      const mark = scope && sensitiveMark(scope.connectionId, catalogKey(scope.database, scope.schema, scope.table, column.name));
      return isSensitive(mark, column, n, t);
    };
    // The scope's fields, not the object: callers build it on each render.
  }, [scope?.connectionId, scope?.database, scope?.schema, scope?.table, names, types, marks]);
}

/** Whether a column's values show masked now; `null` while masking is off. */
export function useMask(table?: TableScope | null): ((column: Column) => boolean) | null {
  const on = useSetting(maskSetting);
  const sensitive = useSensitive(table);
  return on ? sensitive : null;
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
