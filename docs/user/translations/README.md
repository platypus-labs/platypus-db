# Translations of the user guide

The guide in `docs/user/guide/` is written in English. A page translated into another language
lives at `docs/user/translations/<lang>/guide/<same path>.md`, where `<lang>` is the app's language
code (`it`, `es`, `de`, `fr`, `pt`, `zh`):

```
docs/user/guide/getting-started.md                       English
docs/user/translations/it/guide/getting-started.md       Italian
docs/user/guide/databases/postgresql.md
docs/user/translations/it/guide/databases/postgresql.md
```

The Help dialog shows the page in the language of the app when it has a translation, and the
English page when it has not, page by page. The site builds the translated pages at
`docs/<lang>/guide/` and lists the languages of each page at its top. The screenshots are the
English ones.

## Rules for a translated page

- **First line**: `<!-- source: <12 hex> -->`, the first 12 hex digits of the SHA-256 of the
  English page this translation follows. `node scripts/guide-translations.mjs --hash <english file>`
  prints it. When the English page changes, the hash no longer matches and the page is listed as
  stale: update the translation, then write the new hash.
- **Links and images**: the relative targets stay those of the English page (`connect.md`,
  `../FEATURES.md`, `../../screenshots/guide/welcome.png`), read as if the page were at the
  English location. Only the anchor changes: a link to a heading uses the anchor of the
  *translated* heading (`troubleshooting.md#aggiornamenti`), and a unit test checks that every
  anchor leads to a heading.
- **Same structure**: the same headings, lists, tables and code blocks in the same order.
- **Code stays as it is**: SQL, commands, option names, file paths, setting keys and keyboard
  shortcuts are not translated, and neither are quoted error messages from the databases.
- **Labels follow the app**: a button, menu, field or dialog name is the one in
  `apps/desktop/src/i18n/<lang>.ts` (look up the English text, use its value). A label the app
  does not translate stays in English.
- **Words people use in English** (query, driver, database, schema, commit, tunnel SSH…) stay in
  English in languages where they are normal.

`node scripts/guide-translations.mjs` lists, per language, the pages that are missing, stale or
malformed; it never fails the build.
