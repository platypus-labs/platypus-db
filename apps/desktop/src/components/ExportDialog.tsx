import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import type { Bucket, Compression, DriverKind, ExportFormat, ExportOptions, TransferProfile } from "../api";
import { ProfileButtons } from "../transfer/ProfileSave";
import { fmt } from "./ResultPanel";
import { t, msg } from "../i18n";

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
  /** The query to keep as a profile, when it can be: where it runs and its text. */
  query?: { connection_id: string; database: string | null; sql: string };
  /** The profile the dialog was opened from: its choices start the dialog. */
  profile?: TransferProfile;
  /** Only the query can be exported, not rows already loaded. */
  queryOnly?: boolean;
  onExport: (path: string, options: ExportOptions, source: ExportSource) => void;
  onClose: () => void;
}

export const FORMATS: { value: ExportFormat; label: string; ext: string }[] = [
  { value: "csv", label: "CSV", ext: "csv" },
  { value: "json", label: msg("JSON (array)"), ext: "json" },
  { value: "ndjson", label: msg("NDJSON (one object per line)"), ext: "ndjson" },
  { value: "parquet", label: "Parquet (zstd)", ext: "parquet" },
  { value: "xlsx", label: msg("Excel workbook"), ext: "xlsx" },
  { value: "sql", label: msg("SQL INSERT statements"), ext: "sql" },
  { value: "markdown", label: msg("Markdown table"), ext: "md" },
  { value: "html", label: msg("HTML table"), ext: "html" },
  { value: "xml", label: "XML", ext: "xml" },
];

const DELIMITERS = [
  { value: ",", label: msg("Comma ,") },
  { value: ";", label: msg("Semicolon ;") },
  { value: "\t", label: msg("Tab character") },
  { value: "|", label: msg("Pipe |") },
];

const COMPRESSIONS: { value: Compression; label: string; ext: string }[] = [
  { value: "none", label: msg("None"), ext: "" },
  { value: "gzip", label: "gzip (.gz)", ext: "gz" },
  { value: "zstd", label: "zstd (.zst)", ext: "zst" },
];

/** The extension a compressed file adds, e.g. `gz`; empty when uncompressed. */
const compressedExt = (c: Compression | undefined) => COMPRESSIONS.find((x) => x.value === (c ?? "none"))!.ext;

// Parquet and Excel files are compressed already.
const packed = (format: ExportFormat) => format === "parquet" || format === "xlsx";

/** `options` as sent: Parquet and Excel files take no compression, whatever was picked before,
 *  and an empty signed URL is none. */
export const sentOptions = (options: ExportOptions): ExportOptions => ({
  ...options,
  compression: packed(options.format) ? "none" : options.compression,
  upload_url: options.upload_url?.trim() || null,
});

const BUCKET_KEY = "platypus.export.bucket";

/** The bucket of the last upload, without its secret key, which is never kept. */
function lastBucket(): Bucket {
  let kept: Partial<Bucket> = {};
  try {
    kept = JSON.parse(localStorage.getItem(BUCKET_KEY) ?? "{}") as Partial<Bucket>;
  } catch {
    // Nothing kept, or nothing readable.
  }
  const empty = { endpoint: "https://s3.amazonaws.com", region: "us-east-1", bucket: "", prefix: "", access_key: "" };
  return { ...empty, ...kept, secret_key: "" };
}

function keepBucket({ secret_key: _, ...rest }: Bucket) {
  try {
    localStorage.setItem(BUCKET_KEY, JSON.stringify(rest));
  } catch {
    // Remembering it is a convenience.
  }
}

const BUCKET_FIELDS: { key: keyof Bucket; label: string; placeholder?: string }[] = [
  { key: "endpoint", label: msg("Endpoint"), placeholder: "https://s3.eu-west-1.amazonaws.com" },
  { key: "region", label: msg("Region"), placeholder: msg("us-east-1, auto for GCS and R2") },
  { key: "bucket", label: msg("Bucket") },
  { key: "prefix", label: msg("Folder (optional)") },
  { key: "access_key", label: msg("Access key") },
  { key: "secret_key", label: msg("Secret key") },
];

/** The compression of the finished file and where to upload it: a signed URL (with
 *  `signedUrl`, one file) or a bucket with its keys. */
export function OutputOptions(props: {
  options: ExportOptions;
  signedUrl: boolean;
  disabled?: boolean;
  onChange: (o: ExportOptions) => void;
}) {
  const { options, signedUrl, disabled, onChange } = props;
  const upload = options.bucket ? "bucket" : options.upload_url != null ? "url" : "none";
  const setBucket = (bucket: Bucket) => {
    keepBucket(bucket);
    onChange({ ...options, bucket });
  };
  return (
    <>
      {!packed(options.format) && (
        <label className="field">
          <span>{t("Compression")}</span>
          <select
            value={options.compression ?? "none"}
            disabled={disabled}
            onChange={(e) => onChange({ ...options, compression: e.target.value as Compression })}
          >
            {COMPRESSIONS.map((c) => (
              <option key={c.value} value={c.value}>
                {t(c.label)}
              </option>
            ))}
          </select>
        </label>
      )}
      <label className="field">
        <span>{t("Also upload")}</span>
        <select
          value={upload}
          disabled={disabled}
          onChange={(e) => {
            const to = e.target.value;
            onChange({ ...options, upload_url: to === "url" ? "" : null, bucket: to === "bucket" ? lastBucket() : null });
          }}
        >
          <option value="none">{t("No")}</option>
          {signedUrl && <option value="url">{t("To a signed URL (S3, GCS)")}</option>}
          <option value="bucket">{t("To a bucket (S3, GCS, R2, MinIO)")}</option>
        </select>
      </label>
      {upload === "url" && (
        <label className="field">
          <span>{t("Signed URL")}</span>
          <input
            value={options.upload_url ?? ""}
            disabled={disabled}
            placeholder="https://bucket.s3.amazonaws.com/file.csv?X-Amz-Signature=…"
            onChange={(e) => onChange({ ...options, upload_url: e.target.value.trim() })}
          />
        </label>
      )}
      {options.bucket && (
        <div className="bucket-fields">
          {BUCKET_FIELDS.map(({ key, label, placeholder }) => (
            <label key={key} className="field">
              <span>{t(label)}</span>
              <input
                type={key === "secret_key" ? "password" : "text"}
                value={options.bucket![key]}
                disabled={disabled}
                placeholder={placeholder && t(placeholder)}
                autoComplete="off"
                onChange={(e) => setBucket({ ...options.bucket!, [key]: e.target.value.trim() })}
              />
            </label>
          ))}
        </div>
      )}
    </>
  );
}

/** The CSV delimiter and header row of `options`. */
export function CsvOptions(props: { options: ExportOptions; disabled?: boolean; onChange: (o: ExportOptions) => void }) {
  const { options, disabled, onChange } = props;
  return (
    <div className="row">
      <label className="field">
        <span>{t("Delimiter")}</span>
        <select value={options.delimiter} disabled={disabled} onChange={(e) => onChange({ ...options, delimiter: e.target.value })}>
          {DELIMITERS.map((d) => (
            <option key={d.label} value={d.value}>
              {t(d.label)}
            </option>
          ))}
        </select>
      </label>
      <label className="check">
        <input type="checkbox" checked={options.header} disabled={disabled} onChange={(e) => onChange({ ...options, header: e.target.checked })} />
        {t("Header row")}
      </label>
    </div>
  );
}

export function ExportDialog({ name, driver, loadedRows, complete, transactionOpen, query, profile, queryOnly, onExport, onClose }: Props) {
  const saved = profile?.task.kind === "export_query" ? profile.task : null;
  const [options, setOptions] = useState<ExportOptions>(
    saved?.options ?? {
      format: "csv",
      delimiter: ",",
      header: true,
      table: name,
      driver,
    },
  );
  const [source, setSource] = useState<ExportSource>(complete && !queryOnly ? "loaded" : "query");
  const format = FORMATS.find((f) => f.value === options.format)!;

  /** The file to write, asked as the dialog's own choices name it; `null` if none is chosen. */
  async function askPath(chosen: ExportOptions) {
    const zipped = compressedExt(chosen.compression);
    return save({
      defaultPath: saved?.path ?? `${name}.${format.ext}${zipped ? `.${zipped}` : ""}`,
      filters: [{ name: t(format.label), extensions: [zipped || format.ext] }],
    });
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const chosen = sentOptions(options);
    const path = await askPath(chosen);
    if (path) onExport(path, chosen, source);
  }

  async function build() {
    const chosen = sentOptions(options);
    const path = await askPath(chosen);
    return path && query ? { kind: "export_query" as const, ...query, path, options: chosen } : null;
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="modal small" onSubmit={submit} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{profile ? profile.name : t("Export result")}</h2>
        <label className="field">
          <span>{t("Format")}</span>
          <select
            autoFocus
            value={options.format}
            onChange={(e) => setOptions({ ...options, format: e.target.value as ExportFormat })}
          >
            {FORMATS.map((f) => (
              <option key={f.value} value={f.value}>
                {t(f.label)}
              </option>
            ))}
          </select>
        </label>
        {options.format === "sql" && (
          <label className="field">
            <span>{t("Table name")}</span>
            <input value={options.table} onChange={(e) => setOptions({ ...options, table: e.target.value })} />
          </label>
        )}
        {options.format === "csv" && <CsvOptions options={options} onChange={setOptions} />}
        <OutputOptions options={options} signedUrl onChange={setOptions} />
        {!queryOnly && (
          <fieldset className="choices">
            <label className="check">
              <input type="radio" checked={source === "query"} onChange={() => setSource("query")} />
              {t("All rows — run the query again and stream it to the file")}
            </label>
            <label className="check">
              <input type="radio" checked={source === "loaded"} onChange={() => setSource("loaded")} />
              {t(complete ? "Rows already loaded ({n}, the whole result)" : "Rows already loaded ({n})", { n: fmt.format(loadedRows) })}
            </label>
          </fieldset>
        )}
        {source === "query" && transactionOpen && (
          <div className="notice warn small">
            {t("The export uses its own connection: uncommitted changes in this tab are not included.")}
          </div>
        )}
        <div className="actions">
          <span className="spacer" />
          {query && source === "query" && <ProfileButtons profile={profile} build={build} />}
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary">
            {t("Export…")}
          </button>
        </div>
      </form>
    </div>
  );
}
