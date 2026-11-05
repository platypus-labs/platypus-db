// The agent report as Markdown and as one HTML file, built from the summary the store
// aggregates. Pure functions with no import from the app, so the tests load them as they are.
// Agent names, session ids and table names come from the log and are not to be trusted: every
// text goes through an escape before it is written into either format.

/** The figures of a group of calls; a figure is null when the calls were made before the log
 *  recorded it. */
export interface ReportFigures {
  first_at: number;
  last_at: number;
  calls: number;
  measured: number;
  tables_read: number | null;
  rows_returned: number | null;
  changes_allowed: number | null;
  changes_refused: number | null;
  errors: number;
}

export interface ReportSession extends ReportFigures {
  session: string;
}

export interface ReportAgent extends ReportFigures {
  agent: string;
  sessions: ReportSession[];
}

/** The words of the report, already translated. */
export interface ReportText {
  title: string;
  /** Under the title: when it was made and what it covers. */
  subtitle: string;
  agent: string;
  period: string;
  calls: string;
  tables: string;
  rows: string;
  allowed: string;
  refused: string;
  errors: string;
  /** Said when some calls predate the figures. */
  partial: string;
  /** Said always: what is an estimate. */
  estimate: string;
}

/** A figure that was never recorded. */
export const MISSING = "–";

const figure = (n: number | null): string => (n === null ? MISSING : String(n));

/** `2026-11-05 10:20Z`: the report reads the same wherever it is opened. */
function stamp(ms: number): string {
  return new Date(ms).toISOString().slice(0, 16).replace("T", " ") + "Z";
}

/** The period of a group of calls. */
export function periodText(first: number, last: number): string {
  const a = stamp(first);
  const b = stamp(last);
  if (a === b) return a;
  // The same day: the second time only.
  return a.slice(0, 10) === b.slice(0, 10) ? `${a} – ${b.slice(11)}` : `${a} – ${b}`;
}

/** A session id shortened for reading; the log keeps the whole of it. */
export function shortSession(id: string): string {
  return id.length > 13 ? `${id.slice(0, 8)}…` : id;
}

/** True when some calls were made before the log recorded the figures. */
export function hasOlderCalls(report: ReportAgent[]): boolean {
  return report.some((a) => a.measured < a.calls || a.sessions.some((s) => s.measured < s.calls));
}

/** Text safe inside a Markdown table cell, and in a renderer that lets raw HTML through: line
 *  breaks become spaces, `|`, backticks and the markup characters are escaped. */
export function markdownCell(text: string): string {
  return text
    .replace(/\r\n|[\r\n\u2028\u2029]/g, " ")
    .replace(/[\\|`*_[\]~]/g, (c) => `\\${c}`)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

/** Text safe as HTML content and inside a quoted attribute. */
export function htmlEscape(text: string): string {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

interface Row {
  label: string;
  nested: boolean;
  figures: ReportFigures;
}

/** The report as rows: an agent, then its sessions. */
function rows(report: ReportAgent[]): Row[] {
  return report.flatMap((a) => [
    { label: a.agent, nested: false, figures: a as ReportFigures },
    ...a.sessions.map((s) => ({ label: shortSession(s.session), nested: true, figures: s as ReportFigures })),
  ]);
}

/** The cells of a row after its name. */
function cells(f: ReportFigures): string[] {
  return [
    periodText(f.first_at, f.last_at),
    String(f.calls),
    figure(f.tables_read),
    figure(f.rows_returned),
    figure(f.changes_allowed),
    figure(f.changes_refused),
    String(f.errors),
  ];
}

const heads = (text: ReportText): string[] => [
  text.agent,
  text.period,
  text.calls,
  text.tables,
  text.rows,
  text.allowed,
  text.refused,
  text.errors,
];

/** The report as a Markdown document: one table, a row per agent and under it a row per session. */
export function reportMarkdown(report: ReportAgent[], text: ReportText): string {
  const line = (cols: string[]) => `| ${cols.join(" | ")} |`;
  const out = [`# ${markdownCell(text.title)}`, "", markdownCell(text.subtitle), ""];
  out.push(line(heads(text).map(markdownCell)), line(heads(text).map(() => "---")));
  for (const r of rows(report)) {
    const name = markdownCell(r.label);
    out.push(line([r.nested ? `&nbsp;&nbsp;↳ ${name}` : `**${name}**`, ...cells(r.figures).map(markdownCell)]));
  }
  out.push("", `*${markdownCell(text.estimate)}*`);
  if (hasOlderCalls(report)) out.push("", `*${markdownCell(text.partial)}*`);
  return out.join("\n") + "\n";
}

const STYLE = `body{font:14px/1.5 system-ui,sans-serif;margin:2rem;color:#1d1d1f;background:#fff}
h1{font-size:1.4rem;margin:0 0 .25rem}p{margin:.25rem 0 1rem;color:#555}
table{border-collapse:collapse;width:100%}th,td{padding:.35rem .6rem;border-bottom:1px solid #ddd;text-align:left}
th{background:#f4f4f6}td.n{text-align:right;font-variant-numeric:tabular-nums}
tr.agent td{font-weight:600;background:#fafafa}tr.session td:first-child{padding-left:1.8rem;font-weight:400}
.note{font-size:.85rem;color:#555}`;

/** The report as one HTML file: its style inline, no script and no link to anything outside. */
export function reportHtml(report: ReportAgent[], text: ReportText, lang = "en"): string {
  const e = htmlEscape;
  const head = heads(text)
    .map((h, i) => `<th${i > 1 ? ' class="n"' : ""}>${e(h)}</th>`)
    .join("");
  const body = rows(report)
    .map((r) => {
      const [period = "", ...numbers] = cells(r.figures);
      const tds = [`<td>${e(r.label)}</td>`, `<td>${e(period)}</td>`, ...numbers.map((n) => `<td class="n">${e(n)}</td>`)];
      return `<tr class="${r.nested ? "session" : "agent"}">${tds.join("")}</tr>`;
    })
    .join("\n");
  const notes = [text.estimate, ...(hasOlderCalls(report) ? [text.partial] : [])]
    .map((n) => `<p class="note">${e(n)}</p>`)
    .join("\n");
  return `<!doctype html>
<html lang="${e(lang)}">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'">
<title>${e(text.title)}</title>
<style>${STYLE}</style>
</head>
<body>
<h1>${e(text.title)}</h1>
<p>${e(text.subtitle)}</p>
<table>
<thead><tr>${head}</tr></thead>
<tbody>
${body}
</tbody>
</table>
${notes}
</body>
</html>
`;
}
