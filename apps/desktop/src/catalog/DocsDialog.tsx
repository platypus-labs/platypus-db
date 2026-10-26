import { createElement, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, type DocsFormat, type DocsRequest, type SavedConnection, toCommandError } from "../api";
import { answerFor } from "../testAnswers";
import { workbench } from "../workbench";
import { t } from "../i18n";

interface Props {
  connection: SavedConnection;
  database: string | null;
  /** The schemas that can be documented. */
  schemas: string[];
  /** Those checked at first. */
  initial: string[];
  onClose: () => void;
}

type Status =
  | { state: "idle" }
  | { state: "running"; id: number | null; done: number; total: number }
  | { state: "finished"; path: string }
  | { state: "failed"; message: string };

const FORMATS: { value: DocsFormat; label: string; ext: string }[] = [
  { value: "html", label: "HTML", ext: "html" },
  { value: "markdown", label: "Markdown", ext: "md" },
];

const formatOf = (value: DocsFormat) => FORMATS.find((f) => f.value === value)!;

/** `path` with the extension of `format` in place of the other format's. */
const withExtension = (path: string, format: DocsFormat) => path.replace(/\.(md|html?)$/i, `.${formatOf(format).ext}`);

/** Writes the documentation of some schemas of a connection to a Markdown or HTML file. */
function DocsDialog({ connection, database, schemas, initial, onClose }: Props) {
  const [request, setRequest] = useState<DocsRequest>({
    title: database ?? connection.name,
    schemas: initial,
    views: true,
    diagrams: true,
    notes: true,
    format: "html",
    path: "",
  });
  const [status, setStatus] = useState<Status>({ state: "idle" });
  const set = (patch: Partial<DocsRequest>) => setRequest((r) => ({ ...r, ...patch }));
  const running = status.state === "running";
  const close = () => !running && onClose();

  const toggle = (schema: string, on: boolean) =>
    set({ schemas: schemas.filter((s) => (s === schema ? on : request.schemas.includes(s))) });

  async function choose() {
    const format = formatOf(request.format);
    const name = request.title.replace(/[<>:"/\\|?*]/g, "_").trim() || "schema";
    const path = await answerFor("file", () =>
      save({ defaultPath: `${name}.${format.ext}`, filters: [{ name: format.label, extensions: [format.ext] }] }),
    );
    if (typeof path === "string") set({ path });
  }

  async function run() {
    setStatus({ state: "running", id: null, done: 0, total: 0 });
    try {
      const sid = await workbench().sideSession(connection, database);
      if (sid === null) return setStatus({ state: "idle" });
      const id = await api.exportDocs(sid, connection.id, database, { ...request, path: request.path.trim() }, (e) => {
        if (e.kind === "progress") setStatus((s) => (s.state === "running" ? { ...s, done: e.done, total: e.total } : s));
        else if (e.kind === "finished") setStatus({ state: "finished", path: e.path });
        else setStatus({ state: "failed", message: e.message });
      });
      setStatus((s) => (s.state === "running" ? { ...s, id } : s));
    } catch (e) {
      setStatus({ state: "failed", message: toCommandError(e).message });
    }
  }

  function cancel() {
    if (status.state === "running" && status.id !== null) void api.cancelDocs(status.id);
    setStatus({ state: "idle" });
  }

  const check = (label: string, checked: boolean, onChange: (on: boolean) => void) => (
    <label className="check" key={label}>
      <input type="checkbox" checked={checked} disabled={running} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  );

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <form
        className="modal docs-dialog"
        onKeyDown={(e) => e.key === "Escape" && close()}
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <h2>{t("Export documentation")}</h2>
        <div className="row">
          <label className="field">
            <span>{t("Title")}</span>
            <input autoFocus value={request.title} disabled={running} onChange={(e) => set({ title: e.target.value })} />
          </label>
          <label className="field docs-format">
            <span>{t("Format")}</span>
            <select
              value={request.format}
              disabled={running}
              onChange={(e) => {
                const format = e.target.value as DocsFormat;
                set({ format, path: withExtension(request.path, format) });
              }}
            >
              {FORMATS.map((f) => (
                <option key={f.value} value={f.value}>
                  {f.label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="field">
          <span>{t("Schemas")}</span>
          <fieldset className="choices docs-schemas">
            {schemas.map((s) => check(s, request.schemas.includes(s), (on) => toggle(s, on)))}
          </fieldset>
        </div>
        <fieldset className="choices">
          {check(t("Views"), request.views, (views) => set({ views }))}
          {check(t("ER diagram"), request.diagrams, (diagrams) => set({ diagrams }))}
          {check(t("Notes and tags"), request.notes, (notes) => set({ notes }))}
        </fieldset>
        <div className="field">
          <span>{t("File")}</span>
          <div className="row tight">
            <input
              aria-label={t("File")}
              value={request.path}
              disabled={running}
              spellCheck={false}
              onChange={(e) => set({ path: e.target.value })}
            />
            <button type="button" disabled={running} onClick={() => void choose()}>
              {t("Choose file…")}
            </button>
          </div>
        </div>
        {status.state === "running" && (
          <div className="docs-progress">
            <progress max={status.total || undefined} value={status.total ? status.done : undefined} />
            <p className="muted small">
              {status.total ? t("{done} of {total}", { done: status.done, total: status.total }) : t("Reading the tables…")}
            </p>
          </div>
        )}
        {status.state === "finished" && (
          <div className="notice ok small">{t("Documentation written to {path}", { path: status.path })}</div>
        )}
        {status.state === "failed" && <div className="notice error small">{status.message}</div>}
        <div className="actions">
          <span className="spacer" />
          {running ? (
            <button type="button" onClick={cancel}>
              {t("Cancel")}
            </button>
          ) : (
            <button type="button" onClick={onClose}>
              {t(status.state === "finished" ? "Close" : "Cancel")}
            </button>
          )}
          <button type="submit" className="primary" disabled={running || request.schemas.length === 0 || !request.path.trim()}>
            {t("Export")}
          </button>
        </div>
      </form>
    </div>
  );
}

/** Opens the documentation dialog for the schemas of `connection` (in `database`), with
 *  `schema` checked, or every user schema when `null`. */
export async function openDocs(connection: SavedConnection, database: string | null, schema: string | null) {
  try {
    const sid = await workbench().sideSession(connection, database);
    if (sid === null) return;
    const listed = await api.listSchemas(sid);
    const user = listed.filter((s) => !s.system || s.name === schema).map((s) => s.name);
    const schemas = user.length ? user : listed.map((s) => s.name);
    const initial = schema === null ? schemas : [schema];
    workbench().openDialog((close) => createElement(DocsDialog, { connection, database, schemas, initial, onClose: close }));
  } catch (e) {
    workbench().showError(toCommandError(e).message);
  }
}
