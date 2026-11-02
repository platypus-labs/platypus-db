/** What a comparison script does to tables, by its statements: how many tables are created,
 *  altered and dropped, and how many statements drop anything (a column or an index too, not a
 *  default or a NOT NULL, which lose no data). */
export function scriptSummary(statements: string[]): { created: number; altered: number; dropped: number; drops: number } {
  const count = (re: RegExp) => statements.filter((s) => re.test(s)).length;
  // A table altered by several statements (a column added, another dropped) counts once.
  const altered = new Set(statements.map((s) => /^\s*ALTER\s+TABLE\s+(?:ONLY\s+)?(\S+)/i.exec(s)?.[1]?.toLowerCase()).filter(Boolean));
  return {
    created: count(/^\s*CREATE\s+(?:TEMP\w*\s+)?TABLE\b/i),
    altered: altered.size,
    dropped: count(/^\s*DROP\s+TABLE\b/i),
    drops: count(/\bDROP\b(?!\s+(?:DEFAULT|NOT\s+NULL|IDENTITY|EXPRESSION)\b)/i),
  };
}
