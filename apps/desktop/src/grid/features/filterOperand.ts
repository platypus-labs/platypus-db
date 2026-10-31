// What a quick filter compares a value with: the column itself, or its text or bytes where
// the database has no equality for the type. Pure, so that it is tested on its own.
import type { DriverKind } from "../../api";

/** PostgreSQL types without `=` (json, xml, the geometric ones), and arrays of them. */
const PG_TEXT = /^(json|xml|point|line|lseg|box|path|polygon|circle)(\[\])?$/i;
/** SQL Server types that `=` does not take, compared as text or as bytes: the spatial and other
 *  CLR types are shown as their bytes (`hierarchyid`, shown as its path, compares with it). */
const MSSQL_TEXT = /^(n?text|xml|sql_variant)$/i;
const MSSQL_BYTES = /^(image|udt|geography|geometry)$/i;
/** MySQL and MariaDB `float` is single precision: the stored 0.1 is 0.10000000149..., which a
 *  literal 0.1 (read as a double) never equals. Its text is the shortest one, the grid's. */
const MYSQL_FLOAT = /^float\b/i;

/**
 * The expression a condition on column `name` (already quoted) of type `dbType` compares
 * with the literal of a cell: `doc::text` for a PostgreSQL `json`, whose cells hold its text;
 * `CAST(n AS nvarchar(max))` for a SQL Server `ntext` or `sql_variant` (which would compare
 * a number with the text and find nothing); the bytes of a SQL Server CLR value. The column
 * itself everywhere else, so that its indexes still serve.
 */
export function filterOperand(driver: DriverKind, dbType: string, name: string): string {
  const type = dbType.trim();
  if (driver === "postgres" && PG_TEXT.test(type)) return `${name}::text`;
  if (driver === "sqlserver" && MSSQL_TEXT.test(type)) return `CAST(${name} AS nvarchar(max))`;
  if (driver === "sqlserver" && MSSQL_BYTES.test(type)) return `CAST(${name} AS varbinary(max))`;
  if (driver === "mysql" && (/^json$/i.test(type) || MYSQL_FLOAT.test(type))) return `CAST(${name} AS CHAR)`;
  return name;
}
