// A field of a plugin's settings page as the app's own settings are declared, so that the Settings
// dialog draws both the same way. Kept apart from the page, which needs the app around it.
import type { PluginSettingField, PluginSettingState } from "../api";
import type { SettingControl, SettingDef } from "../settings";

type Value = string | number | boolean;

/** What the field's control holds until the user sets a value, when the manifest says nothing. */
function emptyValue(field: PluginSettingField): Value {
  switch (field.type) {
    case "number":
      return field.min ?? 0;
    case "boolean":
      return false;
    case "choice":
      return field.options?.[0]?.value ?? "";
    default:
      return "";
  }
}

function control(field: PluginSettingField): SettingControl {
  switch (field.type) {
    case "number":
      return { type: "number", min: field.min, max: field.max };
    case "boolean":
      return { type: "boolean" };
    case "choice":
      return { type: "select", options: field.options ?? [] };
    default:
      return { type: "text" };
  }
}

/** The declaration of `field`, a field other than a secret, on the page of the plugin `plugin`. */
export function fieldDef(plugin: { id: string; name: string }, field: PluginSettingField): SettingDef<Value> {
  return {
    key: `${plugin.id}.${field.key}`,
    section: plugin.name,
    label: field.label,
    description: field.description,
    default: field.default ?? emptyValue(field),
    control: control(field),
  };
}

/** What the control of `field` shows: the value saved, or the default. */
export function fieldValue(field: PluginSettingField, state: PluginSettingState | undefined): Value {
  return state?.value ?? field.default ?? emptyValue(field);
}

/** The value to save for what the control gave: `null` when it is the default, so that a later
 *  version's default applies. */
export function valueToSave(def: SettingDef<Value>, value: Value | null): Value | null {
  return value === def.default ? null : value;
}
