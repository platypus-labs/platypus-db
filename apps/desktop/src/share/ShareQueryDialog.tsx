import { useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, type SavedConnection, toCommandError } from "../api";
import type { QueryTabModel } from "../components/QueryTab";
import { t } from "../i18n";
import { answerFor } from "../testAnswers";
import { basename } from "../workspace";
import { shareErrorMessage } from "./messages";
import { describeTarget } from "./shareMatch";
import { SHARE_EXTENSION, ShareError, type SharedQuery, encodeFile, encodeLink } from "./shareQuery";

/** The variables worth sharing: the answers the editor remembers for `?` placeholders are
 *  not named values. */
function namedVariables(tab: QueryTabModel): Record<string, string> {
  return Object.fromEntries(Object.entries(tab.variables ?? {}).filter(([k]) => !k.startsWith("?")));
}

/** The default title: the tab's, else its file's name. */
function defaultTitle(tab: QueryTabModel): string {
  if (tab.title) return tab.title;
  return tab.filePath ? basename(tab.filePath).replace(/\.[^.]+$/, "") : "";
}

/** A file name from a title: what file systems refuse is dropped. */
const fileName = (title: string) => `${title.trim().replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim() || "query"}.${SHARE_EXTENSION}`;

/** "Share…" of an SQL editor: a link to copy, or a `.platypus` file to save, with the SQL,
 *  a title, the values of the variables if the user wants them, and where it ran (never the
 *  credentials). */
export function ShareQueryDialog({ tab, connection, onClose }: { tab: QueryTabModel; connection: SavedConnection; onClose: () => void }) {
  const [title, setTitle] = useState(() => defaultTitle(tab));
  const [withParams, setWithParams] = useState(false);
  const variables = useMemo(() => namedVariables(tab), [tab]);
  const [link, setLink] = useState<{ ok: true; text: string } | { ok: false; reason: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const [message, setMessage] = useState<{ error: boolean; text: string } | null>(null);

  const empty = tab.sql.trim() === "";
  const query: SharedQuery = useMemo(
    () => ({
      title: title.trim(),
      sql: tab.sql,
      parameters: withParams ? variables : {},
      target: describeTarget(connection, tab.database),
    }),
    [title, tab.sql, tab.database, withParams, variables, connection],
  );

  // The link follows the choices; a query too big for one is told so, and the file offered.
  useEffect(() => {
    if (empty) return;
    let current = true;
    setCopied(false);
    encodeLink(query).then(
      (text) => current && setLink({ ok: true, text }),
      (e: unknown) => current && setLink({ ok: false, reason: e instanceof ShareError && e.code === "too_large" ? t("This query is too long for a link. Save it as a file instead.") : shareErrorMessage(e) }),
    );
    return () => {
      current = false;
    };
  }, [query, empty]);

  async function copy() {
    if (!link?.ok) return;
    try {
      await navigator.clipboard.writeText(link.text);
      setCopied(true);
      setMessage(null);
    } catch (e) {
      setMessage({ error: true, text: toCommandError(e).message });
    }
  }

  async function saveFile() {
    try {
      const path = await answerFor("file", () =>
        save({ defaultPath: fileName(title), filters: [{ name: t("Shared queries"), extensions: [SHARE_EXTENSION] }] }),
      );
      if (typeof path !== "string") return;
      await api.writeTextFile(path, encodeFile(query));
      setMessage({ error: false, text: t("Saved {path}", { path }) });
    } catch (e) {
      setMessage({ error: true, text: toCommandError(e).message });
    }
  }

  const hasVariables = Object.keys(variables).length > 0;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal share-dialog" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Share query")}</h2>
        {empty && <div className="notice warn small">{t("The editor is empty: there is nothing to share.")}</div>}
        <label className="field">
          <span>{t("Title")}</span>
          <input autoFocus aria-label={t("Title")} value={title} spellCheck={false} onChange={(e) => setTitle(e.target.value)} />
        </label>
        <p className="muted small" data-field="share_target">
          {t("Runs on {name} ({driver}). The connection's user and password are not shared; whoever opens the query picks or creates their own connection.", {
            name: connection.name,
            driver: connection.driver,
          })}
        </p>
        {hasVariables && (
          <label className="check">
            <input type="checkbox" checked={withParams} onChange={(e) => setWithParams(e.target.checked)} />
            <span>{t("Include the values of the variables ({n})", { n: Object.keys(variables).length })}</span>
          </label>
        )}
        {withParams && <div className="notice warn small">{t("The values are written in the link or file in plain text. Do not share secrets.")}</div>}
        {!empty && link?.ok && <textarea className="share-link" readOnly aria-label={t("Link")} data-field="share_link" rows={3} value={link.text} onFocus={(e) => e.target.select()} />}
        {!empty && link && !link.ok && <div className="notice small" data-field="share_link_note">{link.reason}</div>}
        {message && <div className={`notice small ${message.error ? "error" : "ok"}`} data-field="share_message">{message.text}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Close")}
          </button>
          <button type="button" data-action="save-file" disabled={empty} onClick={() => void saveFile()}>
            {t("Save as .platypus…")}
          </button>
          <button type="button" className="primary" data-action="copy-link" disabled={empty || !link?.ok} onClick={() => void copy()}>
            {t(copied ? "Copied" : "Copy link")}
          </button>
        </div>
      </div>
    </div>
  );
}
