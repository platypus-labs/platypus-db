import type { DriverKind } from "../api";

/** The product names, for messages (`DriverKind::name` in Rust). */
export const DRIVER_NAMES: Record<DriverKind, string> = {
  postgres: "PostgreSQL",
  mysql: "MySQL",
  sqlite: "SQLite",
  duckdb: "DuckDB",
  sqlserver: "SQL Server",
  clickhouse: "ClickHouse",
  libsql: "libSQL",
  mongo: "MongoDB",
  redis: "Redis",
  snowflake: "Snowflake",
  bigquery: "BigQuery",
  databricks: "Databricks",
  trino: "Trino",
  adbc: "ADBC",
  oracle: "Oracle",
  db2: "Db2",
  odbc: "ODBC",
};
