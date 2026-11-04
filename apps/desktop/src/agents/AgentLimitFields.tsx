import { useState } from "react";
import type { AgentLimits } from "../api";
import { msg, t } from "../i18n";

/** The limits a connection holds agents to, with the fixed cap shown for those that have one. */
const FIELDS: { key: keyof AgentLimits; label: string; cap?: string }[] = [
  { key: "max_rows", label: msg("Rows per result"), cap: "10000" },
  { key: "max_bytes", label: msg("Bytes per result"), cap: "1048576" },
  { key: "timeout_seconds", label: msg("Seconds per statement"), cap: "300" },
  { key: "calls_per_minute", label: msg("Calls per minute") },
  { key: "max_estimated_rows", label: msg("Estimated rows of a read") },
  { key: "max_plan_cost", label: msg("Plan cost of a read") },
];

/** The positive whole number `text` holds, `undefined` for an empty one (no limit), `null`
 *  for anything else. */
export function parseLimit(text: string): number | undefined | null {
  const trimmed = text.trim();
  if (trimmed === "") return undefined;
  const n = Number(trimmed);
  return /^[1-9]\d*$/.test(trimmed) && Number.isSafeInteger(n) ? n : null;
}

/** Number fields for the limits of a connection: empty means no limit beyond the fixed cap.
 *  A valid value is saved when the field loses focus or Enter is pressed. */
export function AgentLimitFields({
  limits,
  onChange,
}: {
  limits: AgentLimits;
  onChange: (key: keyof AgentLimits, value: number | undefined) => void;
}) {
  const [draft, setDraft] = useState<Record<string, string>>(() =>
    Object.fromEntries(FIELDS.map((f) => [f.key, limits[f.key]?.toString() ?? ""])),
  );
  const commit = (key: keyof AgentLimits) => {
    const value = parseLimit(draft[key] ?? "");
    if (value !== null && value !== limits[key]) onChange(key, value);
  };
  return (
    <fieldset className="agent-limits" data-field="agent_limits">
      <legend>{t("Limits for agents")}</legend>
      <p className="muted small">
        {t("Empty means no limit beyond the fixed cap. A limit only tightens: an agent that asks for more gets the connection's maximum.")}
      </p>
      {FIELDS.map((f) => {
        const invalid = parseLimit(draft[f.key] ?? "") === null;
        return (
          <label key={f.key} className="agent-limit">
            <span>{t(f.label)}</span>
            <input
              type="text"
              inputMode="numeric"
              data-limit={f.key}
              value={draft[f.key] ?? ""}
              placeholder={f.cap ?? t("no limit")}
              aria-invalid={invalid}
              onChange={(e) => setDraft({ ...draft, [f.key]: e.target.value })}
              onBlur={() => commit(f.key)}
              onKeyDown={(e) => {
                if (e.key === "Enter") commit(f.key);
              }}
            />
            {invalid && <span className="small agent-blocked">{t("A positive whole number, or empty")}</span>}
          </label>
        );
      })}
    </fieldset>
  );
}
