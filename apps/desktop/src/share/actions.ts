// Sharing a query and opening what others shared: the actions, and the flow from a file or
// link to a query tab. Nothing here runs SQL: a shared query only ever opens for review.
import { createElement } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { registerAction } from "../actions";
import { api, toCommandError } from "../api";
import type { QueryTabModel } from "../components/QueryTab";
import { t } from "../i18n";
import { answerFor } from "../testAnswers";
import { workbench } from "../workbench";
import { OpenLinkDialog, OpenSharedDialog } from "./SharedDialogs";
import { ShareQueryDialog } from "./ShareQueryDialog";
import { matchTarget, prefillFromTarget } from "./shareMatch";
import { shareErrorMessage } from "./messages";
import { SHARE_EXTENSION, type SharedQuery, decodeFile, decodeLink, isShareLink } from "./shareQuery";
import type { SavedConnection } from "../api";

/** Opens a query tab with the shared SQL on `connection`: parameters filled in (and asked
 *  once at the first run, as a `${name}` value goes into the SQL as it is), title kept, a
 *  notice asking for a review, and nothing run. */
function openShared(query: SharedQuery, connection: SavedConnection) {
  const names = Object.keys(query.parameters);
  workbench().openTab("query", connection.id, {
    sql: query.sql,
    title: query.title || null,
    variables: names.length > 0 ? query.parameters : undefined,
    unconfirmedVariables: names.length > 0 ? names : undefined,
    shared: true,
  });
}

/** Starts a new connection from the descriptor (no credentials), and opens the query on it
 *  once the user saves it. */
async function newConnectionFor(query: SharedQuery) {
  try {
    const base = await api.defaultConnection(query.target.driver as SavedConnection["driver"]);
    workbench().newConnectionFrom(prefillFromTarget(base, query.target), (saved) => openShared(query, saved));
  } catch (e) {
    workbench().showError(toCommandError(e).message);
  }
}

/** Finds the connection of a shared query: the only exact match opens at once, anything else
 *  asks (a choice among the exact and likely ones, or a new connection). */
function openSharedQuery(query: SharedQuery) {
  const connections = workbench().connections();
  const { exact, candidates } = matchTarget(query.target, connections);
  const [only] = exact;
  if (only && exact.length === 1) return openShared(query, only);
  workbench().openDialog((close) =>
    createElement(OpenSharedDialog, {
      query,
      exact,
      candidates,
      onOpen: (connection: SavedConnection) => openShared(query, connection),
      onNew: () => void newConnectionFor(query),
      onClose: close,
    }),
  );
}

/** Opens what the OS or the user handed over: a link, or the path of a `.platypus` file. */
export async function openSharedItem(item: string) {
  try {
    const query = isShareLink(item) ? await decodeLink(item) : decodeFile(await api.readTextFile(item));
    openSharedQuery(query);
  } catch (e) {
    workbench().showError(shareErrorMessage(e));
  }
}

/** The links and files waiting in the backend (the app was started or reopened with them). */
export async function openPendingShares() {
  const items = await api.takePendingShares().catch(() => [] as string[]);
  for (const item of items) await openSharedItem(item);
}

/** Asks for a `.platypus` file and opens it. */
async function pickSharedFile() {
  const path = await answerFor("file", () =>
    open({ multiple: false, filters: [{ name: t("Shared queries"), extensions: [SHARE_EXTENSION] }] }),
  );
  if (typeof path === "string") await openSharedItem(path);
}

/** Shows the Share dialog of an SQL editor tab. */
function shareTab(tab: QueryTabModel) {
  const connection = workbench().connections().find((c) => c.id === tab.connectionId);
  if (!connection) return;
  workbench().openDialog((close) => createElement(ShareQueryDialog, { tab, connection, onClose: close }));
}

/** Registers the sharing actions; `activeQuery` is the SQL editor in view, if any. */
export function registerShareActions(activeQuery: () => QueryTabModel | null): () => void {
  const unregister = [
    registerAction({
      id: "tab.share",
      scope: "tab",
      title: "Share query…",
      group: "share",
      when: ({ tab }) => tab.kind === "query" && !tab.table,
      run: ({ tab }) => {
        if (tab.kind === "query") shareTab(tab);
      },
    }),
    registerAction({
      id: "share.query",
      scope: "global",
      title: "Share query…",
      when: () => activeQuery() !== null,
      run: () => {
        const tab = activeQuery();
        if (tab) shareTab(tab);
      },
    }),
    registerAction({ id: "share.open_file", scope: "global", title: "Open a shared query…", run: () => void pickSharedFile().catch((e: unknown) => workbench().showError(toCommandError(e).message)) }),
    registerAction({
      id: "share.open_link",
      scope: "global",
      title: "Open a shared link…",
      run: () => workbench().openDialog((close) => createElement(OpenLinkDialog, { onOpen: openSharedQuery, onClose: close })),
    }),
  ];
  return () => unregister.forEach((u) => u());
}
