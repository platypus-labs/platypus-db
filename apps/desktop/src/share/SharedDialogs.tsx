import { useState } from "react";
import { api, type SavedConnection, toCommandError } from "../api";
import { t } from "../i18n";
import { shareErrorMessage } from "./messages";
import { type SharedQuery, decodeFile, decodeLink, isShareLink } from "./shareQuery";

/** The first lines of the SQL, enough to recognize the query. */
const PREVIEW_LINES = 8;

const preview = (sql: string) => {
  const lines = sql.split("\n");
  return lines.length > PREVIEW_LINES ? `${lines.slice(0, PREVIEW_LINES).join("\n")}\n…` : sql;
};

/** Where the query ran, for the receiver to compare with their connections. */
const place = (q: SharedQuery) => {
  const { driver, host, port, database } = q.target;
  return [driver, host && (port ? `${host}:${port}` : host), database].filter(Boolean).join(" · ");
};

/** Which saved connection a shared query opens on: the ones that are the sender's database
 *  first, then those of the same driver, or a new connection made from the descriptor. */
export function OpenSharedDialog({
  query,
  exact,
  candidates,
  onOpen,
  onNew,
  onClose,
}: {
  query: SharedQuery;
  exact: SavedConnection[];
  candidates: SavedConnection[];
  onOpen: (connection: SavedConnection) => void;
  onNew: () => void;
  onClose: () => void;
}) {
  const all = [...exact, ...candidates];
  const [chosen, setChosen] = useState<string | null>(exact[0]?.id ?? null);
  const row = (c: SavedConnection) => (
    <label className="share-choice" key={c.id} data-id={c.id}>
      <input type="radio" name="share-connection" checked={chosen === c.id} onChange={() => setChosen(c.id)} />
      <span>
        <strong>{c.name}</strong>
        <span className="muted small">{[c.host && (c.port ? `${c.host}:${c.port}` : c.host), c.database].filter(Boolean).join(" · ")}</span>
      </span>
    </label>
  );
  const picked = all.find((c) => c.id === chosen);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal share-open"
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          if (picked) {
            onOpen(picked);
            onClose();
          }
        }}
      >
        <h2>{query.title || t("Shared query")}</h2>
        <p className="muted small" data-field="share_origin">
          {t("Made for {place}", { place: place(query) })}
        </p>
        <pre className="sql-preview" data-field="share_sql">
          {preview(query.sql)}
        </pre>
        {Object.keys(query.parameters).length > 0 && (
          <p className="muted small">{t("Comes with the values of {n} variables.", { n: Object.keys(query.parameters).length })}</p>
        )}
        <div className="notice warn small">{t("Shared query: review before running.")}</div>
        {exact.length > 0 && (
          <fieldset className="share-choices" data-group="exact">
            <legend>{t("Connections to the same database")}</legend>
            {exact.map(row)}
          </fieldset>
        )}
        {candidates.length > 0 && (
          <fieldset className="share-choices" data-group="candidates">
            <legend>{exact.length > 0 ? t("Other connections of the same type") : t("Connections of the same type")}</legend>
            {candidates.map(row)}
          </fieldset>
        )}
        {all.length === 0 && <p className="muted small">{t("No saved connection is of this type. Create one from the details in the file.")}</p>}
        <div className="actions">
          <button type="button" className="share-new" onClick={() => {
            onNew();
            onClose();
          }}>
            {t("New connection from this…")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" disabled={!picked}>
            {t("Open")}
          </button>
        </div>
      </form>
    </div>
  );
}

/** "Open a shared link…": a link (or the text of a `.platypus` file) pasted in a box. */
export function OpenLinkDialog({ onOpen, onClose }: { onOpen: (query: SharedQuery) => void; onClose: () => void }) {
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function read() {
    const trimmed = text.trim();
    try {
      const query = isShareLink(trimmed) || !trimmed.startsWith("{") ? await decodeLink(trimmed) : decodeFile(trimmed);
      onClose();
      onOpen(query);
    } catch (e) {
      setError(shareErrorMessage(e));
    }
  }

  async function paste() {
    try {
      setText(await api.readClipboard());
      setError(null);
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal share-link-dialog"
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          void read();
        }}
      >
        <h2>{t("Open a shared link")}</h2>
        <label className="field">
          <span>{t("Link")}</span>
          <textarea
            autoFocus
            rows={4}
            spellCheck={false}
            placeholder="platypus://query?d=…"
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setError(null);
            }}
          />
        </label>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <button type="button" onClick={() => void paste()}>
            {t("Paste")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" disabled={!text.trim()}>
            {t("Open")}
          </button>
        </div>
      </form>
    </div>
  );
}
