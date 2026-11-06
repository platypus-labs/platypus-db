import { useCallback, useEffect, useState } from "react";
import { api, type InstalledPlugin, type PluginSettingField, type PluginSettingState, toCommandError } from "../api";
import { SettingRow } from "../components/SettingRow";
import { t } from "../i18n";
import { fieldDef, fieldValue, valueToSave } from "./settingFields";

/** A secret field: the keychain holds it, and the page only knows whether it is set. What is typed
 *  is saved on blur or Enter and the field empties. */
function SecretRow(props: { pluginId: string; field: PluginSettingField; isSet: boolean; onSave: (value: string | null) => void }) {
  const { pluginId, field, isSet, onSave } = props;
  const [text, setText] = useState("");
  const commit = () => {
    if (text === "") return;
    onSave(text);
    setText("");
  };
  return (
    <label className="setting" data-key={`${pluginId}.${field.key}`} data-secret={isSet ? "set" : "unset"}>
      <span>
        <span className="setting-title">{field.label}</span>
        {field.description && <span className="muted small">{field.description}</span>}
      </span>
      <input
        type="password"
        autoComplete="off"
        value={text}
        placeholder={isSet ? t("Saved in the keychain") : t("Not set")}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
      />
      <button type="button" className="link setting-reset" data-action="clear-secret" style={{ visibility: isSet ? "visible" : "hidden" }} onClick={() => onSave(null)}>
        {t("Clear")}
      </button>
    </label>
  );
}

/** The settings page of a plugin, from the fields of its manifest. A chunk of its own, loaded when
 *  it opens; a plugin's fields are in its own language, as its actions are. */
export default function PluginSettings({ plugin }: { plugin: InstalledPlugin }) {
  const { id, name, settings } = plugin.manifest;
  const [states, setStates] = useState<PluginSettingState[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => api.pluginSettings(id).then(setStates), [id]);
  useEffect(() => {
    load().catch((e: unknown) => setError(toCommandError(e).message));
  }, [load]);

  async function save(key: string, value: string | number | boolean | null) {
    setError(null);
    try {
      await api.pluginSetSetting(id, key, value);
      await load();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  return (
    <div className="plugin-settings" data-field="plugin-settings">
      {error && (
        <div className="notice error small" data-field="plugin-settings-error">
          {error}
        </div>
      )}
      {states &&
        (settings ?? []).map((field) => {
          const state = states.find((s) => s.key === field.key);
          if (field.type === "secret") {
            return <SecretRow key={field.key} pluginId={id} field={field} isSet={state?.set ?? false} onSave={(v) => void save(field.key, v)} />;
          }
          const def = fieldDef({ id, name }, field);
          return <SettingRow key={field.key} def={def} current={fieldValue(field, state)} onSave={(v) => void save(field.key, valueToSave(def, v))} />;
        })}
    </div>
  );
}
