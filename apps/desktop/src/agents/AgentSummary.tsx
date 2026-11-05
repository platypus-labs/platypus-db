import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, toCommandError, type AgentCallQuery, type AgentReportAgent } from "../api";
import { t } from "../i18n";
import { MISSING, hasOlderCalls, reportHtml, reportMarkdown, shortSession, type ReportText } from "./agentReport";

const figure = (n: number | null): string => (n === null ? MISSING : String(n));

function when(ms: number): string {
  return new Date(ms).toLocaleString();
}

function period(first: number, last: number): string {
  const a = new Date(first);
  const b = new Date(last);
  if (first === last) return when(first);
  return a.toDateString() === b.toDateString() ? `${when(first)} – ${b.toLocaleTimeString()}` : `${when(first)} – ${when(last)}`;
}

/** The words of the exported report, in the app's language. */
function reportText(report: AgentReportAgent[]): ReportText {
  const calls = report.reduce((n, a) => n + a.calls, 0);
  return {
    title: t("Agent report"),
    subtitle: t("Made {when} · {n} calls", { when: new Date().toISOString().slice(0, 16).replace("T", " ") + "Z", n: calls }),
    agent: t("Agent"),
    period: t("Period"),
    calls: t("Calls"),
    tables: t("Tables read"),
    rows: t("Rows returned"),
    allowed: t("Changes allowed"),
    refused: t("Changes refused"),
    errors: t("Errors"),
    partial: t(
      "Calls made before the log recorded these figures show – for them; the figures count only the calls recorded with them.",
    ),
    estimate: t("Tables read are an estimate for statements: the tables named after FROM and JOIN."),
  };
}

/** One row of the summary: an agent, or a session under it. */
function Row({ label, f, kind }: { label: string; f: AgentReportAgent; kind: "agent" | "session" }) {
  return (
    <tr data-report={kind} data-name={label} className={kind === "session" ? "agent-report-session" : "agent-report-agent"}>
      <th scope="row">{label}</th>
      <td>{period(f.first_at, f.last_at)}</td>
      <td data-col="calls">{f.calls}</td>
      <td data-col="tables">{figure(f.tables_read)}</td>
      <td data-col="rows">{figure(f.rows_returned)}</td>
      <td data-col="allowed">{figure(f.changes_allowed)}</td>
      <td data-col="refused">{figure(f.changes_refused)}</td>
      <td data-col="errors" className={f.errors ? "agent-call-failed" : undefined}>
        {f.errors}
      </td>
    </tr>
  );
}

/** The summary of the calls the Activity filters select, per agent and session, with its
 *  Markdown and HTML export. Loaded when the page opens it; `query` is a new object each time
 *  the page reloads its calls, which reads the figures again. */
export default function AgentSummary({
  query,
  onNotice,
  onError,
}: {
  query: AgentCallQuery;
  onNotice: (text: string) => void;
  onError: (text: string) => void;
}) {
  const [report, setReport] = useState<AgentReportAgent[] | null>(null);

  useEffect(() => {
    let current = true;
    api.agentReport(query).then(
      (r) => current && setReport(r),
      (e: unknown) => current && onError(toCommandError(e).message),
    );
    return () => {
      current = false;
    };
  }, [query, onError]);

  const exportAs = async (kind: "md" | "html") => {
    if (!report) return;
    try {
      const path = await save({
        filters: kind === "md" ? [{ name: "Markdown", extensions: ["md"] }] : [{ name: "HTML", extensions: ["html"] }],
        defaultPath: kind === "md" ? "agent-report.md" : "agent-report.html",
      });
      if (!path) return;
      const text = reportText(report);
      await api.writeTextFile(path, kind === "md" ? reportMarkdown(report, text) : reportHtml(report, text, document.documentElement.lang || "en"));
      onNotice(t("Saved {path}.", { path }));
    } catch (e) {
      onError(toCommandError(e).message);
    }
  };

  if (!report) return <p className="muted small">{t("Loading…")}</p>;
  return (
    <div className="agent-report" data-agent-report>
      <div className="row tight">
        <p className="muted small">
          {t("Each start of the MCP server is a session; the summary covers the calls the filters select.")}
        </p>
        <span className="spacer" />
        <button type="button" onClick={() => void exportAs("md")} data-action="export-report-md" disabled={report.length === 0}>
          {t("Export Markdown")}
        </button>
        <button type="button" onClick={() => void exportAs("html")} data-action="export-report-html" disabled={report.length === 0}>
          {t("Export HTML")}
        </button>
      </div>
      {report.length === 0 ? (
        <p className="muted small">{t("No calls to summarize.")}</p>
      ) : (
        <div className="agent-report-scroll">
          <table className="agents-connections">
            <thead>
              <tr>
                <th>{t("Agent")}</th>
                <th>{t("Period")}</th>
                <th>{t("Calls")}</th>
                <th>{t("Tables read")}</th>
                <th>{t("Rows returned")}</th>
                <th>{t("Changes allowed")}</th>
                <th>{t("Changes refused")}</th>
                <th>{t("Errors")}</th>
              </tr>
            </thead>
            <tbody>
              {report.flatMap((a) => [
                <Row key={a.agent} label={a.agent} f={a} kind="agent" />,
                ...a.sessions.map((s) => <Row key={`${a.agent}/${s.session}`} label={shortSession(s.session)} f={{ ...s, agent: a.agent, sessions: [] }} kind="session" />),
              ])}
            </tbody>
          </table>
        </div>
      )}
      <p className="muted small">{t("Tables read are an estimate for statements: the tables named after FROM and JOIN.")}</p>
      {hasOlderCalls(report) && (
        <p className="muted small">
          {t(
            "Calls made before the log recorded these figures show – for them; the figures count only the calls recorded with them.",
          )}
        </p>
      )}
    </div>
  );
}
