import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, toCommandError } from "../api";
import { msg, t } from "../i18n";
import { setSetting, setStored, useSettingsVersion } from "../settings";
import {
  CUSTOM,
  CUSTOM_THEME_KEY,
  PALETTE_KEYS,
  currentThemeColors,
  customTheme,
  previewTheme,
  themeName,
  themeSetting,
  type CustomTheme,
} from "../theme";
import { readThemeFile, themeFileName, themeFileText, type ReadTheme } from "./file";

/** What `readThemeFile` throws, for the translations. */
const READ_ERRORS = [
  msg("The file is not JSON."),
  msg("The file is not a theme."),
  msg("The file is not a PlatypusDB theme: platypus_theme is missing."),
  msg("The theme is from a newer PlatypusDB."),
  msg("The theme has neither light nor dark colors."),
];

const isColor = (value: string) => CSS.supports("color", value);

function variantsLabel(theme: CustomTheme): string {
  if (theme.light && theme.dark) return t("light and dark, following the system");
  return theme.dark ? t("dark") : t("light");
}

/** The Themes page of the Settings: the theme in use saved as a file, and a file's theme
 *  previewed before it is applied. */
export function ThemesPanel() {
  useSettingsVersion();
  const [read, setRead] = useState<ReadTheme | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const imported = customTheme();

  // Leaving the page ends a preview without applying it.
  useEffect(() => () => previewTheme(null), []);

  const exportTheme = async () => {
    try {
      const theme = currentThemeColors();
      const path = await save({ filters: [{ name: "JSON", extensions: ["json"] }], defaultPath: themeFileName(theme) });
      if (!path) return;
      await api.writeTextFile(path, themeFileText(theme));
      setError(null);
      setNotice(t("Saved {path}.", { path }));
    } catch (e) {
      setError(toCommandError(e).message);
    }
  };

  const importTheme = async () => {
    try {
      const path = await open({ multiple: false, filters: [{ name: "JSON", extensions: ["json"] }] });
      if (typeof path !== "string") return;
      const result = readThemeFile(await api.readTextFile(path), PALETTE_KEYS, isColor);
      setError(null);
      setNotice(null);
      setRead(result);
      previewTheme(result.theme);
    } catch (e) {
      const message = e instanceof Error ? e.message : toCommandError(e).message;
      setError(READ_ERRORS.includes(message) ? t(message) : message);
    }
  };

  const apply = async () => {
    if (!read) return;
    await setStored(CUSTOM_THEME_KEY, JSON.stringify(read.theme));
    await setSetting(themeSetting, CUSTOM);
    previewTheme(null);
    setNotice(t("{name} is now the theme.", { name: read.theme.name }));
    setRead(null);
  };

  const cancel = () => {
    previewTheme(null);
    setRead(null);
  };

  const remove = async () => {
    if (themeName() === CUSTOM) await setSetting(themeSetting, "system");
    await setStored(CUSTOM_THEME_KEY, null);
    setNotice(null);
  };

  return (
    <div className="themes-panel">
      <p className="muted small">
        {t(
          "A theme file holds the app's and the editor's colors, for light, dark or both. Export saves the theme in use, with your custom colors; Import shows a file's theme before you apply it.",
        )}
      </p>
      {error && (
        <div className="notice error small" data-notice="theme-error">
          {error}
        </div>
      )}
      {notice && (
        <div className="notice small" data-notice="theme">
          {notice}
        </div>
      )}
      <div className="row tight">
        <button type="button" onClick={() => void exportTheme()} data-action="export-theme" disabled={!!read}>
          {t("Export theme…")}
        </button>
        <button type="button" onClick={() => void importTheme()} data-action="import-theme" disabled={!!read}>
          {t("Import theme…")}
        </button>
      </div>
      {read && (
        <div className="notice small theme-preview" data-theme-preview={read.theme.name}>
          <p>
            {t("Previewing {name} ({variants}). Apply it, or cancel to go back.", {
              name: read.theme.name,
              variants: variantsLabel(read.theme),
            })}
          </p>
          {read.skipped.length > 0 && (
            <>
              <p>{t("Left out of the file:")}</p>
              <ul className="theme-skipped">
                {read.skipped.map((s) => (
                  <li key={s}>
                    <code>{s}</code>
                  </li>
                ))}
              </ul>
            </>
          )}
          <div className="row tight">
            <span className="spacer" />
            <button type="button" onClick={cancel} data-action="cancel-theme">
              {t("Cancel")}
            </button>
            <button type="button" className="primary" onClick={() => void apply()} data-action="apply-theme">
              {t("Apply")}
            </button>
          </div>
        </div>
      )}
      {imported && !read && (
        <div className="row tight" data-imported-theme={imported.name}>
          <span>
            {t("Imported theme: {name} ({variants})", { name: imported.name, variants: variantsLabel(imported) })}
            {themeName() === CUSTOM && <span className="muted"> · {t("in use")}</span>}
          </span>
          <span className="spacer" />
          {themeName() !== CUSTOM && (
            <button type="button" onClick={() => void setSetting(themeSetting, CUSTOM)} data-action="use-theme">
              {t("Use")}
            </button>
          )}
          <button type="button" onClick={() => void remove()} data-action="remove-theme">
            {t("Remove")}
          </button>
        </div>
      )}
    </div>
  );
}
