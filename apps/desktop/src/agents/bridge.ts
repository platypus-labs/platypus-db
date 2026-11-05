/**
 * The open app's side of the agents' bridge: the changes agents ask to run, waiting for the
 * user's answer, and the agents connected, from the bridge's events. The setting that has
 * agents ask first lives here too.
 */
import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import { api, type ConnectedAgent } from "../api";
import { msg } from "../i18n";
import { defineSetting } from "../settings";
import { type ContextRequest, type ContextVerdict, type Grants, type TabSnapshot, grant, granted, noGrants, readActive, verdictFor } from "./context";
import type { DiagramRequest } from "./diagram";
import { enqueue, settle, type Verdict, type Waiting, type WriteRequest } from "./writes";

/** Read by the `--mcp` server and the CLI (`Agent::confirms_changes`). */
export const confirmWritesSetting = defineSetting<boolean>({
  key: "agents.confirm_writes",
  section: "Safety",
  label: msg("Ask before agents change data"),
  description: msg(
    "On connections agents may change, each change waits for your answer in PlatypusDB, with its estimated rows and plan. Without the app open, agents' changes are refused.",
  ),
  default: true,
  control: { type: "boolean" },
});

interface State {
  waiting: Waiting[];
  /** The requests to see the user's work that wait for them. */
  contexts: Waiting<ContextRequest>[];
  agents: ConnectedAgent[];
}

let state: State = { waiting: [], contexts: [], agents: [] };
/** The agent sessions the user let see their work until they stop. It lives here, in the page:
 *  the content is read here each time, so a request of such a session is answered without
 *  the dialog. A reload forgets it, and the user is asked again. */
let grants: Grants = noGrants;
const listeners = new Set<() => void>();

function update(next: Partial<State>) {
  state = { ...state, ...next };
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The changes waiting for an answer, the oldest first. */
export function useWaitingWrites(): Waiting[] {
  return useSyncExternalStore(subscribe, () => state.waiting);
}

/** The requests to see the user's work waiting for an answer, the oldest first. */
export function useWaitingContexts(): Waiting<ContextRequest>[] {
  return useSyncExternalStore(subscribe, () => state.contexts);
}

/** The agents connected to the bridge. */
export function useConnectedAgents(): ConnectedAgent[] {
  return useSyncExternalStore(subscribe, () => state.agents);
}

/** Answers `id`; it leaves the queue once the answer reached the app, whether or not the
 *  agent still waited. A failure keeps it there, to answer again. */
export async function answerWrite(id: string, verdict: Verdict) {
  await api.answerAgentWrite(id, verdict);
  update({ waiting: settle(state.waiting, id) });
}

/** Answers the request `id` to see the user's work; it leaves the queue once the answer
 *  reached the app, as for a change. */
export async function answerContext(id: string, verdict: ContextVerdict) {
  await api.answerAgentContext(id, verdict);
  update({ contexts: settle(state.contexts, id) });
}

/** Shares `snapshot`, the tab as the dialog showed it, once or for the agent's session: not
 *  a second reading, which could differ from what the user agreed to. */
export async function shareWith(request: ContextRequest, how: "once" | "session", snapshot: TabSnapshot | null) {
  const verdict = verdictFor(request.parts, snapshot, how);
  await answerContext(request.id, verdict);
  if (verdict.answer === "session") grants = grant(grants, request.session);
}

/** A request from a session the user let see their work: shared at once, else it waits for
 *  the user (the dialog). */
function receiveContext(request: ContextRequest, since = Date.now()) {
  const wait = () => update({ contexts: enqueue(state.contexts, request, since) });
  if (!granted(grants, request.session)) return wait();
  // Read now (there is no dialog); a tab on a connection closed to agents is refused.
  readActive()
    .then((snapshot) => api.answerAgentContext(request.id, verdictFor(request.parts, snapshot, "session")))
    .catch(wait);
}

/** A request to draw an ER diagram as SVG: answered at once, with nothing shown. The drawing
 *  code loads here, at the first request. A failure to load or draw is the agent's answer. */
function receiveDiagram(request: DiagramRequest) {
  import("./diagram")
    .then((m) => m.answerDiagram(request))
    .catch((e: unknown) => api.answerAgentDiagram(request.id, { answer: "failed", message: String(e) }))
    .catch(() => {});
}

/** Follows the bridge's events, from the app's start. */
export function startAgentBridge() {
  void listen<WriteRequest>("agent-write-request", ({ payload }) =>
    update({ waiting: enqueue(state.waiting, payload, Date.now()) }),
  );
  void listen<string>("agent-write-settled", ({ payload }) => update({ waiting: settle(state.waiting, payload) }));
  void listen<ContextRequest>("agent-context-request", ({ payload }) => receiveContext(payload));
  void listen<string>("agent-context-settled", ({ payload }) => update({ contexts: settle(state.contexts, payload) }));
  void listen<DiagramRequest>("agent-diagram-request", ({ payload }) => receiveDiagram(payload));
  void listen<ConnectedAgent[]>("agents-connected", ({ payload }) => update({ agents: payload }));
  // What came before the page listened: a reload, or a request at the start.
  api.agentWritesWaiting().then(
    (waiting) =>
      update({
        waiting: waiting.reduce((queue, w) => enqueue(queue, w.request, Date.now() - w.waited_ms), state.waiting),
      }),
    () => {},
  );
  api.agentContextsWaiting().then(
    (waiting) => waiting.forEach((w) => receiveContext(w.request, Date.now() - w.waited_ms)),
    () => {},
  );
  api.connectedAgents().then((agents) => update({ agents }), () => {});
}
