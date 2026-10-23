import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import type { ExportFormat, ExportOptions } from "../api";
import { fmt } from "./ResultPanel";

export type ExportSource = "query" | "loaded";

interface Props {
  /** Suggested file name without extension. */
  name: string;
  loadedRows: number;
  /** Whether the result holds every row already. */
  complete: boolean;
  /** Uncommitted changes are invisible to the export's own connection. */
  transactionOpen: boolean;
  onExport: (path: string, options: ExportOptions, source: ExportSource) => void;
  onClose: () => void;
}

const FORMATS: { value: ExportFormat; label: string; ext: string }[] = [
  { value: "csv", label: "CSV", ext: "csv" },
  { value: "json", label: "JSON (array)", ext: "json" },
  { value: "ndjson", label: "NDJSON (one object per line)", ext: "ndjson" },
  { value: "parquet", label: "Parquet (zstd)", ext: "parquet" },
];

const DELIMITERS = [
  { value: ",", label: "Comma ," },
  { value: ";", label: "Semicolon ;" },
  { value: "\t", label: "Tab" },
  { value: "|", label: "Pipe |" },
];

export function ExportDialog({ name, loadedRows, complete, transactionOpen, onExport, onClose }: Props) {
  const [options, setOptions] = useState<ExportOptions>({ format: "csv", delimiter: ",", header: true });
  const [source, setSource] = useState<ExportSource>(complete ? "loaded" : "query");
  const format = FORMATS.find((f) => f.value === options.format)!;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const path = await save({
      defaultPath: `${name}.${format.ext}`,
      filters: [{ name: format.label, extensions: [format.ext] }],
    });
    if (path) onExport(path, options, source);
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="modal small" onSubmit={submit} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>Export result</h2>
        <label className="field">
          <span>Format</span>
          <select
            autoFocus
            value={options.format}
            onChange={(e) => setOptions({ ...options, format: e.target.value as ExportFormat })}
          >
            {FORMATS.map((f) => (
              <option key={f.value} value={f.value}>
                {f.label}
              </option>
            ))}
          </select>
        </label>
        {options.format === "csv" && (
          <div className="row">
            <label className="field">
              <span>Delimiter</span>
              <select
                value={options.delimiter}
                onChange={(e) => setOptions({ ...options, delimiter: e.target.value })}
              >
                {DELIMITERS.map((d) => (
                  <option key={d.label} value={d.value}>
                    {d.label}
                  </option>
                ))}
              </select>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={options.header}
                onChange={(e) => setOptions({ ...options, header: e.target.checked })}
              />
              Header row
            </label>
          </div>
        )}
        <fieldset className="choices">
          <label className="check">
            <input type="radio" checked={source === "query"} onChange={() => setSource("query")} />
            All rows — run the query again and stream it to the file
          </label>
          <label className="check">
            <input type="radio" checked={source === "loaded"} onChange={() => setSource("loaded")} />
            Rows already loaded ({fmt.format(loadedRows)}
            {complete ? ", the whole result" : ""})
          </label>
        </fieldset>
        {source === "query" && transactionOpen && (
          <div className="notice warn small">
            The export uses its own connection: uncommitted changes in this tab are not included.
          </div>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="primary">
            Export…
          </button>
        </div>
      </form>
    </div>
  );
}
