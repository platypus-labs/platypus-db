import { useEffect, useState } from "react";
import { api, type DriverKind, type ObjectForm, type SavedConnection, toCommandError } from "../api";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { workbench } from "../workbench";
import { msg, t } from "../i18n";

export type ObjectFormKind = ObjectForm["kind"];

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  kind: ObjectFormKind;
  /** The table a new trigger is on, when the dialog opens from a table's menu. */
  table?: string;
  onClose: () => void;
}

type Timing = "before" | "after" | "instead_of";
type Event = "insert" | "update" | "delete";
type ParamRow = { name: string; data_type: string; mode: string };

/** Everything the dialog edits, as the fields hold it (numbers are text until sent). */
interface Fields {
  name: string;
  replace: boolean;
  materialized: boolean;
  query: string;
  start: string;
  increment: string;
  min: string;
  max: string;
  cache: string;
  cycle: boolean;
  params: ParamRow[];
  returns: string;
  language: string;
  body: string;
  table: string;
  timing: Timing;
  events: Event[];
  forEachRow: boolean;
  when: string;
}

/** How long typing pauses before the statements are generated again. */
const TYPING_PAUSE_MS = 250;

const TITLES: Record<ObjectFormKind, string> = {
  view: msg("Create view"),
  sequence: msg("Create sequence"),
  function: msg("Create function"),
  trigger: msg("Create trigger"),
};

const TIMINGS: { value: Timing; label: string }[] = [
  { value: "before", label: "BEFORE" },
  { value: "after", label: "AFTER" },
  { value: "instead_of", label: "INSTEAD OF" },
];

/** The timings a database's triggers can have. */
function timingsOf(driver: DriverKind): Timing[] {
  if (driver === "sqlserver") return ["after", "instead_of"];
  if (driver === "mysql") return ["before", "after"];
  return ["before", "after", "instead_of"];
}

/** A whole number from a field: `null` when empty, `undefined` when it is not a number. */
function whole(text: string): number | null | undefined {
  const s = text.trim();
  if (s === "") return null;
  return /^[+-]?\d+$/.test(s) && Number.isSafeInteger(Number(s)) ? Number(s) : undefined;
}

/** The form the backend generates SQL from, or the message about a field that is no number. */
function toForm(kind: ObjectFormKind, f: Fields, driver: DriverKind): ObjectForm | string {
  const text = (s: string) => (s.trim() === "" ? null : s.trim());
  switch (kind) {
    case "view":
      return { kind, name: f.name, query: f.query, replace: f.replace, materialized: f.materialized };
    case "sequence": {
      const [start, increment, min, max, cache] = [f.start, f.increment, f.min, f.max, f.cache].map(whole);
      if ([start, increment, min, max, cache].includes(undefined)) return t("Sequence options must be whole numbers.");
      return {
        kind,
        name: f.name,
        start: start ?? null,
        increment: increment ?? null,
        min: min ?? null,
        max: max ?? null,
        cycle: f.cycle,
        cache: cache ?? null,
      };
    }
    case "function":
      return {
        kind,
        name: f.name,
        params: f.params.map((p) => ({ name: p.name, data_type: p.data_type, mode: text(p.mode) })),
        returns: f.returns,
        language: driver === "postgres" ? text(f.language) : null,
        body: f.body,
        replace: f.replace,
      };
    case "trigger":
      return {
        kind,
        name: f.name,
        table: f.table,
        timing: f.timing,
        events: f.events,
        for_each_row: f.forEachRow,
        when: text(f.when),
        body: f.body,
        replace: f.replace,
      };
  }
}

/** The statements as a script: each ends in `;` once (Oracle's blocks have theirs already). */
const asScript = (statements: string[]) => statements.map((s) => (s.endsWith(";") ? s : `${s};`)).join("\n\n");

/** Collects what a view, sequence, function or trigger needs, shows the statements that
 *  create it in the database's own syntax as the fields change, and runs them on the
 *  navigator session or opens them in the editor. */
export function ObjectFormDialog({ connection, database, schema, kind, table, onClose }: Props) {
  const driver = connection.driver;
  const timings = timingsOf(driver);
  const [f, setF] = useState<Fields>({
    name: "",
    replace: false,
    materialized: false,
    query: "",
    start: "",
    increment: "",
    min: "",
    max: "",
    cache: "",
    cycle: false,
    params: [],
    returns: "",
    language: "",
    body: "",
    table: table ?? "",
    timing: timings.includes("after") ? "after" : timings[0]!,
    events: ["insert"],
    forEachRow: true,
    when: "",
  });
  const [tables, setTables] = useState<string[]>(table ? [table] : []);
  /** The last result of the generator, with the fields it was generated for. */
  const [preview, setPreview] = useState<{ key: string; sql: string[] | null; error: string | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const set = <K extends keyof Fields>(key: K, value: Fields[K]) => setF((old) => ({ ...old, [key]: value }));

  const form = toForm(kind, f, driver);
  const key = JSON.stringify(form);
  const current = preview?.key === key ? preview : null;
  const sql = current?.sql ?? null;
  const shownError = typeof form === "string" ? form : (current?.error ?? null);
  const runnable = !!sql && !running && !connection.read_only;
  const db = driver === "duckdb";
  const rowTrigger = driver !== "mysql" && driver !== "sqlserver";

  useEffect(() => {
    if (kind !== "trigger") return;
    let live = true;
    void (async () => {
      try {
        const sid = await workbench().sideSession(connection, database);
        if (sid === null || !live) return;
        const objects = await api.listObjects(sid, schema);
        if (!live) return;
        const names = objects.filter((o) => o.kind === "table" || (o.kind === "view" && f.timing === "instead_of")).map((o) => o.name);
        setTables(names);
        setF((old) => (old.table === "" || !names.includes(old.table) ? { ...old, table: table && names.includes(table) ? table : (names[0] ?? "") } : old));
      } catch (e) {
        if (live) setError(toCommandError(e).message);
      }
    })();
    return () => {
      live = false;
    };
  }, [f.timing === "instead_of"]);

  useEffect(() => {
    if (typeof form === "string") return;
    let live = true;
    const timer = window.setTimeout(
      () =>
        api.objectFormSql(driver, schema, form).then(
          (statements) => live && setPreview({ key, sql: statements, error: null }),
          (e: unknown) => live && setPreview({ key, sql: null, error: toCommandError(e).message }),
        ),
      preview ? TYPING_PAUSE_MS : 0,
    );
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [key]);

  async function create() {
    if (!runnable) return;
    setRunning(true);
    setError(null);
    let done = 0;
    try {
      const sid = await workbench().sideSession(connection, database);
      if (sid === null) return;
      for (const statement of sql) {
        await api.executeStatement(sid, statement);
        done++;
      }
      requestNavigatorRefresh({ connectionId: connection.id, database, schema });
      onClose();
    } catch (e) {
      const message = toCommandError(e).message;
      setError(done > 0 ? `${message}\n${t("The first {count} statements ran and stay applied.", { count: done })}` : message);
      if (done > 0) requestNavigatorRefresh({ connectionId: connection.id, database, schema });
    } finally {
      setRunning(false);
    }
  }

  const text = (field: keyof Fields, label: string, extra?: { placeholder?: string; autoFocus?: boolean }) => (
    <label className="field">
      <span>{label}</span>
      <input
        name={field}
        value={f[field] as string}
        onChange={(e) => set(field, e.target.value as never)}
        autoFocus={extra?.autoFocus}
        placeholder={extra?.placeholder}
        spellCheck={false}
      />
    </label>
  );
  const area = (field: "query" | "body", label: string, placeholder: string) => (
    <label className="field">
      <span>{label}</span>
      <textarea name={field} rows={5} value={f[field]} onChange={(e) => set(field, e.target.value)} placeholder={placeholder} spellCheck={false} />
    </label>
  );
  const check = (field: "replace" | "materialized" | "cycle" | "forEachRow", label: string) => (
    <label className="check">
      <input type="checkbox" name={field} checked={f[field]} onChange={(e) => set(field, e.target.checked)} />
      <span>{label}</span>
    </label>
  );

  function setParam(i: number, patch: Partial<ParamRow>) {
    set(
      "params",
      f.params.map((p, j) => (j === i ? { ...p, ...patch } : p)),
    );
  }

  function toggleEvent(event: Event, on: boolean) {
    set("events", (["insert", "update", "delete"] as Event[]).filter((e) => (e === event ? on : f.events.includes(e))));
  }

  return (
    <div className="modal-backdrop" onMouseDown={() => !running && onClose()}>
      <form
        className="modal sql-action object-form"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && !running && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          void create();
        }}
      >
        <h2>{t(TITLES[kind])}</h2>
        <p className="muted small">{t("In {schema}. The statements below run on the connection and commit at once.", { schema })}</p>
        {text("name", t("Name"), { autoFocus: true })}

        {kind === "view" && (
          <>
            {area("query", t("Query"), "SELECT …")}
            {check("replace", t("Replace a view of this name"))}
            {(driver === "postgres" || driver === "oracle") && check("materialized", t("Materialized view"))}
          </>
        )}

        {kind === "sequence" && (
          <>
            {driver === "mysql" && <div className="notice warn small">{t("Sequences need MariaDB 10.3 or later.")}</div>}
            <div className="row">
              {text("start", t("Start"), { placeholder: "1" })}
              {text("increment", t("Increment"), { placeholder: "1" })}
            </div>
            <div className="row">
              {text("min", t("Minimum"))}
              {text("max", t("Maximum"))}
              {!db && text("cache", t("Cache"))}
            </div>
            {check("cycle", t("Start over at the limit (cycle)"))}
          </>
        )}

        {kind === "function" && (
          <>
            <div className="object-params">
              <span className="small">{t("Parameters")}</span>
              {f.params.map((p, i) => (
                <div className="row tight" key={i}>
                  <input
                    aria-label={t("Parameter name")}
                    name={`param-name-${i}`}
                    value={p.name}
                    placeholder={t("Name")}
                    spellCheck={false}
                    onChange={(e) => setParam(i, { name: e.target.value })}
                  />
                  {!db && (
                    <input
                      aria-label={t("Parameter type")}
                      name={`param-type-${i}`}
                      value={p.data_type}
                      placeholder={t("Type")}
                      spellCheck={false}
                      onChange={(e) => setParam(i, { data_type: e.target.value })}
                    />
                  )}
                  {(driver === "postgres" || driver === "oracle") && (
                    <select aria-label={t("Parameter mode")} name={`param-mode-${i}`} value={p.mode} onChange={(e) => setParam(i, { mode: e.target.value })}>
                      <option value="">{t("Default")}</option>
                      <option value="IN">IN</option>
                      <option value="OUT">OUT</option>
                      <option value="INOUT">INOUT</option>
                    </select>
                  )}
                  <button
                    type="button"
                    className="link"
                    aria-label={t("Remove parameter")}
                    onClick={() =>
                      set(
                        "params",
                        f.params.filter((_, j) => j !== i),
                      )
                    }
                  >
                    ✕
                  </button>
                </div>
              ))}
              <button type="button" className="link" name="add-param" onClick={() => set("params", [...f.params, { name: "", data_type: "", mode: "" }])}>
                {t("Add parameter")}
              </button>
            </div>
            <div className="row">
              {!db && text("returns", t("Returns"))}
              {driver === "postgres" && text("language", t("Language"), { placeholder: "plpgsql" })}
            </div>
            {area("body", db ? t("Expression") : t("Body"), db ? "a + b" : "RETURN …;")}
            {check("replace", t("Replace a function of this name"))}
          </>
        )}

        {kind === "trigger" && (
          <>
            <div className="row">
              <label className="field">
                <span>{t("Table")}</span>
                <select name="table" value={f.table} onChange={(e) => set("table", e.target.value)}>
                  {tables.map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>{t("Timing")}</span>
                <select name="timing" value={f.timing} onChange={(e) => set("timing", e.target.value as Timing)}>
                  {TIMINGS.filter((x) => timings.includes(x.value)).map((x) => (
                    <option key={x.value} value={x.value}>
                      {x.label}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <div className="row">
              {(["insert", "update", "delete"] as Event[]).map((event) => (
                <label className="check" key={event}>
                  <input type="checkbox" name={`event-${event}`} checked={f.events.includes(event)} onChange={(e) => toggleEvent(event, e.target.checked)} />
                  <span>{event.toUpperCase()}</span>
                </label>
              ))}
            </div>
            {rowTrigger && check("forEachRow", t("For each row"))}
            {rowTrigger && text("when", t("Only when (condition)"), { placeholder: "NEW.id IS NOT NULL" })}
            {area("body", t("Body"), "INSERT INTO log …;")}
            {check("replace", t("Replace a trigger of this name"))}
          </>
        )}

        <pre className="sql-preview">{sql ? asScript(sql) : "…"}</pre>
        {connection.read_only && (
          <div className="notice warn small">{t("{name} is read-only: this statement cannot be run.", { name: connection.name })}</div>
        )}
        {(shownError ?? error) && <div className="notice error small">{shownError ?? error}</div>}
        <div className="actions">
          <button
            type="button"
            className="link"
            name="open-in-editor"
            disabled={!sql}
            onClick={() => {
              if (sql) workbench().openTab("query", connection.id, { sql: asScript(sql), database });
              onClose();
            }}
          >
            {t("Open in editor")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" name="create" disabled={!runnable}>
            {running ? t("Running…") : t("Create")}
          </button>
        </div>
      </form>
    </div>
  );
}
