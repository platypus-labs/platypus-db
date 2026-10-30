// The editor's dialect for CQL, the query language of Cassandra and ScyllaDB: SQL-like, with
// its own words (KEYSPACE, CLUSTERING, ALLOW FILTERING, USING TTL…), `$$ … $$` strings and `//`
// comments. Completion of the catalog's names and the snippets come from the shared sources
// (`language` in SqlEditor).
import { SQLDialect } from "@codemirror/lang-sql";

const KEYWORDS =
  "add aggregate all allow alter and any apply as asc authorize batch begin by called clustering " +
  "columnfamily compact contains create custom delete desc describe distinct drop each_quorum entries " +
  "exists filtering finalfunc from full function functions grant if in index infinity initcond input " +
  "insert into is json key keys keyspace keyspaces language limit list local_one local_quorum " +
  "materialized modify nan nologin norecursive nosuperuser not null of on one options or order " +
  "partition password per permission permissions primary quorum rename replace returns revoke role " +
  "roles schema select set sfunc static storage stype superuser table tables to token trigger " +
  "truncate ttl two type unlogged unset update use user users using values view views where with " +
  "writetime";

const TYPES =
  "ascii bigint blob boolean counter date decimal double duration float frozen inet int list map " +
  "set smallint text time timestamp timeuuid tinyint tuple uuid varchar varint vector";

const BUILTIN =
  "avg count cast currentdate currenttime currenttimestamp currenttimeuuid max maxtimeuuid min " +
  "mintimeuuid now sum todate tojson fromjson tounixtimestamp totimestamp token uuid writetime ttl";

export const CQL = SQLDialect.define({
  keywords: KEYWORDS,
  types: TYPES,
  builtin: BUILTIN,
  doubleDollarQuotedStrings: true,
  slashComments: true,
  caseInsensitiveIdentifiers: false,
});
