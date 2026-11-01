/** What a comparison script does to tables, by its statements: how many are created, altered
 *  and dropped, and how many statements drop anything (a column or an index too). */
export function scriptSummary(statements: string[]): { created: number; altered: number; dropped: number; drops: number } {
  const count = (re: RegExp) => statements.filter((s) => re.test(s)).length;
  return {
    created: count(/^\s*CREATE\s+(?:TEMP\w*\s+)?TABLE\b/i),
    altered: count(/^\s*ALTER\s+TABLE\b/i),
    dropped: count(/^\s*DROP\s+TABLE\b/i),
    drops: count(/\bDROP\b/i),
  };
}
