import { t } from "../i18n";
import { pluginDrivers, usePlugins } from "./store";

interface Props {
  /** The plugin's id, the connection's `plugin` option. */
  value: string;
  onChange: (id: string) => void;
}

/** The plugin that is the driver of a connection, among the installed ones; a plugin removed
 *  or turned off since stays listed, marked, so the field keeps saying what it was. */
export function PluginDriverField({ value, onChange }: Props) {
  const drivers = pluginDrivers(usePlugins());
  const missing = value && !drivers.some((d) => d.id === value);
  return (
    <label className="field" data-field="plugin-driver">
      <span>{t("Plugin")}</span>
      <select required value={value} onChange={(e) => onChange(e.target.value)}>
        {!value && <option value="">{t("Choose a plugin…")}</option>}
        {missing && <option value={value}>{t("{id} (not installed or turned off)", { id: value })}</option>}
        {drivers.map((d) => (
          <option key={d.id} value={d.id}>
            {d.name}
          </option>
        ))}
      </select>
      {drivers.length === 0 && <span className="muted small">{t("Install a plugin with a driver in Settings, Plugins.")}</span>}
    </label>
  );
}
