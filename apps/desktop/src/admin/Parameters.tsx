/** The admin tab's list of the server's configuration parameters: a search, a filter for those
 *  that differ from their defaults, and for each the statement that sets it, which opens in
 *  the editor and is never run from here. */
import { useState } from "react";
import type { ServerParameter } from "../api";
import { formatCount, msg, plural, t } from "../i18n";
import { changedCount, shownParameters, valueText } from "./parametersModel";
import { EmptyState } from "../ui/State";
import { AdminSplit } from "./AdminSplit";

/** Rows drawn at once: ClickHouse has thousands of settings. */
const LIMIT = 500;

/** What each context the servers write means, for the title of its cell. */
const CONTEXTS: Record<string, string> = {
  internal: msg("Fixed when the server was built or started."),
  postmaster: msg("Changes after a restart of the server."),
  sighup: msg("Changes when the server reloads its configuration."),
  superuser: msg("A superuser can change it for a session."),
  user: msg("Any user can change it for a session."),
  "read only": msg("Read only: it changes in the configuration, with a restart."),
  dynamic: msg("It can change while the server runs."),
  restart: msg("Changes after a restart of the server."),
};

const contextText = (context: string | null) => (context && CONTEXTS[context] ? t(CONTEXTS[context]) : undefined);

const keyOf = (p: ServerParameter) => `${p.name}\u0000${p.source ?? ""}`;

export function Parameters({
  parameters,
  onOpen,
}: {
  parameters: ServerParameter[];
  onOpen: (sql: string) => void;
}) {
  const [filter, setFilter] = useState("");
  const [onlyChanged, setOnlyChanged] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const matching = shownParameters(parameters, filter, onlyChanged);
  const shown = matching.slice(0, LIMIT);
  const chosen = parameters.find((p) => keyOf(p) === selected) ?? null;
  return (
    <>
      <div className="admin-toolbar">
        <label className="check small">
          <input type="checkbox" checked={onlyChanged} onChange={(e) => setOnlyChanged(e.target.checked)} />
          {t("Only changed ({n})", { n: formatCount(changedCount(parameters)) })}
        </label>
        <span className="muted small" data-field="parameter-count">
          {plural(matching.length, "{n} parameter", "{n} parameters")}
        </span>
        <span className="spacer" />
        <input
          type="search"
          placeholder={t("Search by name, value or description")}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>
      <AdminSplit
        list={
        <div className="admin-list">
          <table className="meta-table admin-table admin-parameters">
            <thead>
              <tr>
                <th>{t("Parameter")}</th>
                <th>{t("Value")}</th>
                <th>{t("Default")}</th>
                <th>{t("Source")}</th>
                <th>{t("Context")}</th>
                <th>{t("Description")}</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((p) => (
                <tr
                  key={keyOf(p)}
                  data-name={p.name}
                  className={`admin-session${keyOf(p) === selected ? " selected" : ""}${p.changed ? " changed" : ""}`}
                  onClick={() => setSelected(keyOf(p))}
                >
                  <td className="admin-param-name">
                    {p.name}
                    {p.pending_restart && (
                      <span className="admin-own" title={t("A change waits for a restart or a reload to take effect.")}>
                        {t("pending")}
                      </span>
                    )}
                  </td>
                  <td className="admin-param-value" title={valueText(p.value, p.unit)}>
                    {valueText(p.value, p.unit)}
                  </td>
                  <td className="admin-param-default">{valueText(p.default, p.unit)}</td>
                  <td>{p.source}</td>
                  <td title={contextText(p.context)}>{p.context}</td>
                  <td className="admin-query" title={p.description ?? undefined}>
                    {p.description}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {matching.length > shown.length && (
            <EmptyState compact className="admin-empty" title={t("{n} more: narrow the search to see them.", { n: formatCount(matching.length - shown.length) })} />
          )}
          {matching.length === 0 && <EmptyState compact className="admin-empty" title={t("No parameters match.")} />}
        </div>
        }
        details={<ParameterDetails parameter={chosen} onOpen={onOpen} />}
      />
    </>
  );
}

function ParameterDetails({ parameter: p, onOpen }: { parameter: ServerParameter | null; onOpen: (sql: string) => void }) {
  if (!p) {
    return (
      <aside className="admin-details empty">
        <p className="muted small">{t("Select a parameter to see its details.")}</p>
      </aside>
    );
  }
  const limits = [p.min, p.max].some((v) => v !== null) ? `${p.min ?? "…"} – ${p.max ?? "…"}` : null;
  const fields: [string, string, string | null][] = [
    ["value", t("Value"), valueText(p.value, p.unit) || null],
    ["default", t("Default"), valueText(p.default, p.unit) || null],
    ["limits", t("Range"), limits],
    ["source", t("Source"), p.source],
    ["context", t("Context"), contextText(p.context) ?? p.context],
  ];
  return (
    <aside className="admin-details" data-field="parameter-details">
      <div className="admin-details-title">{p.name}</div>
      {p.description && <p className="small">{p.description}</p>}
      <dl className="admin-fields">
        {fields
          .filter(([, , value]) => value)
          .map(([key, name, value]) => (
            <div key={key} data-field={key}>
              <dt>{name}</dt>
              <dd>{value}</dd>
            </div>
          ))}
      </dl>
      {p.statement ? (
        <>
          <div className="admin-heading">{t("To change it")}</div>
          <pre className="admin-statement" data-field="parameter-statement">
            {p.statement}
          </pre>
          <div className="admin-actions">
            <button data-action="parameter-open" onClick={() => onOpen(p.statement ?? "")}>
              {t("Open in the editor")}
            </button>
          </div>
          <p className="muted small">{t("The statement is not run: review it in the editor and run it there.")}</p>
        </>
      ) : (
        <p className="muted small">{t("The server does not let this parameter be set.")}</p>
      )}
    </aside>
  );
}
