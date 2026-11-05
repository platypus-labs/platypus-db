/**
 * Edits made outside the app that give agents more than the user allowed: agent access in
 * `connections.toml`, the Safety settings for agents and the rules that mask in `settings.toml`,
 * the sensitive marks in `catalog.db`, files a coding agent with file tools can write. Agents do
 * not get such an edit until the user accepts it here; the app asks when it starts and when its
 * window gets focus, once for each set of changes, and Settings › Agents › Security lists them
 * until they are answered.
 */
import { api, toCommandError, type AgentPolicyChange } from "../api";
import { forgetCatalog } from "../catalog/catalog";
import { t } from "../i18n";
import { choose } from "../ui/ask";
import { workbench } from "../workbench";
import { agentAccessLabel } from "./access";

/** One change, for the question. */
function describe(c: AgentPolicyChange): string {
  switch (c.kind) {
    case "access":
      return t("{name}: agent access {from} → {to}", { name: c.name, from: agentAccessLabel(c.from), to: agentAccessLabel(c.to) });
    case "confirm_writes":
      return t("agents' changes run without asking");
    case "unfiltered":
      return t("agents may run UPDATE or DELETE without WHERE");
    case "values_shown":
      return t("{name}: agents read the values of sensitive columns", { name: c.name });
    case "agent_limits":
      return t("{name}: the limits on agents were raised or removed", { name: c.name });
    case "masking_rules":
      return c.by === "name"
        ? t("the rules on sensitive names no longer have: {patterns}", { patterns: c.patterns.join(", ") })
        : t("the rules on sensitive types no longer have: {patterns}", { patterns: c.patterns.join(", ") });
    case "sensitive_marks":
      return t("{name}: sensitive marks removed or lifted ({count}): {columns}", {
        name: c.name,
        count: c.count,
        columns: c.count > c.columns.length ? `${c.columns.join(", ")}, …` : c.columns.join(", "),
      });
    case "masking_lost":
      return t("the record of what you allowed no longer matches its seal, so every value reads masked for agents until you answer");
  }
}

/** The question about `changes`, here and in Settings › Agents › Security. */
export function agentPolicyQuestion(changes: AgentPolicyChange[]): string {
  return t("What agents may do was changed outside PlatypusDB: {changes}. Agents keep what you allowed until you accept the change.", {
    changes: changes.map(describe).join("; "),
  });
}

const answered = new Set<() => void>();

/** Calls `listener` after each answer to the question, wherever it was given. */
export function onAgentPolicyAnswered(listener: () => void): () => void {
  answered.add(listener);
  return () => answered.delete(listener);
}

/** Gives the user's answer about `changes`; `reload` brings the files, as accepted or as put
 *  back, into the app. */
export async function answerAgentPolicy(accept: boolean, changes: AgentPolicyChange[], reload: () => Promise<void>) {
  await api.answerAgentPolicy(accept);
  asked = "";
  for (const c of changes) if (c.kind === "sensitive_marks") forgetCatalog(c.id);
  await reload();
  answered.forEach((listener) => listener());
}

/** The changes last asked about, so that the same ones are not asked at every focus. */
let asked = "";
let asking = false;

/** Asks about the edits that give agents more, if any; `reload` brings the connections
 *  and settings put back into the app. Without `marks` the sensitive marks are not read, as at
 *  the start of the app; a question, when there is one, is still about all of it. */
export async function checkAgentPolicy(reload: () => Promise<void>, marks = true) {
  if (asking) return;
  asking = true;
  try {
    let changes = await api.agentPolicyChanges(marks);
    if (!marks && changes.length > 0) changes = await api.agentPolicyChanges(true);
    const key = JSON.stringify(changes);
    if (changes.length === 0 || key === asked) {
      if (changes.length === 0 && marks) asked = "";
      return;
    }
    asked = key;
    const accept = await choose(agentPolicyQuestion(changes), [
      { label: t("Undo the change"), value: false, primary: true },
      { label: t("Accept the change"), value: true, danger: true },
    ]);
    // Cancel leaves agents what was allowed, and asks again when the changes do.
    if (accept === null) return;
    await answerAgentPolicy(accept, changes, reload);
  } catch (e) {
    workbench().showError(toCommandError(e).message);
  } finally {
    asking = false;
  }
}
