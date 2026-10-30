import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { shortcutLabel, useActions } from "../actions";
import { DATA_FILE_EXTENSIONS, toCommandError } from "../api";
import bannerOnDark from "../assets/brand/platypus-db-on-dark.png";
import bannerOnLight from "../assets/brand/platypus-db-on-light.png";
import { Icon } from "../ui/Icon";
import { t } from "../i18n";
import { SHARE_EXTENSION } from "../share/shareQuery";

interface Props {
  /** No connection saved yet: the first start. */
  empty: boolean;
  version: string;
  onNew: () => void;
  onImport: () => void;
  /** Opens a data file as a DuckDB database. */
  onOpenFile: (path: string) => void;
  /** Opens the sample database, made at the first click. */
  onSample: () => Promise<void>;
  onError: (message: string) => void;
}

/** The shell's actions worth knowing first, with their shortcuts as the user set them. */
const SHORTCUTS = ["app.palette", "tab.new", "tab.reopen", "app.settings"];

/** Asks for a data file to open; `null` when none is chosen. */
export async function pickDataFile(): Promise<string | null> {
  const path = await open({
    multiple: false,
    filters: [
      { name: t("Data files"), extensions: DATA_FILE_EXTENSIONS },
      { name: t("Shared queries"), extensions: [SHARE_EXTENSION] },
    ],
  });
  return typeof path === "string" ? path : null;
}

/** What the workspace shows with no tab open. */
export function Welcome({ empty, version, onNew, onImport, onOpenFile, onSample, onError }: Props) {
  const [sampleBusy, setSampleBusy] = useState(false);
  async function pickFile() {
    const path = await pickDataFile();
    if (path !== null) onOpenFile(path);
  }
  const global = useActions("global", undefined);
  const shortcuts = SHORTCUTS.flatMap((id) => {
    const action = global.find((a) => a.id === id);
    const keys = action && shortcutLabel(action);
    return action && keys ? [{ id, title: t(action.title), keys }] : [];
  });
  return (
    <div className="welcome">
      <div className="welcome-intro">
        <h1>
          <img className="banner banner-on-light" src={bannerOnLight} alt="PlatypusDB" />
          <img className="banner banner-on-dark" src={bannerOnDark} alt="PlatypusDB" />
        </h1>
        <p className="welcome-lead">
          {empty
            ? t("A SQL client for your databases and data files. Add a connection to get started.")
            : t("Double-click a connection to open a SQL editor, or expand it to browse.")}
        </p>
        <button type="button" className="primary welcome-cta" onClick={onNew}>
          <Icon name="plus" size={15} />
          {t("New connection")}
        </button>
        <p className="muted small">v{version}</p>
      </div>
      <div className="welcome-actions">
        <button type="button" className="welcome-card" onClick={() => pickFile().catch((e: unknown) => onError(toCommandError(e).message))}>
          <Icon name="file" size={18} />
          <span>
            <strong>{t("Open a data file")}</strong>
            <span className="muted">{t("CSV, Parquet, JSON or a spreadsheet, queried with DuckDB. You can also drop it on the window.")}</span>
          </span>
        </button>
        <button
          type="button"
          className="welcome-card"
          // The first click builds the database: a second one meanwhile would open it twice.
          disabled={sampleBusy}
          aria-busy={sampleBusy}
          onClick={() => {
            setSampleBusy(true);
            onSample()
              .catch((e: unknown) => onError(toCommandError(e).message))
              .finally(() => setSampleBusy(false));
          }}
        >
          <Icon name="database" size={18} />
          <span>
            <strong>{t("Try the sample database")}</strong>
            <span className="muted">{t("A SQLite field survey of platypuses: linked tables, a view and a few saved scripts, no server needed.")}</span>
          </span>
        </button>
        <button type="button" className="welcome-card" onClick={onImport}>
          <Icon name="import" size={18} />
          <span>
            <strong>{t("Import connections")}</strong>
            <span className="muted">{t("From DBeaver, DataGrip or pgAdmin.")}</span>
          </span>
        </button>
        <dl className="welcome-keys">
          {shortcuts.map((s) => (
            <div key={s.id}>
              <dt>{s.title}</dt>
              <dd>
                <kbd>{s.keys}</kbd>
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </div>
  );
}
