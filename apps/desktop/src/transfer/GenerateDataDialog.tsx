import { useEffect, useState } from "react";
import { api, isMongo, type ColumnMeta, type ForeignKeyMeta, type SavedConnection, type TableDetails } from "../api";
import { GuardDialog } from "../guards/GuardDialog";
import { type GuardRequest, needsProductionConfirm } from "../guards/guards";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { workbench } from "../workbench";
import { JobFooter, useJob } from "./job";
import { t, plural, msg } from "../i18n";
import { type Generator, isJson, JSON_KINDS, type Kind, MONGO_ONLY, numberRange, REPEATING, suggest } from "./generators";

const LABELS: Record<Kind, string> = {
  skip: msg("Default (not filled)"),
  next_key: msg("Next key (after the largest)"),
  reference: msg("Existing key of the referenced table"),
  integer: msg("Whole number"),
  decimal: msg("Decimal number"),
  bool: msg("True or false"),
  first_name: msg("First name"),
  last_name: msg("Last name"),
  full_name: msg("Full name"),
  email: msg("Email"),
  city: msg("City"),
  phone: msg("Phone number"),
  text: msg("Words"),
  word_list: msg("List of words"),
  document: msg("Document (name, city, phone)"),
  object_id: msg("New ObjectId"),
  date: msg("Date"),
  timestamp: msg("Date and time"),
  time: msg("Time of day"),
  ip: msg("IP address"),
  uuid: "UUID",
  choice: msg("One of the enum's values"),
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

/** The generator of `kind` for `column`, with its default settings: numbers within what its
 *  type holds. */
function generator(kind: Kind, column: ColumnMeta, fk: ForeignKeyMeta | undefined): Generator {
  const range = numberRange(column.data_type);
  switch (kind) {
    case "integer":
      return { kind, min: range.min, max: range.max };
    case "decimal":
      return { kind, min: range.min, max: range.max, scale: range.scale };
    case "reference":
      return {
        kind,
        schema: fk!.ref_schema || null,
        table: fk!.ref_table,
        column: fk!.ref_columns[fk!.columns.indexOf(column.name)] ?? fk!.ref_columns[0]!,
        // The columns of a composite key take their values from one referenced row.
        key: fk!.name,
      };
    case "choice":
      return { kind, values: column.enum_values };
    default:
      return { kind } as Generator;
  }
}

/** Whether `column` takes no value twice: a unique index or constraint of its own. */
function isUnique(details: TableDetails, column: string): boolean {
  const only = (columns: string[]) => columns.length === 1 && columns[0] === column;
  return (
    details.indexes.some((i) => i.unique && !i.primary && only(i.columns)) ||
    details.constraints.some((c) => c.kind === "unique" && only(c.columns))
  );
}

/** Adds fake rows to a table, each column filled by a generator picked for it. */
export function GenerateDataDialog({ connection, database, schema, currentSchema, table, onClose }: Props) {
  const mongo = isMongo(connection.driver);
  const [sid, setSid] = useState<number | null>(null);
  const [columns, setColumns] = useState<{ column: ColumnMeta; fk?: ForeignKeyMeta; unique: boolean; generator: Generator }[]>([]);
  const [rows, setRows] = useState(100);
  const { status, start, fail } = useJob();
  const [confirm, setConfirm] = useState<GuardRequest | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const id = await workbench().sideSession(connection, database);
        if (id === null) return;
        const details = await api.tableDetails(id, schema, table);
        // Identities and computed columns are the database's to fill: an identity refuses
        // values, and a numbered key would fall behind its sequence.
        const generated = mongo ? [] : await api.generatedColumns(id, schema, table).catch(() => []);
        setSid(id);
        setColumns(
          details.columns.map((column) => {
            const fk = details.foreign_keys.find((k) => k.columns.includes(column.name));
            // A collection's `_id` takes no value twice.
            const unique = isUnique(details, column.name) || (mongo && column.name === "_id");
            const kind = suggest(column, fk, unique, mongo, generated.some((g) => g.name === column.name));
            // A unique column with no generator of distinct values to suggest is left to its default.
            const usable = unique && REPEATING.includes(kind) ? "skip" : kind;
            return { column, fk, unique, generator: generator(usable, column, fk) };
          }),
        );
      } catch (e) {
        fail(e);
      }
    })();
  }, []);

  const filled = columns.filter((c) => c.generator.kind !== "skip");
  const qualified = currentSchema ? table : `${schema}.${table}`;

  function request() {
    if (!needsProductionConfirm(connection)) return void run();
    const names = filled.map((c) => c.column.name).join(", ");
    const count = plural(rows, "{n} fake row", "{n} fake rows");
    const collection = `db.${currentSchema ? "" : `getSiblingDB(${JSON.stringify(schema)}).`}getCollection(${JSON.stringify(table)})`;
    const production = [mongo ? `${collection}.insertMany([...]) // ${count}` : `INSERT INTO ${qualified} (${names}) VALUES ... -- ${count}`];
    setConfirm({ connection, unfiltered: [], production, blocked: false });
  }

  async function run() {
    if (sid === null) return;
    // The type picks the BSON type of a MongoDB field's numbers.
    const specs = filled.map((c) => ({ name: c.column.name, generator: c.generator, unique: c.unique, data_type: c.column.data_type }));
    const refresh = () => requestNavigatorRefresh({ connectionId: connection.id, database, schema });
    // MongoDB has no transactions: the documents of a failed or stopped run stay.
    await start((onEvent) => api.fakeRows(sid, currentSchema ? null : schema, table, rows, specs, onEvent), refresh, mongo ? refresh : undefined);
  }

  const failed = mongo ? (n: number) => t("Added at least {n} documents, then stopped: ", { n: n.toLocaleString() }) : t("Nothing was added: ");

  const running = status.state === "running";
  const close = () => !running && onClose();
  const choose = (i: number, kind: Kind) =>
    setColumns((cs) => cs.map((c, j) => (j === i ? { ...c, generator: generator(kind, c.column, c.fk) } : c)));
  /** Sets a bound of the numbers of column `i`. */
  const bound = (i: number, which: "min" | "max", value: number) =>
    setColumns((cs) =>
      cs.map((c, j) => (j === i && (c.generator.kind === "integer" || c.generator.kind === "decimal") ? { ...c, generator: { ...c.generator, [which]: value } } : c)),
    );
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal wide generate-data" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{t("Generate data for {table}", { table })}</h2>
        <label className="field">
          <span>{t("Rows")}</span>
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
          <span className="muted small">{t("Column")}</span>
          <span className="muted small">{t("Filled with")}</span>
          {columns.map(({ column, fk, unique, generator: g }, i) => (
            <GeneratorRow
              key={column.name}
              column={column}
              fk={fk}
              unique={unique}
              mongo={mongo}
              generator={g}
              disabled={running}
              onChange={(k) => choose(i, k)}
              onBound={(which, v) => bound(i, which, v)}
            />
          ))}
        </div>
        {connection.read_only && <div className="notice warn small">{t("{name} is read-only: nothing is written there.", { name: connection.name })}</div>}
        <JobFooter status={status} messages={[msg("Adding… {n} rows"), msg("Added {n} rows.")]} 
          failed={failed}
          stopped={mongo ? msg("Stopped after {n} documents.") : msg("Stopped: nothing was added.")}
          onClose={onClose}
        >
          <button type="button" className="primary" disabled={running || sid === null || filled.length === 0 || connection.read_only} onClick={request}>
            {t("Generate")}
          </button>
        </JobFooter>
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
  unique: boolean;
  mongo: boolean;
  generator: Generator;
  disabled: boolean;
  onChange: (kind: Kind) => void;
  onBound: (which: "min" | "max", value: number) => void;
}) {
  const { column, fk, unique, mongo, generator: g, disabled, onChange, onBound } = props;
  const json = isJson(column.data_type.toLowerCase());
  const kinds = (Object.keys(LABELS) as Kind[]).filter(
    (k) =>
      (k !== "reference" || fk) &&
      (k !== "choice" || column.enum_values.length > 0) &&
      !(unique && REPEATING.includes(k)) &&
      (mongo || !MONGO_ONLY.includes(k) || (json && JSON_KINDS.includes(k))),
  );
  const number = (which: "min" | "max", value: number, label: string) => (
    <input
      type="number"
      className="generator-bound"
      value={value}
      disabled={disabled}
      aria-label={t(label, { column: column.name })}
      onChange={(e) => Number.isFinite(e.target.valueAsNumber) && onBound(which, e.target.valueAsNumber)}
    />
  );
  return (
    <>
      <span>
        {column.name} <span className="muted small">{unique ? t("{type}, unique", { type: column.data_type }) : column.data_type}</span>
      </span>
      <span className="generator">
        <select value={g.kind} disabled={disabled} aria-label={t("Generator of {column}", { column: column.name })} onChange={(e) => onChange(e.target.value as Kind)}>
          {kinds.map((k) => (
            <option key={k} value={k}>
              {k === "reference" && fk ? `${t(LABELS[k])} (${fk.ref_table})` : t(LABELS[k])}
            </option>
          ))}
        </select>
        {(g.kind === "integer" || g.kind === "decimal") && (
          <>
            {number("min", g.min, msg("Least value of {column}"))}
            {number("max", g.max, msg("Greatest value of {column}"))}
          </>
        )}
      </span>
    </>
  );
}
