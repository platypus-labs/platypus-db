import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { shortcutLabel, useActions } from "../actions";
import { api, DATA_FILE_EXTENSIONS, type SavedConnection, toCommandError } from "../api";
import { DriverIcon } from "../components/DriverPicker";
import { typeBadge } from "../connections";
import bannerOnDark from "../assets/brand/platypus-db-on-dark.png";
import bannerOnLight from "../assets/brand/platypus-db-on-light.png";
import { Icon } from "../ui/Icon";
import { language, t } from "../i18n";
import { openScriptFile } from "../scripts/scripts";
import { recentScripts, RECENT_SCRIPTS } from "../scripts/recent";
import { openSharedProject } from "../projects/shared";
import { SHARE_EXTENSION } from "../share/shareQuery";

interface Props {
  /** No connection saved yet (the first start); `null` while the list loads, when neither line
   *  is shown yet. */
  empty: boolean | null;
  version: string;
  onNew: () => void;
  onImport: () => void;
  onExport: () => void;
  /** Opens a data file as a DuckDB database. */
  onOpenFile: (path: string) => void;
  /** Opens the sample database, made at the first click. */
  onSample: () => Promise<void>;
  onError: (message: string) => void;
  /** The saved connections: the recent ones are listed. */
  connections: SavedConnection[];
  onOpenConnection: (connection: SavedConnection) => void;
}

/** The connections used last, from the history of the queries: at most this many. */
const RECENT = 5;

/** When a connection was used last: "2 hours ago", in the app's language. */
function ago(epochMs: number): string {
  const seconds = (epochMs - Date.now()) / 1000;
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ["year", 31_536_000],
    ["month", 2_592_000],
    ["week", 604_800],
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ];
  const format = new Intl.RelativeTimeFormat(language(), { numeric: "auto" });
  for (const [unit, size] of units) {
    if (Math.abs(seconds) >= size) return format.format(Math.round(seconds / size), unit);
  }
  return format.format(0, "minute");
}

/** The connections queried last, newest first, with when. */
function useRecent(connections: SavedConnection[]): { connection: SavedConnection; at: number }[] {
  const [recent, setRecent] = useState<{ connection: SavedConnection; at: number }[]>([]);
  useEffect(() => {
    if (connections.length === 0) return;
    let live = true;
    api
      .searchHistory({ limit: 200 })
      .then((entries) => {
        const seen = new Map<string, number>();
        for (const e of entries) if (!seen.has(e.connection_id)) seen.set(e.connection_id, e.started_at);
        const byId = new Map(connections.map((c) => [c.id, c]));
        const list = [...seen].flatMap(([id, at]) => (byId.has(id) ? [{ connection: byId.get(id)!, at }] : []));
        if (live) setRecent(list.slice(0, RECENT));
      })
      // The history is a convenience here: without it the list stays empty.
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [connections]);
  return recent;
}

/** The scripts opened or saved last on a connection that still exists, newest first. */
function useRecentScripts(connections: SavedConnection[]) {
  const [scripts] = useState(recentScripts);
  const ids = new Set(connections.map((c) => c.id));
  // A script of a deleted connection opens on the first one left.
  return scripts.slice(0, RECENT_SCRIPTS).flatMap((s) => {
    const connectionId = ids.has(s.connectionId) ? s.connectionId : connections[0]?.id;
    return connectionId ? [{ ...s, connectionId }] : [];
  });
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
export function Welcome({ empty, version, onNew, onImport, onExport, onOpenFile, onSample, onError, connections, onOpenConnection }: Props) {
  const [sampleBusy, setSampleBusy] = useState(false);
  const recent = useRecent(connections);
  const scripts = useRecentScripts(connections);
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
        {empty !== null && (
          <p className="welcome-lead">
            {empty
              ? t("A SQL client for your databases and data files. Add a connection to get started.")
              : t("Double-click a connection to open a SQL editor, or expand it to browse.")}
          </p>
        )}
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
        <button type="button" className="welcome-card" data-action="open-shared" onClick={() => void openSharedProject()}>
          <Icon name="open" size={18} />
          <span>
            <strong>{t("Open shared project…")}</strong>
            <span className="muted">{t("A project folder a colleague shared on a drive or in Git.")}</span>
          </span>
        </button>
        <button type="button" className="welcome-card" onClick={onExport}>
          <Icon name="upload" size={18} />
          <span>
            <strong>{t("Export connections")}</strong>
            <span className="muted">{t("To a file for another computer or a colleague.")}</span>
          </span>
        </button>
      </div>
      <div className="welcome-more">
        {(recent.length > 0 || scripts.length > 0) && (
          <section>
            <h2 className="welcome-heading">{t("Recent")}</h2>
            <ul className="welcome-list welcome-recent">
              {recent.map(({ connection, at }) => (
                <li key={connection.id}>
                  <button type="button" onClick={() => onOpenConnection(connection)}>
                    <DriverIcon driver={connection.driver} />
                    <span className="welcome-name">{connection.name}</span>
                    {typeBadge(connection) && (
                      <span className={`type-badge ${typeBadge(connection)!.toLowerCase()}`}>{typeBadge(connection)}</span>
                    )}
                    <span className="welcome-when">{ago(at)}</span>
                  </button>
                </li>
              ))}
              {scripts.map((script) => (
                <li key={script.path}>
                  <button
                    type="button"
                    title={script.path}
                    data-script={script.path}
                    onClick={() => openScriptFile(script.path, script.connectionId).catch((e: unknown) => onError(toCommandError(e).message))}
                  >
                    <Icon name="file" />
                    <span className="welcome-name">{script.path.split(/[/\\]/).pop()}</span>
                    <span className="welcome-when">{ago(script.at)}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}
        <section>
          <h2 className="welcome-heading">{t("Keyboard shortcuts")}</h2>
          <dl className="welcome-keys welcome-list">
            {shortcuts.map((s) => (
              <div key={s.id}>
                <dt>{s.title}</dt>
                <dd>
                  <kbd>{s.keys}</kbd>
                </dd>
              </div>
            ))}
          </dl>
        </section>
      </div>
    </div>
  );
}
