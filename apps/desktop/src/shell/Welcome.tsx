import { open } from "@tauri-apps/plugin-dialog";
import { shortcutLabel, useActions } from "../actions";
import { DATA_FILE_EXTENSIONS, toCommandError } from "../api";
import bannerOnDark from "../assets/brand/platypus-db-on-dark.png";
import bannerOnLight from "../assets/brand/platypus-db-on-light.png";
import { Icon } from "../ui/Icon";

interface Props {
  /** No connection saved yet: the first start. */
  empty: boolean;
  version: string;
  onNew: () => void;
  onImport: () => void;
  /** Opens a data file as a DuckDB database. */
  onOpenFile: (path: string) => void;
  onError: (message: string) => void;
}

/** The shell's actions worth knowing first, with their shortcuts as the user set them. */
const SHORTCUTS = ["app.palette", "tab.new", "tab.reopen", "app.settings"];

/** What the workspace shows with no tab open. */
export function Welcome({ empty, version, onNew, onImport, onOpenFile, onError }: Props) {
  async function pickFile() {
    const path = await open({ multiple: false, filters: [{ name: "Data files", extensions: DATA_FILE_EXTENSIONS }] });
    if (typeof path === "string") onOpenFile(path);
  }
  const global = useActions("global", undefined);
  const shortcuts = SHORTCUTS.flatMap((id) => {
    const action = global.find((a) => a.id === id);
    const keys = action && shortcutLabel(action);
    return action && keys ? [{ id, title: action.title, keys }] : [];
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
            ? "A SQL client for your databases and data files. Add a connection to get started."
            : "Double-click a connection to open a SQL editor, or expand it to browse."}
        </p>
        <button type="button" className="primary welcome-cta" onClick={onNew}>
          <Icon name="plus" size={15} />
          New connection
        </button>
        <p className="muted small">v{version}</p>
      </div>
      <div className="welcome-actions">
        <button type="button" className="welcome-card" onClick={() => pickFile().catch((e: unknown) => onError(toCommandError(e).message))}>
          <Icon name="file" size={18} />
          <span>
            <strong>Open a data file</strong>
            <span className="muted">CSV, Parquet, JSON or a spreadsheet, queried with DuckDB. You can also drop it on the window.</span>
          </span>
        </button>
        <button type="button" className="welcome-card" onClick={onImport}>
          <Icon name="import" size={18} />
          <span>
            <strong>Import connections</strong>
            <span className="muted">From DBeaver, DataGrip or pgAdmin.</span>
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
