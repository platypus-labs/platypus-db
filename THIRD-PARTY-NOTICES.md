# Third-party notices

PlatypusDB is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE).
It includes and links to software from other projects, each under its own license. All of
them can be combined with the AGPL.

## Code and assets kept in this repository

| Component | Where | License |
|---|---|---|
| [tiberius](https://github.com/prisma/tiberius) 0.12.3, with PlatypusDB's patches (listed at the top of its `Cargo.toml`) | `vendor/tiberius/` | MIT or Apache-2.0 ([MIT](vendor/tiberius/LICENSE-MIT.txt), [Apache](vendor/tiberius/LICENSE-APACHE.txt)) |
| [scylla](https://github.com/scylladb/scylla-rust-driver) 1.9.0, the Cassandra and ScyllaDB driver, with rustls on the ring provider only (see the top of its `Cargo.toml`) | `vendor/scylla/` | MIT or Apache-2.0 |
| [Mermaid](https://github.com/mermaid-js/mermaid) 11.4.1, `mermaid.min.js`, embedded in the HTML schema documentation | `crates/platypus-docs/assets/` | MIT |
| [Inter](https://github.com/rsms/inter) font | `apps/desktop/src/assets/fonts/`, `site/fonts/` | SIL Open Font License 1.1 ([text](apps/desktop/src/assets/fonts/OFL-Inter.txt)) |
| [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) font | `apps/desktop/src/assets/fonts/`, `site/fonts/` | SIL Open Font License 1.1 ([text](apps/desktop/src/assets/fonts/OFL-JetBrainsMono.txt)) |

## Bundled dependencies

The desktop app bundles these JavaScript packages, among others:

| Component | Use | License |
|---|---|---|
| [Lucide](https://lucide.dev) icons (`lucide-react`) | interface icons | ISC |
| [Simple Icons](https://simpleicons.org) (`simple-icons`) | database logos in the driver picker | CC0-1.0; the logos remain trademarks of their owners |
| [CodeMirror](https://codemirror.net) | SQL editor | MIT |
| [codemirror-vim](https://github.com/replit/codemirror-vim) (`@replit/codemirror-vim`) | Vim mode of the SQL editor | MIT |
| [React](https://react.dev) | interface | MIT |
| [Apache Arrow](https://arrow.apache.org) (`apache-arrow`) | result batches | Apache-2.0 |
| [Tauri](https://tauri.app) API and plugins | desktop shell | Apache-2.0 or MIT |

The Rust side compiles in, among others, [DuckDB](https://duckdb.org) (MIT) and
[SQLite](https://sqlite.org) (public domain) as bundled native libraries. The full list of Rust
crates is in `Cargo.lock`, and of JavaScript packages in `apps/desktop/package-lock.json`. Their
licenses are permissive (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, CDLA-Permissive-2.0 for
the root certificates, UPL-1.0) or weak copyleft (MPL-2.0: `cssparser`, `selectors`,
`dtoa-short`, `option-ext`); none is incompatible with the AGPL. `cargo metadata` lists each
crate's license, `npm ls --all` each package's.

## Downloaded on first use, not distributed

Some drivers are fetched by the app when a connection needs them, from their vendors, under the
vendors' own terms: ADBC drivers from the dbc registry (the app only installs those with a free
license), Oracle Instant Client and IBM's ODBC driver for Db2. PlatypusDB does not ship them.
