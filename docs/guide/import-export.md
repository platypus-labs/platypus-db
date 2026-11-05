[← User guide](README.md)

# Import and export

The actions below are in the right-click menu of the navigator, and in the command palette
(`Ctrl+K`). The full list of formats and options is in
[FEATURES.md](../FEATURES.md#import-export-and-transfer).

## Import a file into a table

1. Right-click the table in the navigator, then **Data** › **Import data…**.
2. Choose the file: CSV, TSV, JSON, NDJSON, Parquet or Excel.
3. Check the preview and the mapping of the columns.
4. Choose whether to add the rows, replace them, or update those with the same key, and run it.

For a new table, right-click the schema and choose **Import data into a new table…**: the types
are those DuckDB infers from the file. **Paste CSV as a new table…** (same menu) imports the
clipboard.

Import is not offered on a read-only connection, on one that forbids data editing, or on
NoSQL databases.

## Export tables to a file

1. Right-click one table, or select several, then **Data** › **Export data…**. For every table of a
   schema, right-click the schema and choose **Export all tables…**.
2. Choose the format (CSV, JSON, NDJSON, Parquet, Excel, SQL `INSERT`, Markdown, HTML or XML) and
   the file.
3. Optionally compress it (gzip or zstd) or upload it to a bucket or a signed URL.

CSV cells that a spreadsheet would run as a formula get a leading `'` unless you turn off
**Formulas as text, for spreadsheets**. A connection can forbid exports (**Appearance and safety**
page); the action is then not offered. See [Safety](safety.md).

To take a query's result out, copy it from the grid as CSV, JSON, Markdown, HTML, SQL `INSERT`
or an `IN` list.

## Copy a table or a schema to another database

Right-click the tables and choose **Data** › **Copy tables to a database…**, or right-click the
schema and choose **Copy schema to a database…**. It works between any pair of databases; the
types are mapped.

## Run a SQL file, or dump to one

Right-click a connection or a database and choose **Run SQL file…**: it runs a `.sql` file (or `.sql.gz`, `.sql.zst`) of any size statement by statement,
with progress and Stop, by default in one transaction that a failure rolls back. **Dump to SQL file…**
(on a schema, or on tables under **Data**) writes the DDL of a schema or of chosen objects, and if wanted their rows, to one file.

## Save a transfer for later

Every export, import and copy dialog has **Save as profile…**. **Transfer profiles…** (command
palette) runs, renames, deletes and schedules them. A bucket's secret key is never saved in the
profile file.

## Query a data file without importing it

Drop a CSV, JSON, Parquet or Excel file on the window, or click **Open a data file** on the
welcome screen: it opens as a database (each Excel sheet is a table). The first time, the app
offers to download DuckDB's library.

## Import connections

On the welcome screen, **Import connections** reads DBeaver, DataGrip and pgAdmin (and
`pg_service.conf`); Db2 connections from DBeaver and DataGrip, and `jdbc:db2:` URLs, are read
too. **Export connections** writes a `.platypusconn` file for another computer: see
[Projects and sharing](projects-sharing.md).
