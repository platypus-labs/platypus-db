// QA4 grid, PostgreSQL (medium: wrong data shown). The time 24:00:00 ("end of day", valid in
// PostgreSQL and used for closing times) is shown in the grid and copied as the error text
// "ERROR: Cast error: Failed to convert 86400000000 to temporal for Time64(µs)" (same family as
// qa-grid-pg-infinity). The same text goes into copies, exports and undo scripts.
// Steps: CREATE TABLE with a time column holding '24:00:00' and '23:59:59.999999'; open it; copy all.
// Expected: 24:00:00 as psql prints it. Actual: the Arrow cast error text.
// Code: apps/desktop/src-tauri/src/display.rs (render via arrow ArrayFormatter) on the Time64
// value 86_400_000_000 that the PostgreSQL driver produces; needs the special-casing that
// infinity already has.
import { assert, connections, copyAll, needs, openEditor, run, sh, stubClipboard } from "../qa-grid.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP TABLE IF EXISTS public.qa4g_t; CREATE TABLE public.qa4g_t (id int PRIMARY KEY, closes time, closes_tz timetz);
    INSERT INTO public.qa4g_t VALUES (1, '23:59:59.999999', '23:59:59+02'), (2, '24:00:00', '24:00:00+00');`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await openEditor(page, "QA PG", "pg", "public");
    await run(page, "SELECT * FROM public.qa4g_t ORDER BY id");
    await stubClipboard(page);
    const text = await copyAll(page);
    assert.ok(!/ERROR|Cast error/.test(text), text);
    assert.match(text, /^2\t24:00:00\t24:00:00/m);
  } finally {
    sh("pg", "DROP TABLE IF EXISTS public.qa4g_t");
  }
}
