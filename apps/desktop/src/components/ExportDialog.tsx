import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import type { Compression, DriverKind, ExportFormat, ExportOptions } from "../api";
import { fmt } from "./ResultPanel";

export type ExportSource = "query" | "loaded";

interface Props {
  /** Suggested file name without extension. */
  name: string;
  /** The connection's database, which SQL statements are written for. */
  driver: DriverKind;
  loadedRows: number;
  /** Whether the result holds every row already. */
  complete: boolean;
  /** Uncommitted changes are invisible to the export's own connection. */
  transactionOpen: boolean;
  onExport: (path: string, options: ExportOptions, source: ExportSource) => void;
  onClose: () => void;
}

export const FORMATS: { value: ExportFormat; label: string; ext: string }[] = [
  { value: "csv", label: "CSV", ext: "csv" },
  { value: "json", label: "JSON (array)", ext: "json" },
  { value: "ndjson", label: "NDJSON (one object per line)", ext: "ndjson" },
  { value: "parquet", label: "Parquet (zstd)", ext: "parquet" },
  { value: "xlsx", label: "Excel workbook", ext: "xlsx" },
  { value: "sql", label: "SQL INSERT statements", ext: "sql" },
  { value: "markdown", label: "Markdown table", ext: "md" },
  { value: "html", label: "HTML table", ext: "html" },
  { value: "xml", label: "XML", ext: "xml" },
];

const DELIMITERS = [
  { value: ",", label: "Comma ," },
  { value: ";", label: "Semicolon ;" },
  { value: "\t", label: "Tab" },
  { value: "|", label: "Pipe |" },
];

/** The CSV delimiter and header row of `options`. */
const COMPRESSIONS: { value: Compression; label: string; ext: string }[] = [
  { value: "none", label: "None", ext: "" },
  { value: "gzip", label: "gzip (.gz)", ext: "gz" },
  { value: "zstd", label: "zstd (.zst)", ext: "zst" },
];

/** The extension a compressed file adds, e.g. `gz`; empty when uncompressed. */
export const compressedExt = (c: Compression | undefined) => COMPRESSIONS.find((x) => x.value === (c ?? "none"))!.ext;

/** The compression of the finished file and, with `upload`, a signed URL to send it to. */
export function OutputOptions(props: {
  options: ExportOptions;
  upload: boolean;
  disabled?: boolean;
  onChange: (o: ExportOptions) => void;
}) {
  const { options, upload, disabled, onChange } = props;
  // Parquet and Excel files are compressed already.
  const packed = options.format === "parquet" || options.format === "xlsx";
  return (
    <>
      {!packed && (
        <label className="field">
          <span>Compression</span>
          <select
            value={options.compression ?? "none"}
            disabled={disabled}
            onChange={(e) => onChange({ ...options, compression: e.target.value as Compression })}
          >
            {COMPRESSIONS.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
      )}
      {upload && (
        <label className="field">
          <span>Also upload to (signed S3 or GCS URL, optional)</span>
          <input
            value={options.upload_url ?? ""}
            disabled={disabled}
            placeholder="https://bucket.s3.amazonaws.com/file.csv?X-Amz-Signature=…"
            onChange={(e) => onChange({ ...options, upload_url: e.target.value.trim() || null })}
          />
        </label>
      )}
    </>
  );
}

export function CsvOptions(props: { options: ExportOptions; disabled?: boolean; onChange: (o: ExportOptions) => void }) {
  const { options, disabled, onChange } = props;
  return (
    <div className="row">
      <label className="field">
        <span>Delimiter</span>
        <select value={options.delimiter} disabled={disabled} onChange={(e) => onChange({ ...options, delimiter: e.target.value })}>
          {DELIMITERS.map((d) => (
            <option key={d.label} value={d.value}>
              {d.label}
            </option>
          ))}
        </select>
      </label>
      <label className="check">
        <input type="checkbox" checked={options.header} disabled={disabled} onChange={(e) => onChange({ ...options, header: e.target.checked })} />
        Header row
      </label>
    </div>
  );
}

export function ExportDialog({ name, driver, loadedRows, complete, transactionOpen, onExport, onClose }: Props) {
  const [options, setOptions] = useState<ExportOptions>({
    format: "csv",
    delimiter: ",",
    header: true,
    table: name,
    driver,
  });
  const [source, setSource] = useState<ExportSource>(complete ? "loaded" : "query");
  const format = FORMATS.find((f) => f.value === options.format)!;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const packed = options.format === "parquet" || options.format === "xlsx";
    const chosen = { ...options, compression: packed ? "none" : options.compression } as ExportOptions;
    const zipped = compressedExt(chosen.compression);
    const path = await save({
      defaultPath: `${name}.${format.ext}${zipped ? `.${zipped}` : ""}`,
      filters: [{ name: format.label, extensions: [zipped || format.ext] }],
    });
    if (path) onExport(path, chosen, source);
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
        {options.format === "sql" && (
          <label className="field">
            <span>Table name</span>
            <input value={options.table} onChange={(e) => setOptions({ ...options, table: e.target.value })} />
          </label>
        )}
        {options.format === "csv" && <CsvOptions options={options} onChange={setOptions} />}
        <OutputOptions options={options} upload onChange={setOptions} />
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
