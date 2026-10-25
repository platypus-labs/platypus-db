import { useEffect, useState } from "react";
import { api, type ColumnMeta, type ForeignKeyMeta, type SavedConnection, toCommandError } from "../api";
import { GuardDialog } from "../guards/GuardDialog";
import { type GuardRequest, needsProductionConfirm } from "../guards/guards";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { workbench } from "../workbench";

/** How a column is filled, as the backend's `fake::Generator`; `skip` leaves it to its default. */
export type Generator =
  | { kind: "skip" }
  | { kind: "integer"; min: number; max: number }
  | { kind: "decimal"; min: number; max: number; scale: number }
  | { kind: "next_key" }
  | { kind: "reference"; schema: string | null; table: string; column: string }
  | { kind: "choice"; values: string[] }
  | { kind: "bool" | "first_name" | "last_name" | "full_name" | "email" | "city" | "phone" | "text" | "date" | "timestamp" | "uuid" | "null" };

type Kind = Generator["kind"];

const LABELS: Record<Kind, string> = {
  skip: "Default (not filled)",
  next_key: "Next key (after the largest)",
  reference: "Existing key of the referenced table",
  integer: "Whole number",
  decimal: "Decimal number",
  bool: "True or false",
  first_name: "First name",
  last_name: "Last name",
  full_name: "Full name",
  email: "Email",
  city: "City",
  phone: "Phone number",
  text: "Words",
  date: "Date",
  timestamp: "Date and time",
  uuid: "UUID",
  choice: "One of the enum's values",
  null: "NULL",
};

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  /** Whether `schema` is the session's current one, whose tables are named unqualified. */
  currentSchema: boolean;
  table: string;
  onClose: () => void;
}

type Status =
  | { state: "idle" }
  | { state: "running"; id: number | null; rows: number }
  | { state: "done"; rows: number }
  | { state: "failed"; message: string };

/** The generator of `kind` for `column`, with its default settings. */
function generator(kind: Kind, column: ColumnMeta, fk: ForeignKeyMeta | undefined): Generator {
  switch (kind) {
    case "integer":
      return { kind, min: 1, max: 1000 };
    case "decimal":
      return { kind, min: 0, max: 1000, scale: 2 };
    case "reference":
      return {
        kind,
        schema: fk!.ref_schema || null,
        table: fk!.ref_table,
        column: fk!.ref_columns[fk!.columns.indexOf(column.name)] ?? fk!.ref_columns[0]!,
      };
    case "choice":
      return { kind, values: column.enum_values };
    default:
      return { kind } as Generator;
  }
}

/** The generator a column most likely wants, from its keys, name and type. */
export function suggest(column: ColumnMeta, fk: ForeignKeyMeta | undefined): Kind {
  const name = column.name.toLowerCase();
  const type = column.data_type.toLowerCase();
  const integer = /int|serial|number/.test(type);
  if (fk) return "reference";
  if (column.default !== null) return "skip";
  if (column.primary_key) return integer ? "next_key" : /uuid|uniqueidentifier/.test(type) ? "uuid" : "next_key";
  if (column.enum_values.length > 0) return "choice";
  if (name.includes("email")) return "email";
  if (/first.?name/.test(name)) return "first_name";
  if (/last.?name|surname/.test(name)) return "last_name";
  if (name.includes("name")) return "full_name";
  if (name.includes("city")) return "city";
  if (/phone|tel/.test(name)) return "phone";
  if (/bool|^bit/.test(type)) return "bool";
  if (/timestamp|datetime/.test(type)) return "timestamp";
  if (/date/.test(type)) return "date";
  if (/uuid|uniqueidentifier/.test(type)) return "uuid";
  if (/numeric|decimal|real|float|double|money/.test(type)) return "decimal";
  if (integer) return "integer";
  return "text";
}

/** Adds fake rows to a table, each column filled by a generator picked for it. */
export function GenerateDataDialog({ connection, database, schema, currentSchema, table, onClose }: Props) {
  const [sid, setSid] = useState<number | null>(null);
  const [columns, setColumns] = useState<{ column: ColumnMeta; fk?: ForeignKeyMeta; generator: Generator }[]>([]);
  const [rows, setRows] = useState(100);
  const [status, setStatus] = useState<Status>({ state: "idle" });
  const [confirm, setConfirm] = useState<GuardRequest | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const id = await workbench().sideSession(connection, database);
        if (id === null) return;
        const details = await api.tableDetails(id, schema, table);
        setSid(id);
        setColumns(
          details.columns.map((column) => {
            const fk = details.foreign_keys.find((k) => k.columns.includes(column.name));
            return { column, fk, generator: generator(suggest(column, fk), column, fk) };
          }),
        );
      } catch (e) {
        setStatus({ state: "failed", message: toCommandError(e).message });
      }
    })();
  }, []);

  const filled = columns.filter((c) => c.generator.kind !== "skip");
  const qualified = currentSchema ? table : `${schema}.${table}`;

  function request() {
    if (!needsProductionConfirm(connection)) return void run();
    const names = filled.map((c) => c.column.name).join(", ");
    const production = [`INSERT INTO ${qualified} (${names}) VALUES ... -- ${rows} fake rows`];
    setConfirm({ connection, unfiltered: [], production, blocked: false });
  }

  async function run() {
    if (sid === null) return;
    setStatus({ state: "running", id: null, rows: 0 });
    const specs = filled.map((c) => ({ name: c.column.name, generator: c.generator }));
    try {
      const id = await api.fakeRows(sid, currentSchema ? null : schema, table, rows, specs, (event) => {
        if (event.type === "progress") setStatus((s) => (s.state === "running" ? { ...s, rows: event.rows } : s));
        else if (event.type === "failed") setStatus({ state: "failed", message: event.error.message });
        else if (event.type === "cancelled") setStatus({ state: "idle" });
        else {
          setStatus({ state: "done", rows: event.rows });
          requestNavigatorRefresh({ connectionId: connection.id, database, schema });
        }
      });
      setStatus((s) => (s.state === "running" ? { ...s, id } : s));
    } catch (e) {
      setStatus({ state: "failed", message: toCommandError(e).message });
    }
  }

  const running = status.state === "running";
  const close = () => !running && onClose();
  const choose = (i: number, kind: Kind) =>
    setColumns((cs) => cs.map((c, j) => (j === i ? { ...c, generator: generator(kind, c.column, c.fk) } : c)));
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal wide generate-data" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>Generate data for {table}</h2>
        <label className="field">
          <span>Rows</span>
          <input
            type="number"
            min={1}
            max={1_000_000}
            value={rows}
            disabled={running}
            onChange={(e) => setRows(Math.max(1, Math.min(1_000_000, Math.trunc(Number(e.target.value)) || 1)))}
          />
        </label>
        <div className="import-mapping two">
          <span className="muted small">Column</span>
          <span className="muted small">Filled with</span>
          {columns.map(({ column, fk, generator: g }, i) => (
            <GeneratorRow key={column.name} column={column} fk={fk} kind={g.kind} disabled={running} onChange={(k) => choose(i, k)} />
          ))}
        </div>
        {status.state === "running" && <div className="notice small">Adding… {status.rows.toLocaleString()} rows</div>}
        {status.state === "done" && <div className="notice small">Added {status.rows.toLocaleString()} rows.</div>}
        {status.state === "failed" && <div className="notice error small">Nothing was added: {status.message}</div>}
        <div className="actions">
          <span className="spacer" />
          {running ? (
            <button type="button" disabled={status.id === null} onClick={() => status.id !== null && void api.cancelExport(status.id)}>
              Stop
            </button>
          ) : (
            <button type="button" onClick={onClose}>
              {status.state === "done" ? "Close" : "Cancel"}
            </button>
          )}
          <button type="button" className="primary" disabled={running || sid === null || filled.length === 0} onClick={request}>
            Generate
          </button>
        </div>
      </div>
      {confirm && (
        <GuardDialog
          request={confirm}
          onAnswer={(go) => {
            setConfirm(null);
            if (go) void run();
          }}
        />
      )}
    </div>
  );
}

function GeneratorRow(props: {
  column: ColumnMeta;
  fk: ForeignKeyMeta | undefined;
  kind: Kind;
  disabled: boolean;
  onChange: (kind: Kind) => void;
}) {
  const { column, fk, kind, disabled, onChange } = props;
  const kinds = (Object.keys(LABELS) as Kind[]).filter(
    (k) => (k !== "reference" || fk) && (k !== "choice" || column.enum_values.length > 0),
  );
  return (
    <>
      <span>
        {column.name} <span className="muted small">{column.data_type}</span>
      </span>
      <select value={kind} disabled={disabled} aria-label={`Generator of ${column.name}`} onChange={(e) => onChange(e.target.value as Kind)}>
        {kinds.map((k) => (
          <option key={k} value={k}>
            {k === "reference" && fk ? `${LABELS[k]} (${fk.ref_table})` : LABELS[k]}
          </option>
        ))}
      </select>
    </>
  );
}
