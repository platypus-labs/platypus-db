// Copying the selection in other formats. The text is built by the backend from the full
// loaded values (the grid shows them cut to one line); pending edits are not included.
import { registerAction } from "../../actions";
import { api, type CopyFormat } from "../../api";
import type { GridActionContext } from "../DataGrid";

/** The result rows and columns of the selection; new rows of the editor are left out. */
function selectedRange({ grid }: GridActionContext) {
  const sel = grid.selection();
  if (!sel) return null;
  const last = grid.dataRow(sel.r1);
  if (last === null) return null;
  const first = grid.dataRow(sel.r0) ?? 0;
  const columns = sel.cols;
  return { offset: first, limit: last - first + 1, columns };
}

async function copy(ctx: GridActionContext, format: CopyFormat, header = false) {
  const range = selectedRange(ctx);
  const jobId = ctx.source?.jobId;
  if (!range || jobId == null) return;
  const text = await api.copyRows(jobId, range.offset, range.limit, range.columns, format, {
    header,
    table: ctx.source?.table,
  });
  await navigator.clipboard.writeText(text);
}

const canCopy = (ctx: GridActionContext) => ctx.source?.jobId != null && selectedRange(ctx) !== null;

registerAction({
  id: "grid.copy_with_headers",
  scope: "grid",
  title: "Copy with headers",
  shortcut: "Mod+Shift+C",
  group: "copy",
  order: 11,
  when: canCopy,
  run: (ctx) => copy(ctx, "tsv", true),
});

const formats: { id: string; title: string; format: CopyFormat; header?: boolean }[] = [
  { id: "csv", title: "CSV", format: "csv", header: true },
  { id: "json", title: "JSON", format: "json" },
  { id: "markdown", title: "Markdown table", format: "markdown" },
  { id: "html", title: "HTML table", format: "html" },
  { id: "insert", title: "SQL INSERT", format: "insert" },
  { id: "in_list", title: "SQL IN list", format: "in_list" },
];

formats.forEach((f, i) =>
  registerAction({
    id: `grid.copy_as_${f.id}`,
    scope: "grid",
    title: f.title,
    submenu: "Copy as",
    group: "copy",
    order: 12 + i,
    when: canCopy,
    run: (ctx) => copy(ctx, f.format, f.header),
  }),
);
