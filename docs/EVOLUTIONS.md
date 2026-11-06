# PlatypusDB — Evolutions

Ideas and evolutions for after the [roadmap](ROADMAP.md). They follow the same logic: **independent
groups**, each with its own area of code, which several people can work on in parallel.
They are taken once every roadmap group is `✅ completed`, `✅ done (…)` with nothing left to
pick, `🔒`, `⏸ waiting` or `⛔ blocked`. The procedure is the one in [CLAUDE.md](../CLAUDE.md),
with these differences: the lock lives in the "Evolution locks" table below and the branch is
named `evo-<letter>-<topic>` (e.g. `evo-a-agents`).

Sources: user requests and the known limits in the roadmap's group sections, checked against
what the app already does so that no row repeats it. **Fixes are not evolutions**: a problem
found while testing or working on something else goes in the roadmap's "Correzioni aperte"
(open fixes) table, as the limits left by the group S testing did.

**Priority**
- **P1**: many users need it, or a basic function is missing.
- **P2**: important, but it can wait.
- **P3**: niche, polish, or to be evaluated.

In the **Origin** column:
- **D**: present in DBeaver Community.
- **D+**: only in the paid DBeaver editions.
- **N**: not in DBeaver.
- **U**: requested by the user.

In the first column of the tables:
- ✅: done.
- 🆕: added after this file was created, still to do.

---

## 1. Rules

The rules of [ROADMAP.md](ROADMAP.md) apply: "Rules for all groups" (own area, database features
through the `Dialect` or driver capabilities, conformance tests, continuous refactoring) and the
Definition of Done at the end of that file. The `⏭ needs a Mac` and `⛔ blocked` rows keep their
meaning: they are not taken and do not count towards `✅ completed`. Rows that depend on a user
choice are marked `to decide`: ask first, then implement.

Before adding a type, a `Connection` trait method or a module in `platypus-core` or
`platypus-sql`, check that it does not exist yet (see [CLAUDE.md](../CLAUDE.md)). The roadmap's
"Open fixes" always come first.

---

## 2. Work groups

### Evolution locks

Whoever works on a group marks it here and pushes to `main` before writing code. Format:
`🔒 <name> · <branch> · <YYYY-MM-DD>`. A lock with no commit on its branch for more than 3 days
has expired. `✅ completed` and `✅ done (…)` work as in the roadmap.

| Group | Lock |
|---|---|
| A — Agents and MCP | ✅ completed |
| B — Plugins and mods | 🔒 Delfa · evo-b-plugins · 2026-11-06 |
| C — Kerberos and authentication | 🔒 Delfa · evo-c-kerberos · 2026-11-06 |
| D — User documentation | ✅ done (P2; P3 in part) |
| E — Publishing and distribution | free |

### Group A — Agents and MCP

**Status: completed.** Remote MCP (the `/mcp` endpoint reachable from other machines, with TLS and OAuth 2.1) was dropped on 2026-11-05: it would expose saved connections on the network, and no concrete use asks for it.

**What exists** (roadmap group N, [AGENTS.md](AGENTS.md)): the server (`platypus-db --mcp` on
stdio, `/mcp` over HTTP through the bridge with the open app) with the tools `list_connections`,
`list_schemas`, `list_objects` (pages), `describe_table` (with database comments),
`get_ddl`, `search_objects` (by name), `run_query` (limits on rows, bytes and time,
cancellation), `sample_rows`, `explain`, `list_scripts`, `read_script`, `search_history`,
`list_open_sessions`, `get_user_context` (on consent), `propose_sql` and `get_er_diagram` (Mermaid); the schema as
resources (`platypus://<connection>/<schema>/<table>`); access per connection (off, read only,
read and write; Prod never more than read only) sealed in the keychain; read-only transactions
and the statement check; confirmation in the app for every change; sensitive values masked in
scripts, history and errors; the call log with CSV export; the Claude Code plugin and the Codex
configuration. The rows below add only what is not there.

**Area:** `crates/platypus-mcp` (tools, prompts, resources), `apps/desktop/src-tauri/src/agent/` (bridge to the open app), `apps/desktop/src/agents/` (settings, confirmations, activity); reused, not rewritten: `platypus-docs` (Mermaid), `apps/desktop/src/erd/` (SVG export), `platypus-schema` (schema compare), `platypus-local` (table profile, queries across databases)
**Depends on:** roadmap group N (completed); groups I, J, M and Q (completed) for the features reused

| Feature | Prio | Origin |
|---|---|---|
| ✅ MCP tool `get_er_diagram`, Mermaid part: the `erDiagram` of a schema (tables, columns, primary keys, FKs) built from the catalog with `platypus-docs` (`mermaid.rs`), so it works with the app closed; cut at the byte limit with a note | P2 | U |
| ✅ `get_er_diagram` with a list of tables and FK depth (the tables linked to the chosen ones, up to 3 hops, in both directions), to avoid a huge diagram on large schemas | P3 | U |
| ✅ Tests for the Mermaid tool: unit tests of the FK hops, stdio on SQLite, and the conformance suite on PostgreSQL, MySQL and SQLite (tables, PKs, FKs, names with spaces) | P2 | U |
| ✅ `get_er_diagram` as SVG, like the app's export (`apps/desktop/src/erd/`), requested from the open app as `get_user_context` does (`/v1/context`); a `format` parameter picks between Mermaid and SVG. Needs the app open; with an E2E scenario for the request | P3 | U |
| ✅ **Sensitive columns masked in results**: masking covers `read_script` and `search_history` only; `run_query`, `sample_rows` and `get_user_context` return as they are the values of the columns marked sensitive in `catalog.db` or matched by the Safety rules. Mask them as the grid does (`••••••`, NULL left NULL), with a per-connection choice in Settings → Agents → Security to show them | P1 | N |
| ✅ **Catalog for agents**: `describe_table`, `list_objects` and the table resource also return the user's notes, tags and sensitive marks from `catalog.db` (today only the database comment), and `search_objects` also matches comments, notes and `#tag`, so an agent finds "the invoices table" without knowing its name | P2 | N |
| ✅ Tool `compare_schemas`: the differences between two schemas (same or different connection, as the app's schema compare of roadmap group I) and the migration script as text; the script reaches the user only through `propose_sql`, the tool runs nothing | P2 | U |
| ✅ Tool `profile_table`: per column NULLs, distinct values, min/max, top values and patterns, from the app's table profile (`platypus-local::table_profile`), on a row sample and within the `run_query` timeout; sensitive columns give counts only, no values | P2 | U |
| ✅ **Dry run of a change**: tool `estimate_change` returns, without running anything, what the confirmation dialog would show the user (rows the database expects to change, plan, the Safety checks that would ask), so the agent corrects a statement before asking | P2 | U |
| ✅ **Per-agent limits and cost guard**: per connection in Settings → Agents → Security, the maximum rows, bytes and timeout an agent may ask for (today fixed caps: 10,000 rows, 1 MB, 300 s), calls per minute, and a threshold on estimated rows or plan cost above which a read is refused with the plan in the error | P2 | U |
| ✅ Read-only server tools `server_activity` (sessions, running queries, locks) and `top_queries` (`pg_stat_statements`, `performance_schema`, Query Store), from the views of roadmap group J; no kill or cancel | P2 | U |
| ✅ **Edits proposed by agents**: tool `propose_edits` (rows to insert, update or delete in a table, by key) opens them in the app's data viewer as pending edits, shown as a diff in the grid, which the user saves or discards; like `propose_sql`, it changes nothing by itself | P2 | U |
| ✅ **Large results in pages**: `run_query` returns a `cursor` to read the next pages of the same result (kept a few minutes, closed with the session) instead of running the query again with a larger `max_rows` | P3 | U |
| ✅ **MCP prompts** (`prompts/list`): ready tasks that chain the existing tools (explain and improve a query, find the slowest queries, write a migration for a change, document a table), and the user's saved scripts with `:parameters` as prompts with arguments | P3 | U |
| ✅ **Schema change notifications**: `resources/subscribe` on the schema resources and `notifications/resources/list_changed` when the app sees a change (navigator refresh, schema timeline of group I), so a long agent session does not work on an old schema | P3 | U |
| ✅ Tool `query_across_databases`: one read-only statement over tables of several connections through the DuckDB join of roadmap group M (`platypus-local`), each source checked against its own agent access | P3 | U |
| ✅ **Agent report**: in Settings → Agents → Activity a summary per agent and session (tables read, rows returned, changes allowed and refused, errors), exported as Markdown or HTML next to today's CSV of single calls | P3 | U |

---

### Group B — Plugins and mods

**Status: to do.**

**Area:** `crates/platypus-plugins`, `apps/desktop/src/plugins/`, [PLUGINS.md](PLUGINS.md); existing hook points in [EXTENDING.md](EXTENDING.md): action registry, `registerTabKind`, `GridSourceExtensions`, settings sections, theme
**Depends on:** WASM plugins from roadmap group G (actions and drivers, ABI 1)

Today a plugin adds actions and a driver (HTTP). Here the ABI grows so that add-ons attach to the
app without touching its source. Each new capability is a permission approved at install time,
like `http` and `clipboard`; the changes do not break ABI 1 (`api = 2`, backward compatible).

| Feature | Prio | Origin |
|---|---|---|
| 🆕 **Plugin panels and tab kinds**: a plugin declares a tab (`kind`) or a side panel; the content is an HTML document in an isolated iframe that talks to the app by messages, registered through `registerTabKind` | P2 | U |
| 🆕 **Themes as plugins**: a package with only colour tokens and fonts (`kinds = ["theme"]`), no WASM module, which shows up in the theme picker | P2 | U |
| 🆕 **Event hooks**: before and after a query, on connect, on saving edits; a "before" hook can block with a message, and goes through the Safety checks (group L) | P2 | U |
| 🆕 **Grid extensions**: a plugin adds computed columns, cell renderers or cell actions through `GridSourceExtensions` | P3 | U |
| 🆕 **Plugin settings page**: field schema in the manifest, drawn by the app in Settings and saved in the store, read by the module | P2 | U |
| 🆕 **Catalog and installer**: a list of plugins from an index (JSON) with download, SHA-256 hash and package signature checks, update and removal | P2 | U |
| 🆕 Documentation and an example plugin in `examples/plugins` for each new point, with a test of the denied permission | P2 | U |

---

### Group C — Kerberos and authentication

**Status: to do.** The Kerberos row of roadmap group F stays as it is (`⛔ blocked`); this group
holds the plan to get past it.

**Area:** `vendor/tokio-postgres`, `crates/platypus-driver-pg` (and `platypus-core` for `Auth`), `docker/compose.yml` (test KDC), `crates/platypus-driver-mysql` for the blocked row
**Depends on:** roadmap group F (completed)

| Feature | Prio | Origin |
|---|---|---|
| 🆕 **Kerberos/GSSAPI for PostgreSQL**: in the `tokio-postgres` copy already maintained in `vendor/`, handle the GSS authentication messages with `cross-krb5` (SSPI on Windows, GSSAPI elsewhere); a new `Auth` value (`kerberos`) and a field for the service name. Optional Cargo feature, so those who do not want it do not depend on the Kerberos libraries | P2 | D+ |
| 🆕 Test with a KDC in Docker (a `kdc` service and PostgreSQL with `gss` in `pg_hba.conf`, a `docker/compose.yml` profile), in the conformance suite; skipped without a ticket or KDC | P2 | D+ |
| 🆕 Ticket from an external `kinit` or from a keytab (`KRB5CCNAME`, `KRB5_CLIENT_KTNAME`) and a clear message when it is missing; `sign_in` for `psql` too | P3 | D+ |
| ⛔ blocked — **Kerberos for MySQL and MariaDB**, MariaDB's `auth_gssapi` first (MySQL's Kerberos plugin is Enterprise only). `mysql_async` has no hook for authentication plugins: that comes first (a patch or a copy in `vendor/`) | P3 | D+ |

---

### Group D — User documentation

**Status: P2 done, in P3 help in the app, the link check and the guide's screenshots done; to do: the split of `docs/`, the shorter READMEs, the site's pages from `docs/`, translations.**

**What exists:** [README.md](../README.md) and [README.it.md](../README.it.md) (the same ten
sections: why, install, databases, features, agents, performance, development, contributing,
security, licence); [FEATURES.md](FEATURES.md), the reference of every feature area by area,
with a table of databases and one line of notes each; [SHORTCUTS.md](SHORTCUTS.md);
[AGENTS.md](AGENTS.md), complete for agents and the CLI; [PLUGINS.md](PLUGINS.md); the developer
documents (DEVELOPMENT, EXTENDING, RELEASING, BENCHMARKS) and the internal ones in Italian
(ROADMAP, COVERAGE, COLLAUDO, INVENTORY, MVP_PLAN), all side by side in `docs/` with the index
[docs/README.md](README.md); `docs/screenshots/` made by the E2E `readme` and `gifs` scenarios;
CONTRIBUTING and CHANGELOG at the root. The site `site/` has home, features, databases,
compare, download and docs, where `docs.html` holds the only getting started (install, first
connection, sample database, first keys) and links the Markdown on GitHub. In the app there is
no help entry nor any link to the documentation; the interface speaks seven languages, the
documentation English (the README also Italian).

**Area:** `docs/`, `README.md` and `README.it.md`, `site/`, `scripts/`; for in-app help, `apps/desktop/src/shell/` and the action registry
**Depends on:** nothing

| Feature | Prio | Origin |
|---|---|---|
| ✅ A **user guide by task** in `docs/guide/`: connect (URL, SSH, TLS, keychain), run queries and read results, edit data, import and export, safety (Prod, read-only, confirmations), projects and sharing, agents; steps and a screenshot per page, with links to FEATURES.md, which stays the reference by area and not a path to follow | P2 | U |
| ✅ **Getting started** in the repository, as the guide's first page: today it is only in `site/docs.html`; the text moves to `docs/guide/` and the site page comes from it | P2 | U |
| ✅ **A page per database**: how to connect, privileges needed, what works and what does not (editing, explain, transactions, administration), drivers downloaded on first use (DuckDB, ADBC), ODBC and Db2 on Windows; from the one-line notes of the FEATURES.md table and `site/databases.html` | P2 | U |
| ✅ **Troubleshooting and FAQ**, missing today: connection errors (TLS, SSH, proxy, keychain and master password), a DuckDB or ADBC download that fails, an agent that does not see a connection (access off, master password, password asked every time), the portable copy, where the data folder and the logs are, updates | P2 | U |
| ✅ The guide's screenshots made by E2E scenarios, as `readme` and `gifs` do for `docs/screenshots/`, so they are redone when the interface changes; a script copies them into `site/img/screenshots/` (by hand today, see `site/README.md`) | P3 | U |
| ✅ **Help in the app**: a Help entry in the command palette and the menu (guide, shortcuts, report a bug, what is new from CHANGELOG) and "?" links to the guide page from the connection form, Settings → Agents and Safety | P3 | D |
| 🆕 User documentation apart from developer documentation: `docs/user/` (guide, features, shortcuts, agents, plugins) and `docs/dev/` (development, extending, releasing, benchmarks, and the internal documents in Italian), with docs/README.md and the links of the READMEs, the site and the code updated | P3 | U |
| 🆕 A shorter README, in English and Italian: what it is, why, install, a link to the guide; the databases, features and agents sections, already in the guide and FEATURES.md, shrink to a few lines. A check that the two READMEs have the same sections | P3 | U |
| 🆕 **Site docs pages from `docs/`**: a script in `scripts/` (no new build tool) turns the guide's Markdown into pages in the site's style, instead of `site/docs.html` linking the Markdown on GitHub; publishing the site is in group E | P3 | U |
| 🆕 **Translations of the guide**. **To decide**: which of the app's languages and which pages (getting started first) | P3 | U |
| ✅ Automatic check of relative links between documents and of the site's links (`scripts/`), run by hand | P3 | N |

---

### Group E — Publishing and distribution

**Status: to do.** Only what is not already in roadmap group P, which keeps Windows signing,
Linux verification with a GPU, the DBeaver benchmark, the launch and the repository showcase.

**Area:** `packaging/`, `scripts/packaging.mjs`, `.github/`, `site/`, [RELEASING.md](RELEASING.md)
**Depends on:** nothing; but the first rows are the owner's decisions

| Feature | Prio | Origin |
|---|---|---|
| 🆕 **Make the repository public.** **To decide (owner).** It unblocks auto-update (today `latest.json` returns 404 because the repository is private: see the blocked row of group P), package managers and unlimited Actions minutes. Before: check that no secret or personal data is in the history, licences are in order, `SECURITY.md` and `CONTRIBUTING.md` are up to date | P1 | U |
| 🆕 **Submission to package managers**, with the manifests already in `packaging/`: winget, Homebrew, AUR, Flathub; for each the steps in RELEASING.md and a check that SHA-256 and version match the release | P2 | N |
| 🆕 **GitHub Pages** for the site (`site/`), with a manually triggered workflow; domain to decide | P3 | U |
| 🆕 After publishing: the "real auto-update test" row of group P can be done | P1 | U |

---

## 3. Recommended waves

With 1–3 people. Each group can start when its dependency is ready.

| Wave | Groups | Goal | Status |
|---|---|---|---|
| **1** | **A** | Give agents more of what the app knows (ER diagram, catalog, schema compare, profiles) with safer results and limits | To do |
| **2** | **B**, **C** | Extend the app from outside (plugins) and cover enterprise networks with Kerberos | To do |
| **3** | **D**, **E** | User documentation and distribution, once the user decides to publish | D to do, E waiting for the decision |

Each group follows the Definition of Done of the roadmap.
