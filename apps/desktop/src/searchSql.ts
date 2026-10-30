// The statements the app writes for the indices of Elasticsearch and OpenSearch.

const MATCH_ALL = '{"match_all":{}}';

/** The search that reads the documents of `index` (`limit` of them at most), as the driver
 *  writes it for the data viewer: the grid edits what it finds. A `filter` is a query string
 *  (`status:open AND price:[10 TO 20]`). */
export function searchRequest(index: string, limit: number | null, filter = ""): string {
  const text = filter.trim();
  const query = text ? `{"query_string":{"query":${JSON.stringify(text)}}}` : MATCH_ALL;
  return `GET /${index}/_search\n{${limit === null ? "" : `"size":${limit},`}"query":${query}}`;
}
