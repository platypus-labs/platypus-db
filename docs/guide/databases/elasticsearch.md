[← Databases](README.md)

# Elasticsearch and OpenSearch

One driver talks to both over their HTTP API (`reqwest` with rustls). The server says which it is
when you connect, and the connection shows it (`Connected: OpenSearch 2.17.1`). The driver is part
of the app; nothing is downloaded.

## Connect

| You paste | Read as |
|---|---|
| `elasticsearch://user:pass@host:9200/logs-*` | plain HTTP; the path is an index pattern |
| `elasticsearchs://user:pass@host:9200` | HTTPS (verified: `verify-full`) |
| `opensearch://…` and `opensearchs://…` | the same, remembered as OpenSearch so the URL is written back that way |

The default port is 9200. The **Database** field is an optional index pattern (`logs-*`, or
several separated by commas) that narrows what the navigator lists; empty lists every index that
is not hidden.

- **Signing in**: a user and password use Basic authentication. With the user empty, the password
  field is an **API key** (the `encoded` value), sent as `Authorization: ApiKey …`.
- **TLS**: HTTPS for `elasticsearchs://` and `opensearchs://` URLs and for SSL modes from
  `require` up; `elasticsearch://` and `prefer` are plain HTTP. `require` does not check the
  certificate; `verify-ca` and `verify-full` do. A CA file and a client certificate go in the
  TLS fields.

## Privileges

The ones the requests need. The navigator reads the indices, aliases, data streams and mappings
the login can see; there are no administration tabs.

## What works

- **Requests as Kibana's Dev Tools console writes them**: a line such as `GET /orders/_search`
  (the slash is optional; methods `GET`, `POST`, `PUT`, `DELETE`, `HEAD`), then its JSON body.
  `_bulk` and `_msearch` take NDJSON bodies that end at a blank line or the next request.
- **SQL**: anything else is SQL (`_sql` on Elasticsearch, `_plugins/_sql` on OpenSearch), with
  typed columns and the server's cursor for long results. Errors keep the server's reason and an
  SQL error points at its place in the statement.
- A search gives documents as MongoDB's results do: a column for each top-level field, with the
  focused document as a tree (`F7`). Aggregations come as results of their own.
- The navigator lists indices as tables (health, shards and replicas in the tooltip) and aliases
  and data streams as views. An index opens as a search of all its documents, read 1,000 rows
  at a time with a point in time and `search_after` (a scroll on OpenSearch), so reading does
  not stop at 10,000 hits. A field with no mapped column goes into a last `_other` column as JSON.
- **The grid edits documents**: a changed cell is an `_update` by `_id`, a new row is a `_create`
  (an `_id` that exists is refused by the server) or a generated document, a deleted row is a
  `DELETE`. The preview shows the requests, which run one after the other; one the server
  refuses stops the rest, and the earlier ones stay. *Undo script* puts the old `_source` back
  (a document with a generated `_id` has to be deleted by hand).
- `_delete_by_query` and `_update_by_query` without a query (or `match_all`), and
  `DELETE /_all`, ask first like a `DELETE` without `WHERE`. Read-only connections run only the
  requests and SQL that read.
- Cancel stops a search on the server. An index's object DDL is the request that creates it again.
- The details are in [FEATURES.md](../../FEATURES.md#elasticsearch-and-opensearch).

## What does not

- No transactions (no manual commit) and no explain.
- No SQL tools: no table editor, ER diagram, schema comparison, query builder, SQL import,
  snapshots before changes, or terminal mode.
- The data viewer has no filter and sort bar.

## Agents

`run_query` and `sample_rows` work with console requests and SQL, and a request that changes data
is asked about first. Refused on Elasticsearch and OpenSearch: `explain`, `get_er_diagram`,
`propose_edits` and `query_across_databases`. See [Agents and the CLI](../../AGENTS.md).

## Known limits

- The app refuses Elasticsearch older than 7.10 and OpenSearch older than 1.0: unlike the other
  databases, the connection fails with "older than … the oldest version supported".
- Tested on Elasticsearch 8 and OpenSearch 2, not on 7.x.

## Tested on

The conformance suite runs on Elasticsearch 8.15.3 (login `elastic`) and OpenSearch 2.17.1: server
info, SQL with types and paging, searches that page past one page, the catalog and mappings,
saving the grid's changes and undoing them, cancel of a slow search, and the server's errors.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
