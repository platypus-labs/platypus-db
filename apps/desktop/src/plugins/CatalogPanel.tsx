import { useCallback, useEffect, useRef, useState } from "react";
import { api, type CatalogEntry, type CatalogItem, type CatalogView, type PreparedPlugin, type TrustedPluginKey, toCommandError } from "../api";
import { msg, t } from "../i18n";
import { ask } from "../ui/ask";
import { EmptyState } from "../ui/State";
import { PluginReview } from "./PluginsPanel";
import { reloadPlugins } from "./store";
import { entryAction } from "./catalogEntry";

const STATUS_BUTTON: Record<CatalogItem["status"], string> = {
  new: msg("Install"),
  update: msg("Update"),
  current: msg("Install again"),
  older: msg("Install this older version"),
};

/** The address of the index, saved when asked. */
function IndexUrl({ url, busy, onSave }: { url: string; busy: boolean; onSave: (url: string) => void }) {
  const [text, setText] = useState(url);
  useEffect(() => setText(url), [url]);
  return (
    <div className="setting" data-field="catalog-url">
      <span>
        <span className="setting-title">{t("Index address")}</span>
        <span className="muted small">{t("HTTPS only, or an address on this computer. PlatypusDB contacts it when you open this page or ask, and at no other time; it has no address of its own.")}</span>
      </span>
      <input type="url" spellCheck={false} placeholder="https://" value={text} onChange={(e) => setText(e.target.value)} />
      <button type="button" data-action="save-url" disabled={busy} onClick={() => onSave(text)}>
        {t("Save and read")}
      </button>
    </div>
  );
}

/** The public keys the user trusts for packages. */
function TrustedKeys({ onError }: { onError: (message: string | null) => void }) {
  const [keys, setKeys] = useState<TrustedPluginKey[]>([]);
  const [name, setName] = useState("");
  const [key, setKey] = useState("");

  useEffect(() => {
    api.pluginCatalogKeys().then(setKeys, (e: unknown) => onError(toCommandError(e).message));
  }, [onError]);

  async function trust() {
    onError(null);
    try {
      setKeys(await api.pluginCatalogTrust(name, key));
      setName("");
      setKey("");
    } catch (e) {
      onError(toCommandError(e).message);
    }
  }

  async function untrust(old: TrustedPluginKey) {
    onError(null);
    try {
      setKeys(await api.pluginCatalogUntrust(old.key));
    } catch (e) {
      onError(toCommandError(e).message);
    }
  }

  return (
    <div className="driver-block" data-field="trusted-keys">
      <strong>{t("Keys you trust")}</strong>
      <p className="muted small">{t("A package from the catalog must be signed by one of these minisign public keys. Trust a key only if you got it from its author.")}</p>
      {keys.map((k) => (
        <div key={k.key} className="driver-row-inner" data-key-name={k.name}>
          <span>{k.name}</span>
          <span className="spacer" />
          <button type="button" data-action="untrust" onClick={() => void untrust(k)}>
            {t("Remove")}
          </button>
        </div>
      ))}
      <div className="driver-row-inner">
        <input type="text" data-field="key-name" placeholder={t("Name")} value={name} onChange={(e) => setName(e.target.value)} />
        <input type="text" data-field="key-text" spellCheck={false} placeholder={t("Public key")} value={key} onChange={(e) => setKey(e.target.value)} />
        <button type="button" data-action="trust" disabled={key.trim() === ""} onClick={() => void trust()}>
          {t("Trust this key")}
        </button>
      </div>
    </div>
  );
}

/** The separate question before a package with no valid signature. */
function UnsignedConfirm(props: { entry: CatalogEntry; reason: string; busy: boolean; onInstall: () => void; onCancel: () => void }) {
  return (
    <div className="notice warn small" data-field="unsigned-confirm" data-plugin={props.entry.id}>
      <p>
        <strong>{t("{name} has no valid signature.", { name: props.entry.name })}</strong> {props.reason}
      </p>
      <p>{t("Nothing shows who made this package or that it is what they published. Install it only if you know where it comes from.")}</p>
      <div className="driver-actions">
        <button type="button" data-action="cancel-unsigned" disabled={props.busy} onClick={props.onCancel}>
          {t("Cancel")}
        </button>
        <button type="button" className="danger" data-action="install-unsigned" disabled={props.busy} onClick={props.onInstall}>
          {t("Install as unsigned")}
        </button>
      </div>
    </div>
  );
}

/** Settings › Plugin catalog, a chunk of its own: the plugins an index lists, read when the page
 *  opens or the user asks. Nothing installs without a click, and an install goes through the review
 *  of a manual one. */
export default function CatalogPanel() {
  const [url, setUrl] = useState<string | null>(null);
  const [view, setView] = useState<CatalogView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [unsigned, setUnsigned] = useState<{ item: CatalogItem; reason: string } | null>(null);
  const [review, setReview] = useState<{ prepared: PreparedPlugin; item: CatalogItem } | null>(null);
  // The download waiting for an answer, forgotten if the page closes first.
  const waiting = useRef<string | null>(null);

  const read = useCallback(async (address: string) => {
    setError(null);
    setUnsigned(null);
    if (address === "") {
      setView(null);
      return;
    }
    setBusy(true);
    try {
      setView(await api.pluginCatalogFetch());
    } catch (e) {
      setView(null);
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    api.pluginCatalogUrl().then(
      (saved) => {
        setUrl(saved);
        void read(saved);
      },
      (e: unknown) => {
        setUrl("");
        setError(toCommandError(e).message);
      },
    );
    return () => {
      if (waiting.current) void api.pluginCatalogDiscard(waiting.current);
    };
  }, [read]);

  function settleWaiting(ticket: string | null) {
    if (waiting.current && waiting.current !== ticket) void api.pluginCatalogDiscard(waiting.current);
    waiting.current = ticket;
  }

  async function saveUrl(text: string) {
    setError(null);
    try {
      const saved = await api.pluginCatalogSetUrl(text);
      setUrl(saved);
      await read(saved);
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  async function prepare(item: CatalogItem, options: { unsigned?: boolean; downgrade?: boolean }) {
    setError(null);
    setUnsigned(null);
    setReview(null);
    settleWaiting(null);
    setBusy(true);
    try {
      const prepared = await api.pluginCatalogPrepare(item.entry, options.unsigned ?? false, options.downgrade ?? false);
      settleWaiting(prepared.ticket);
      setReview({ prepared, item });
    } catch (e) {
      const failure = toCommandError(e);
      if (failure.kind === "plugin_unsigned") setUnsigned({ item, reason: failure.message });
      else setError(failure.message);
    } finally {
      setBusy(false);
    }
  }

  async function start(item: CatalogItem) {
    const action = entryAction(item);
    if (action.confirmDowngrade) {
      const confirmed = await ask(t("Install version {offered} of {name} over the newer {installed}?", { offered: item.entry.version, name: item.entry.name, installed: item.installed ?? "" }), { confirm: t("Install this older version"), danger: true });
      if (!confirmed) return;
    }
    await prepare(item, { downgrade: action.confirmDowngrade });
  }

  async function install() {
    if (!review) return;
    const { prepared, item } = review;
    setError(null);
    setBusy(true);
    try {
      await api.pluginCatalogInstall(prepared.ticket, prepared.inspected.hash);
      waiting.current = null;
      setReview(null);
      await reloadPlugins();
      await read(url ?? "");
    } catch (e) {
      waiting.current = null;
      setReview(null);
      setError(t("{name} was not installed: {reason}", { name: item.entry.name, reason: toCommandError(e).message }));
    } finally {
      setBusy(false);
    }
  }

  function cancelReview() {
    settleWaiting(null);
    setReview(null);
  }

  if (url === null) return null;
  return (
    <div className="plugins-panel catalog-panel">
      {error && (
        <div className="notice error small" data-field="catalog-error">
          {error}
        </div>
      )}
      <IndexUrl url={url} busy={busy} onSave={(text) => void saveUrl(text)} />
      <TrustedKeys onError={setError} />
      {url === "" && <EmptyState icon="extension" title={t("No index set.")}>{t("Set the address of an index to see the plugins it lists.")}</EmptyState>}
      {url !== "" && (
        <div className="driver-actions">
          <button type="button" data-action="refresh" disabled={busy} onClick={() => void read(url)}>
            {t("Read the index again")}
          </button>
        </div>
      )}
      {unsigned && (
        <UnsignedConfirm
          entry={unsigned.item.entry}
          reason={unsigned.reason}
          busy={busy}
          onCancel={() => setUnsigned(null)}
          onInstall={() => void prepare(unsigned.item, { unsigned: true, downgrade: entryAction(unsigned.item).confirmDowngrade })}
        />
      )}
      {review && (
        <PluginReview plugin={review.prepared.inspected} busy={busy} onInstall={() => void install()} onCancel={cancelReview}>
          {review.prepared.signature.state === "verified" ? (
            <p className="small" data-field="signature" data-state="verified">
              {t("Signed by a key you trust: {key}.", { key: review.prepared.signature.key })}
            </p>
          ) : (
            <p className="notice warn small" data-field="signature" data-state="unsigned">
              {t("Not signed: you chose to install it anyway. {reason}", { reason: review.prepared.signature.reason })}
            </p>
          )}
        </PluginReview>
      )}
      {view?.items.length === 0 && view.problems.length === 0 && <p className="muted small" data-field="catalog-empty">{t("The index lists no plugins.")}</p>}
      {view?.items.map((item) => (
        <div key={item.entry.id} className="driver-block" data-catalog-entry={item.entry.id} data-status={item.status}>
          <div className="driver-row-inner">
            <div>
              <strong>{item.entry.name}</strong> · <span data-field="version">{item.entry.version}</span>
              {item.entry.author && <div className="muted small">{item.entry.author}</div>}
            </div>
            <span className="spacer" />
            {item.installed && <span className="muted small" data-field="installed">{t("Installed: {version}", { version: item.installed })}</span>}
            <button type="button" data-action="install-entry" className={item.status === "new" || item.status === "update" ? "primary" : undefined} disabled={busy} onClick={() => void start(item)}>
              {t(STATUS_BUTTON[item.status])}
            </button>
          </div>
          {item.entry.description && <p className="muted small">{item.entry.description}</p>}
        </div>
      ))}
      {(view?.problems.length ?? 0) > 0 && (
        <div className="notice warn small" data-field="catalog-problems">
          <p>{t("Left out of the list:")}</p>
          <ul>
            {view!.problems.map((p, i) => (
              <li key={i}>
                {p.id ?? "?"}: {p.reason}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
