// A plugin's HTML document in its frame, and the only way out of it: messages, read and answered
// here. The document is untrusted, and all that keeps it apart from the app is below:
// - `sandbox="allow-scripts"`, never `allow-same-origin`: an opaque origin, with no access to the
//   app's DOM, globals or storage, and no popups, top navigation, forms or dialogs. The scheme
//   that serves it says the same in its own policy, with no network (see `frames.rs`).
// - a message counts only if its source is this frame's window and its origin the opaque one, and
//   nothing else of it is read before that. What it says goes through `frameProtocol.ts`, then
//   the backend. Text ends up in a toast or a tab's editor, never in `innerHTML`.
// - a frame that leaves its document (a second `load`), stops answering pings, floods, or sends
//   what no page of the app would send is removed, and the user is told.
import { useEffect, useRef, useState } from "react";
import { api, type InstalledPlugin, type SavedConnection, toCommandError } from "../api";
import { language, msg, t } from "../i18n";
import { THEME_EVENT } from "../theme";
import { copyText } from "../ui/copy";
import { ErrorState } from "../ui/State";
import { workbench } from "../workbench";
import { FrameGuard, type FrameRequestName, MAX_IN_FLIGHT, hung, readFrameMessage, refusedBy } from "./frameProtocol";

/** How often a frame is asked if it is alive. */
const PING_MS = 2000;
/** How long a document may take to load. */
const LOAD_MS = 20_000;
/** The colours and fonts a page is told, to look like the app. */
const THEME_VARS = ["--bg", "--bg-sidebar", "--fg", "--fg-muted", "--accent", "--border", "--font-ui", "--font-mono"];

interface Props {
  plugin: InstalledPlugin;
  /** The document, a path inside the package. */
  document: string;
  title: string;
  /** The tab's connection, or the active tab's for a panel: where a query opens. Told to the
   *  page by name, driver and database, never its host, user or password. */
  connection: SavedConnection | null;
}

function theme() {
  const style = getComputedStyle(document.documentElement);
  return Object.fromEntries(THEME_VARS.map((name) => [name, style.getPropertyValue(name).trim()]));
}

export function PluginFrame({ plugin, document: entry, title, connection }: Props) {
  const box = useRef<HTMLDivElement>(null);
  // Read when a request needs it: changing tab does not reload the page.
  const connectionRef = useRef(connection);
  connectionRef.current = connection;
  const approved = useRef(plugin.approved);
  approved.current = plugin.approved;
  const [stopped, setStopped] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const id = plugin.manifest.id;
  const version = plugin.manifest.version;
  const name = plugin.manifest.name;

  useEffect(() => {
    const host = box.current;
    if (!host) return;
    let frame: HTMLIFrameElement | null = null;
    let over = false;
    let pinged = 0;
    let answered = 0;
    let inFlight = 0;
    let loads = 0;
    let watchdog = 0;
    let loading = 0;
    const guard = new FrameGuard(performance.now());

    const teardown = () => {
      over = true;
      window.removeEventListener("message", onMessage);
      window.removeEventListener(THEME_EVENT, onTheme);
      clearInterval(watchdog);
      clearTimeout(loading);
      frame?.remove();
      frame = null;
    };
    const stop = (reason: string) => {
      teardown();
      setStopped(reason);
    };
    // Its origin is opaque: no other target can be named. A navigation away is seen at its `load`.
    const post = (message: object) => frame?.contentWindow?.postMessage({ platypus: 1, ...message }, "*");
    const event = (event: string, data: unknown) => post({ type: "event", event, data: JSON.stringify(data) });

    /** Does what a request asks of the page, once the backend said yes; answers for the frame. */
    async function serve(request: FrameRequestName, params: unknown): Promise<unknown> {
      const refused = refusedBy(request, approved.current);
      if (refused) throw new Error(refused);
      if ((request === "notify" || request === "open_query") && !guard.noisy(performance.now())) {
        throw new Error("too many requests of this kind");
      }
      const answer = await api.pluginFrameRequest(id, { name: request, params });
      const text = (answer as { text?: string } | null)?.text ?? "";
      switch (request) {
        case "notify":
          workbench().notify(`${name}: ${text}`);
          return null;
        case "clipboard":
          await copyText(text);
          return null;
        case "open_query": {
          const on = connectionRef.current;
          if (!on) throw new Error("no connection is open");
          workbench().openTab("query", on.id, { sql: text });
          return null;
        }
        default:
          return answer;
      }
    }

    function onMessage(e: MessageEvent) {
      // Nothing else of a message is looked at until it is known to be this frame's.
      if (!frame || e.source !== frame.contentWindow) return;
      if (e.origin !== "null") return stop(msg("The page is not isolated as it should be."));
      const admission = guard.admit(performance.now());
      if (admission === "stop") return stop(msg("The page sent too many messages."));
      if (admission === "drop") return;
      const message = readFrameMessage(e.data);
      switch (message.kind) {
        case "broken":
          if (guard.strike()) stop(msg("The page sent messages the app does not understand."));
          return;
        case "pong":
          answered = Math.max(answered, Math.min(message.n, pinged));
          return;
        case "refused":
          post({ type: "answer", id: message.id, error: message.reason });
          if (message.strike && guard.strike()) stop(msg("The page sent messages the app does not understand."));
          return;
        case "request":
          if (inFlight >= MAX_IN_FLIGHT) {
            post({ type: "answer", id: message.id, error: "too many requests at once" });
            return;
          }
          inFlight += 1;
          serve(message.name, message.params).then(
            (value) => {
              inFlight -= 1;
              if (!over) post({ type: "answer", id: message.id, value: JSON.stringify(value ?? null) });
            },
            (e: unknown) => {
              inFlight -= 1;
              if (!over) post({ type: "answer", id: message.id, error: e instanceof Error ? e.message : toCommandError(e).message });
            },
          );
          return;
      }
    }
    const onTheme = () => event("theme", theme());

    function mount(url: string) {
      const element = document.createElement("iframe");
      // Before `src`: the flags in force are those when it navigates.
      element.setAttribute("sandbox", "allow-scripts");
      element.setAttribute("referrerpolicy", "no-referrer");
      element.setAttribute("allow", "");
      element.className = "plugin-frame";
      element.title = title;
      element.dataset.plugin = id;
      element.addEventListener("load", () => {
        if (over) return;
        loads += 1;
        // Its own document is the only one it gets: any other load is the page leaving it.
        if (loads > 1) return stop(msg("The page tried to leave its document."));
        clearTimeout(loading);
        const on = connectionRef.current;
        event("init", {
          plugin: id,
          language: language(),
          theme: theme(),
          connection: on && { name: on.name, driver: on.driver, database: on.database },
        });
        watchdog = window.setInterval(() => {
          // A hidden window may be asleep, the frame with it.
          if (document.hidden) {
            answered = pinged;
            return;
          }
          if (hung(pinged, answered)) return stop(msg("The page stopped responding."));
          pinged += 1;
          post({ type: "ping", n: pinged });
        }, PING_MS);
      });
      frame = element;
      window.addEventListener("message", onMessage);
      window.addEventListener(THEME_EVENT, onTheme);
      loading = window.setTimeout(() => stop(msg("The page did not load.")), LOAD_MS);
      element.src = url;
      host?.append(element);
    }

    setStopped(null);
    api.pluginFrameUrl(id, entry).then(
      (url) => {
        if (!over) mount(url);
      },
      (e: unknown) => {
        if (!over) setStopped(toCommandError(e).message);
      },
    );
    return teardown;
    // A new version of the plugin loads its page again; other changes to it do not.
  }, [id, version, entry, attempt, title, name]);

  return (
    <div className="plugin-frame-box" ref={box} data-field="plugin-frame" data-stopped={stopped !== null || undefined}>
      {stopped !== null && (
        <ErrorState
          title={t("The page of “{name}” was stopped", { name })}
          actions={
            <button type="button" data-action="reload-frame" onClick={() => setAttempt((n) => n + 1)}>
              {t("Load it again")}
            </button>
          }
        >
          {t(stopped)}
        </ErrorState>
      )}
    </div>
  );
}
