/**
 * Edits made outside the app that give agents more than the user allowed: agent access in
 * `connections.toml`, or the Safety settings for agents in `settings.toml`, files a coding agent
 * with file tools can write. Agents do not get such an edit until the user accepts it here; the
 * app asks when it starts and when its window gets focus, once for each set of changes.
 */
import { api, toCommandError, type AgentPolicyChange } from "../api";
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
  }
}

/** The changes last asked about, so that the same ones are not asked at every focus. */
let asked = "";
let asking = false;

/** Asks about the edits that give agents more, if any; `reload` brings the connections
 *  and settings put back into the app. */
export async function checkAgentPolicy(reload: () => Promise<void>) {
  if (asking) return;
  asking = true;
  try {
    const changes = await api.agentPolicyChanges();
    const key = JSON.stringify(changes);
    if (changes.length === 0 || key === asked) {
      if (changes.length === 0) asked = "";
      return;
    }
    asked = key;
    const accept = await choose(
      t("What agents may do was changed outside PlatypusDB: {changes}. Agents keep what you allowed until you accept the change.", {
        changes: changes.map(describe).join("; "),
      }),
      [
        { label: t("Undo the change"), value: false, primary: true },
        { label: t("Accept the change"), value: true, danger: true },
      ],
    );
    // Cancel leaves agents what was allowed, and asks again when the changes do.
    if (accept === null) return;
    await api.answerAgentPolicy(accept);
    if (!accept) await reload();
  } catch (e) {
    workbench().showError(toCommandError(e).message);
  } finally {
    asking = false;
  }
}
