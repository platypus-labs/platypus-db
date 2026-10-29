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
  agents: ConnectedAgent[];
}

let state: State = { waiting: [], agents: [] };
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

/** Follows the bridge's events, from the app's start. */
export function startAgentBridge() {
  void listen<WriteRequest>("agent-write-request", ({ payload }) =>
    update({ waiting: enqueue(state.waiting, payload, Date.now()) }),
  );
  void listen<string>("agent-write-settled", ({ payload }) => update({ waiting: settle(state.waiting, payload) }));
  void listen<ConnectedAgent[]>("agents-connected", ({ payload }) => update({ agents: payload }));
  // What came before the page listened: a reload, or a request at the start.
  api.agentWritesWaiting().then(
    (waiting) =>
      update({
        waiting: waiting.reduce((queue, w) => enqueue(queue, w.request, Date.now() - w.waited_ms), state.waiting),
      }),
    () => {},
  );
  api.connectedAgents().then((agents) => update({ agents }), () => {});
}
