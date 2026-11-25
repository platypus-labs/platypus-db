// Every translated page of the guide has an English original and a well-formed source line, and
// the check tells a stale page from a current one (scripts/guide-translations.mjs).
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { languagesIn, report, sourceHash } from "./guide-translations.mjs";

test("every translated page has an original and a source line", () => {
  for (const lang of languagesIn()) {
    const r = report(lang);
    assert.deepEqual(r.orphans, [], `${lang}: pages with no English original`);
    assert.deepEqual(r.malformed, [], `${lang}: pages without a "<!-- source: <12 hex> -->" first line`);
  }
});

test("a page is missing, current, stale, malformed or an orphan", () => {
  const dir = mkdtempSync(join(tmpdir(), "guide-tr-"));
  try {
    const write = (path, text) => {
      mkdirSync(join(dir, path, ".."), { recursive: true });
      writeFileSync(join(dir, path), text);
    };
    for (const p of ["a", "b", "c", "d"]) write(`docs/user/guide/${p}.md`, `# ${p}\r\n`);
    write("docs/user/translations/it/guide/a.md", `<!-- source: ${sourceHash("# a\n")} -->\n# a\n`);
    write("docs/user/translations/it/guide/b.md", "<!-- source: 000000000000 -->\n# b\n");
    write("docs/user/translations/it/guide/c.md", "# c\n");
    write("docs/user/translations/it/guide/gone.md", "<!-- source: 000000000000 -->\n");
    assert.deepEqual(report("it", dir), { missing: ["d.md"], stale: ["b.md"], orphans: ["gone.md"], malformed: ["c.md"] });
    assert.deepEqual(languagesIn(dir), ["it"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
