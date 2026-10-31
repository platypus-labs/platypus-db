[← README](README.md)

# Copertura dei test

Per ogni funzionalità visibile all'utente, il test che la copre. Le funzionalità vengono da [FEATURES.md](FEATURES.md) e dalle righe ✅ della [roadmap](ROADMAP.md); i test sono gli scenari E2E, i test di conformance e, quando sono l'unica copertura, i test unitari. È l'inventario del gruppo S (collaudo finale): i buchi in fondo (sezione "Buchi") diventano test.

## Come si legge

- **Stato**: ✅ coperto da un E2E o da un test di conformance che esercita il comportamento vero; 🟡 coperto solo da test unitari, solo dal caso felice o solo su alcuni database; ❌ nessun test.
- **E2E**: i nomi sono i file di `apps/desktop/e2e/scenarios/`, senza `.mjs`. Quelli senza database nel nome girano su una SQLite temporanea (`-sqlite`) o su un database di `docker/compose.yml` (`-postgres`, `-mysql`, …); i `qa-*` sono scenari nati da un bug, i `qa-*-ok-*` sono guardie di regressione.
- **Conformance**: `crates/platypus-conformance/tests/<file>.rs` e il nome della funzione. Le funzioni di `conformance.rs` girano su ogni database raggiungibile fra PostgreSQL, MySQL, MariaDB, SQL Server, ClickHouse, SQLite, libSQL, DuckDB e, con il profilo `pgwire`, CockroachDB, YugabyteDB e TimescaleDB; saltano senza errore quelli che non raggiungono e, per alcune funzioni, quelli senza la capacità.
- **Unit**: `tests/*.test.mjs` del frontend (`apps/desktop/tests/`) e `#[test]` dei crate.
- Le funzioni di `conformance.rs` sono citate senza prefisso quando il nome basta a riconoscerle.

## Come si aggiorna

Ogni funzionalità nuova aggiunge una riga alla sezione della sua area, con il test che la copre, nello stesso commit o nella stessa unione che la introduce. Un bug corretto porta il suo scenario `qa-*`: la riga della funzionalità lo cita. Se un test viene tolto o ridotto, lo Stato torna a 🟡 o ❌. Al collaudo si rilegge l'intero file.

## Connessioni

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Griglia dei loghi, ricerca, `+`, incolla di URL / JDBC / ADO.NET | qa3-url-ok-engines, qa3-url-ok-wrong-scheme, qa3-url-ok-keyboard-paste, qa2-jrn-palette-new-connection, connectionUrl.test | ✅ |
| Modalità URL (porte di ClickHouse, IPv6, `mongodb+srv`, segreti, parametri TLS, SQL Server) | qa3-url-ok-clickhouse, qa3-url-ok-ipv6, qa3-url-ok-mongo-srv, qa3-url-ok-pg-roundtrip, qa3-url-ok-secrets, qa3-url-sqlserver-params, qa3-url-libsql-tls-param, qa3-url-mysql-sslmode, qa3-url-remove-cert, qa-conn-mysql-netstring, qa-conn-url-dropped-parts | ✅ |
| Test connection per ogni database del form | qa-conn-ok-test-connection, qa3-url-ok-engines, qa-conn-port-range-error, qa-conn-raw-error-text | ✅ |
| Copy as URL (senza password) e Duplicate | qa-conn-ok-copy-url-secrets, qa-conn-duplicate-missing, qa-conn-host-credentials-leak | ✅ |
| Variabili `${env:NAME}` e file `.env` | connections-env-pooler | ✅ |
| Pooler in transaction mode (PgBouncer, Supavisor, Neon) | connections-env-pooler; conformance `works_through_a_transaction_pooler`; platypus-driver-pg `tests/pooler.rs`; pooler.test | ✅ |
| Import da DBeaver (connessioni, `.dbp`, password, tunnel SSH) | import-dbeaver-project, connections-sqlite, qa-conn-reimport-project, qa2-jrn-ok-import-organise, qa2-jrn-import-relative-sqlite | ✅ |
| Import da DataGrip, pgAdmin e `pg_service.conf` | import-datagrip-sqlite, import-pgadmin-sqlite, import-pg-service-sqlite; unit in `crates/platypus-store` (`import.rs`, `jdbc.rs`) | ✅ |
| Cartelle di connessioni e di progetti, trascinamento, Alt+Su/Giù | folders-sqlite, projects-folder-field-sqlite, qa-conn-ok-new-in-folder, railDrop.test, pointerDrag.test | ✅ |
| TLS con `sslmode`, CA | qa-conn-ok-pg-tls-modes, qa-conn-sqlserver-tls-flag, qa-conn-libsql-url-tls, qa3-url-mysql-sslmode | ✅ |
| Certificati client per PostgreSQL e MySQL | conformance `client_certificate.rs` (`client_certificate_postgres`, `client_certificate_mysql`, con i container `postgres-mtls` e `mysql-mtls`, profilo `tls`); unit (`platypus-tls`, `platypus-store` `config.rs`) | ✅ |
| Tunnel SSH (password, chiave, jump host, `known_hosts`) | platypus-net `tests/tunnel.rs` (`forwards_with_a_password_or_a_key`, `goes_through_a_jump_host`, `refuses_a_changed_server_key`, `connects_databases_through_the_tunnel`); E2E del solo form: connections-sqlite, qa-conn-ssh-port-clear, qa-conn-ssh-timeout | ✅ |
| Tunnel SSH con agente SSH (e Pageant) | platypus-net `tests/ssh_agent.rs` (`ssh_agent_tunnel`: agente servito dal test sulla pipe `openssh-ssh-agent` o su `SSH_AUTH_SOCK`; Pageant non provato) | ✅ |
| Network profiles condivisi fra connessioni | connections-sqlite | 🟡 |
| Proxy SOCKS5 / HTTP, anche davanti all'SSH | connection-proxy; `tunnel.rs` (`socks5_proxy`, `reaches_the_ssh_server_through_a_proxy`, `forwards_through_a_proxy_alone`) | ✅ |
| Port forward da `kubectl` e `aws ssm` | cloud-sign-in; conformance `cloud_cli.rs` (`reaches_through_the_forward`) | ✅ |
| Comandi prima e dopo la connessione (VPN) | connection-commands | ✅ |
| Password: keychain, "chiedi ogni volta", comando di shell | qa-conn-keychain-empty-asks, qa-conn-wrong-password-reprompt, qa-conn-saved-placeholder, qa-safe-ok-credentials, connections-sqlite | ✅ |
| Password da `.pgpass` | qa-conn-pgpass-postgres (PGPASSFILE, commenti, jolly, escape `:`, prima riga che corrisponde, riga mancante, password sbagliata, file tolto e rimesso); unit (`connectionTest.test`, `platypus-store`) | ✅ |
| Gestori di password (1Password, Bitwarden, Vault, AWS Secrets Manager) | credential-managers (con CLI finte), credentialManagers.test, qa-safe-mcp-password-command-output | ✅ |
| Master password (Argon2id + AES-256-GCM) | vault-postgres, qa3-mcp-vault, app-lock-sqlite, qa3-inst-upgrade-v020 | ✅ |
| Accesso cloud: AWS IAM, Google Cloud IAM, Entra ID | cloud-sign-in (CLI finte); conformance `cloud_cli.rs` (`signs_in_with_the_token`, `tokens_and_forwards_reach_the_servers`) | ✅ |
| SQL Server come utente Windows (SSPI) | signIn.test (solo la scelta nel form) | 🟡 |
| Test connections di un progetto (quattro alla volta) | test-connections-sqlite | ✅ |
| Tipi Development / Test / Production, colori, badge PROD, sola lettura | connections-sqlite, qa-safe-ok-readonly-connection, qa-safe-ok-prod-transfers, safety-sqlite | ✅ |
| Schema predefinito, fuso orario e istruzioni di avvio a ogni connessione | connections-sqlite (scheda Init), qa-conn-init-timezone-unsupported; conformance `sets_the_session_up_after_connecting` | ✅ |
| Controllo delle sessioni inattive e riconnessione | qa-conn-ok-reconnect-postgres, qa-conn-reconnect-mysql, qa-conn-reconnect-sqlserver, qa-sql-pg-reconnect-session-state, connections-sqlite; conformance `reconnect.rs` (`qa_fixconn_*`) | ✅ |
| Modifica di una connessione riapre le sessioni dei tab aperti | qa-conn-edit-open-tab-stale | ✅ |
| Eliminazione di una connessione; file delle connessioni corrotto | qa-conn-ok-delete-closes-tabs, qa-conn-ok-corrupt-file-kept | ✅ |
| Istruzioni di init di un progetto condiviso: approvazione del testo esatto | projects-shared-sqlite (`init_approval`) | ✅ |
| Prima apertura, database di esempio, schermata di benvenuto | sample-database, qa2-jrn-ok-first-day, welcome-recent-scripts-sqlite | ✅ |
| Copia portatile (cartella `data` accanto all'eseguibile) | portable-data-folder | ✅ |
| Gestori di pacchetti (winget, Homebrew, AUR, Flathub): avviso senza aggiornare | update-banner-package-manager (solo l'avviso) | 🟡 |
| Secondo giro (qa5): BOM UTF-8 in `.env`, `.pgpass`, `pg_service.conf` e import; database di un servizio senza `dbname`; nome di un URL Redis; alias nel selettore dei driver; Invio nel form | qa5-conn-dotenv-bom, qa5-conn-pgpass-bom, qa5-conn-import-bom, qa5-conn-import-service-database, qa5-conn-redis-url-name, qa5-conn-picker-aliases, qa5-conn-enter-saves, qa5-conn-ok-commands-shared-sessions | ✅ |

## Navigator e oggetti

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Albero lazy di database, schemi, tabelle, viste, routine, trigger, tipi, estensioni, eventi | dev-postgres, navigator-objects, navigator-select-sqlite, smoke-sqlite; conformance `navigates_metadata` | ✅ |
| Righe e dimensioni stimate, partizioni sotto la tabella | navigator-search; conformance `estimates_table_rows`, `sizes_tables_and_nests_partitions`, `qa_fixsafe_estimates_rows_of_tables_never_analyzed` | ✅ |
| Segnalibri, filtri per connessione, vista Simple | navigator-search, qa-nav-bookmark-after-rename, navigator-simple-sqlite | ✅ |
| 10.000 tabelle: albero veloce | scale-sqlite, qa-perf-pg-schema-5000-tables, qa-nav-pg-many-tables-slow; conformance `scale.rs` (`lists_ten_thousand_tables`) | ✅ |
| Albero in cache su disco, anche offline | navigator-search (cache offline) | ✅ |
| Logo, colore e stato della connessione | visual-sqlite, connections-sqlite | 🟡 |
| Genera SELECT / INSERT / UPDATE / DELETE / UPSERT / DDL; rinomina, svuota, elimina con anteprima | navigator-objects, qa-nav-ok-pg-odd-names, qa-safe-ok-odd-names-sqlite, qa-nav-pg-keyword-table-wrong-data, qa-nav-mysql-reserved-names, qa-nav-sqlite-keyword-names, qa2-jrn-prod-drop-uncommitted | ✅ |
| DDL di molti oggetti in blocchi, con Stop | generate-ddl-many-pg, ddlBlocks.test; conformance `reads_the_ddl_of_many_tables_together` | ✅ |
| Pagina dell'oggetto: proprietà, colonne, indici, vincoli, chiavi, riferimenti | navigator-objects, qa3-schema-ok-roundtrip-sqlite; conformance `describes_objects_and_their_ddl`, `qa_fixnav_*` | ✅ |
| Cerca nomi, colonne, sorgente di viste/routine e testo delle righe | navigator-search; conformance `searches_metadata`, `searches_table_data`; `search.rs`/searchNames.test | ✅ |
| Note e tag, commenti del database, etichetta di riga | catalog-sqlite, comments-postgres, annotation.test, labelColumn.test; conformance `writes_comments` | ✅ |
| Colonne sensibili (regole per nome/tipo, scheda dell'oggetto) | sensitive-columns-sqlite, mask-sqlite, maskRules.test | ✅ |
| "Used by" di una colonna | column-usage-sqlite | ✅ |
| Esporta documentazione (Markdown / HTML con ER) | catalog-sqlite (Markdown), qa-nav-docs-markdown-anchors, readme-duckdb | 🟡 |
| Crea vista, funzione, trigger, sequenza (moduli con anteprima) | schema-objects-sqlite, schema-objects-postgres | ✅ |
| Attività del navigator con tastiera (frecce, Invio, menu); tooltip del nome troncato | shell-tabs-sqlite, qa-nav-refresh-shortcut, qa-nav-table-tooltip-name, qa2-jrn-ok-keyboard | ✅ |
| Cartelle degli oggetti tradotte in italiano | qa-nav-italian-folders, qa-ui-navigator-italian | ✅ |
| Eventi ed estensioni PostgreSQL, tipi enum e sinonimi (SQL Server) | conformance `qa_fixnav_synonyms_and_enum_types`, qa-nav-sqlserver-synonyms | ✅ |
| Il navigator si aggiorna dopo il DDL dell'editor | qa-nav-pg-editor-ddl-refresh, qa-nav-pg-expand-dropped-table, qa2-jrn-pg-refresh-default-db | ✅ |
| Secondo giro (qa5): selezione da tastiera e PageUp / PageDown nell'albero; Generate DDL su PostgreSQL (collation, sequenze, opzioni, RLS, INHERITS) e SQL Server; dettagli di indici parziali e chiavi esterne | qa5-nav-keyboard-multiselect, qa5-nav-keyboard-page-keys, qa5-nav-pg-ddl-collation, qa5-nav-pg-ddl-serial-sequence, qa5-nav-pg-ddl-options-rls, qa5-nav-pg-ddl-inherits, qa5-nav-sqlserver-ddl-collation, qa5-nav-details-index-fk-hide, qa5-nav-ok-pg-ddl-not-valid | ✅ |

## Editor SQL

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Esegui statement (Ctrl+Invio) e script (Alt+X); errore indicato nel testo | smoke-sqlite, editor-sqlite, qa-sql-error-position-emoji, qa-sql-error-position-params; conformance `maps_errors_with_their_position` | ✅ |
| Completamento: alias, CTE, sottoquery, JOIN dalle chiavi esterne | completion-sqlite, editor-sqlite, qa-sql-completion-schema-dot, sqlScope.test, typedWord.test | ✅ |
| Formattatore | editor-sqlite, format.test, formatting.test | ✅ |
| Comandi di riga, cursori multipli, folding, outline, Ctrl+clic su un nome | editor-more-sqlite (cursori sopra e sotto, Ctrl+D, Esc; outline con titoli, clic e stato vuoto) | ✅ |
| Parametri `:id`/`?`, variabili `${}`, `@set`, `@echo`, `@include`, `@export` | editor-more-sqlite, qa-sql-include-nested-relative, qa-sql-pg-include-bom, qa-xfer-export-directive-extension, qa-sql-sqlite-named-params | ✅ |
| Snippet e storico delle query, file `.sql` | editor-more-sqlite, scripts-builder-sqlite, history.test, qa-sql-ok-history-duckdb, qa-sql-script-file-encodings | ✅ |
| Ctrl+\ in un nuovo tab di risultato | editor-more-sqlite | ✅ |
| Modalità terminale (risultati come testo) | editor-more-sqlite | ✅ |
| Script con `psql`, `mysql`, `sqlcmd`, `sqlite3` locali | editor-cli-script-postgres (psql vero: `set`, `copy` da e verso file, errore che ferma lo script, selezione, client mancante, sola lettura; l'annullamento lascia correre la query sul server, bug B4); `mysql`, `sqlcmd`, `sqlite3`: solo unit in `apps/desktop/src-tauri` | 🟡 |
| Esegui su più connessioni | editor-multi-run-sqlite, multiRun.test | ✅ |
| Modalità Vim | vim-sqlite | ✅ |
| Linter (colonne ignote, join senza condizione, `SELECT *` in produzione) e controllo ortografico dei commenti | editor-more-sqlite (messaggi al passaggio del mouse, solo produzione per `SELECT *`, impostazione off / commenti / tutto); conformance `compares_and_lints_schemas` | ✅ |
| Script salvati per progetto (cartelle, trascinamento, connessione associata) | scripts-builder-sqlite, qa3-inst-upgrade-v020, welcome-recent-scripts-sqlite | ✅ |
| Cancellazione di una query in corso, su ogni database | qa-sql-ok-cancel-sqlite, qa-perf-ok-cancel-postgres, qa-sql-clickhouse-cancel, qa-ora-cancel-plsql; conformance `cancels_running_queries`, `oracle_cancels_a_running_statement`, `cancels_blocking_commands` (Redis), `cancels_a_slow_search_and_goes_on` | ✅ |
| Script lunghi: velocità, schede di risultato, chiusura del tab con statement in corso | qa-sql-script-throughput, qa-sql-pg-close-tab-running | ✅ |
| Più set di risultati e messaggi del server (`RAISE NOTICE`, `PRINT`) | qa-sql-mssql-multiple-results, qa-sql-server-messages; conformance `qa_fixsql_multiple_result_sets`, `qa_fixsql_server_messages` | ✅ |
| Script e delimitatori per database (`DELIMITER`, `GO`, `$$`, `/` di Oracle) | qa-sql-ok-pg-scripts, qa-sql-ok-mysql-scripts, qa-sql-mysql-delimiter-glued, qa-sql-ok-mssql-batches, qa-sql-mssql-go-count, qa-sql-mssql-merge-semicolon, qa-sql-mssql-script-variables, qa-ora-ok-editor | ✅ |
| Transazioni dal editor (commit manuale, errore dentro la transazione) | qa-sql-ok-pg-transactions, qa-sql-pg-commit-after-error; conformance `manual_commit_and_rollback`, `qa_fixsql_manual_commit_survives_a_failed_statement` | ✅ |
| Editor MongoDB (mongosh), Redis, Elasticsearch/OpenSearch, CQL | qa-sql-ok-mongo-shell, qa-sql-mongo-use-context, redis-editor, search-editor, cassandra | ✅ |
| Query plan, Explain analyze, mappa di calore, grafo, Compare…, Open query plan… | explain-sqlite, explain-postgres, explain-compare-postgres, explain-open-plan, qa-ora-explain-plan, qa-sql-explain-unsupported, explainPlans.test, plans.mjs; conformance `explains_queries`, `explained_plans_read_back`, `suggests_indexes_for_large_scans` | ✅ |
| Query manager (Ctrl+Maiusc+M), traccia rete/server/fetch, statement lenti | explain-sqlite, qa2-jrn-it-query-manager | 🟡 |
| Divisione dell'editor in due (Ctrl+Maiusc+D) | shell-tabs-sqlite | ✅ |
| Creazione di un nuovo tab con Ctrl+T e focus | qa2-jrn-new-editor-focus, qa-ui-project-new-tab | ✅ |
| Notifica di sistema per una query oltre i 10 secondi con la finestra in secondo piano | nessun test | ❌ |
| Notebook (celle Markdown/SQL, Run all, `.pnb`, export HTML) | notebook-sqlite, notebook.test | ✅ |
| Query builder visuale | scripts-builder-sqlite, query-builder-sqlite | ✅ |
| Debugger PL/pgSQL (passo, breakpoint, variabili) | admin-debugger-postgres, debugger.test; conformance `debugger.rs` (`debugs_a_function`, `debugs_a_procedure_to_its_end`, `stopping_leaves_no_blocked_session`) | ✅ |
| Secondo giro (qa5): formattatore con operatori PostgreSQL, esponenti con segno e assegnazioni T-SQL; completamento dopo la richiesta della password; istruzione sotto il cursore | qa5-sql-format-pg-operators, qa5-sql-format-exponent, qa5-sql-format-mssql-compound, qa5-sql-completion-password-prompt, qa5-sql-ok-statement-under-cursor | ✅ |

## Griglia e modifica

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Griglia su canvas, pagine da 1.000 righe, Fetch all | grid-sqlite, qa-perf-ok-1m-rows-sqlite, qa-perf-took-counts-idle, rowWindow.test, fetchBackoff.test; conformance `streams_large_results_in_batches`, `scale.rs` (`streams_a_million_rows_to_files`) | ✅ |
| Apri una tabella; `WHERE` scritto a mano; ordinamento dall'intestazione | viewer-sort-sqlite, viewerSort.test, readme-sqlite | ✅ |
| Filtri rapidi sul valore, Count rows | grid-sqlite, grid-more-sqlite | ✅ |
| Copia con intestazioni, CSV, JSON, Markdown, HTML, INSERT, IN | grid-sqlite, qa-perf-copy-long-cell, qa-perf-copy-unloaded-rows, qa-grid-copy-paste-empty-sqlite | ✅ |
| Trova nelle righe caricate (Ctrl+F) | grid-sqlite, textSearch.test | ✅ |
| Pannello valore (F7): testo lungo, JSON, XML, esadecimale, immagini, salva | grid-text-json-sqlite, qa-perf-value-panel-huge, grid-value-panel-binary-sqlite (XML, esadecimale, salva: ok; l'immagine non si vede: bug 1), jsonText.test | 🟡 |
| Colonne nascoste, bloccate, mosse, ridimensionate, layout per tabella | grid-more-sqlite | ✅ |
| Formattazione condizionale | grid-formatting-sqlite, formatting.test | ✅ |
| Colonne calcolate | grid-computed-sqlite, computed.test | ✅ |
| Totali, vista record, conteggi per valore, statistiche, confronto di due righe | grid-more-sqlite | ✅ |
| Chiavi esterne che aprono la riga, pannello References | grid-more-sqlite, edit-more-sqlite | ✅ |
| Pannello mappa (PostGIS, MySQL, WKT) | grid-map-postgres (tipi geometrici di PostgreSQL, EWKB con SRID, WKB, WKT e collezioni, colonna GEOMETRY di MySQL, illeggibili, selezione di più colonne, clic, zoom, oltre 10.000 forme); grid-more-sqlite (WKB su SQLite); conformance `reads_geometries`. Nessuna immagine di prova ha PostGIS: i valori che PostGIS invierebbe sono memorizzati come testo/bytea, una colonna di tipo `geometry` vera non è provata | 🟡 |
| Risultati in sola lettura con aggiornamento automatico | grid-more-sqlite, rerun.test | ✅ |
| Date, fuso orario, raggruppamento cifre, tempi relativi dalle impostazioni | qa-ana-locale-numbers, qa2-jrn-count-locale, format.test | 🟡 |
| Valori speciali mostrati come nel database (infinity, bit, intervalli, UNION, money, datetime2) | qa-grid-pg-infinity, qa-grid-pg-bit-display, qa-grid-duckdb-display, qa-grid-mssql-money, qa-grid-mssql-datetime, qa2-dbs-interval-display, qa2-dbs-duckdb-union-null, qa2-dbs-trino-complex-values; conformance `value_fidelity.rs` | ✅ |
| Modifica in cella (doppio clic, F2, digitando), NULL/DEFAULT, righe aggiunte, duplicate, eliminate | edit-sqlite, edit-guards-sqlite, qa-grid-duplicate-autokey-sqlite, qa-grid-duplicate-identity, editState.test; conformance `edits_table_data`, `edit_rollbacks`, `edits_many_rows` | ✅ |
| Anteprima SQL, salvataggio tutto o niente, commit manuale | edit-guards-sqlite, qa-grid-ok-pg-roundtrip, qa-grid-ok-batch-mysql; conformance `edits_keep_uncommitted_work_without_savepoints` | ✅ |
| Script di annullamento di ogni salvataggio | edit-more-sqlite, qa-grid-undo-script-generated-pg, qa-grid-redo-sqlite | ✅ |
| Chiave primaria, chiave univoca, chiave virtuale; perché è sola lettura | edit-more-sqlite, qa-grid-pg-quoted-key, qa-grid-pg-real-key; conformance `edits_composite_keys` | ✅ |
| Risultato di una query su una tabella modificabile | edit-more-sqlite | ✅ |
| Incolla da foglio di calcolo (sulle celle e come righe nuove) | edit-more-sqlite, qa-grid-paste-single-value-sqlite, paste.test | ✅ |
| Modifica della selezione: valore, NULL, DEFAULT, riempi in basso, trova e sostituisci, sequenza | edit-more-sqlite, qa-perf-ok-mass-edit-5000-sqlite, massEdit.test | ✅ |
| Carica una cella da file (50 MB chiede, 200 MB rifiuta) | edit-more-sqlite (caso base) | 🟡 |
| Chiavi esterne con nome della riga e lista per la modifica | edit-more-sqlite, labelColumn.test | ✅ |
| Array e valori compositi PostgreSQL | arrays-postgres; conformance `edits_arrays_and_composites`, `arrays_as_postgresql_prints_them` | ✅ |
| Colonne generate e identità | qa-grid-generated-columns-pg, qa-ora-grid-duplicate-virtual; conformance `qa_fixgrid_generated_columns` | ✅ |
| Annulla/ripeti, Ctrl+frecce, tastiera della griglia | qa-grid-redo-sqlite, qa-grid-ctrl-arrows-sqlite, qa-perf-ctrl-end-no-focus | ✅ |
| Modifica su Redis, Elasticsearch, Cassandra | redis-edit, search-edit, cassandra; conformance `saves_grid_edits_and_undoes_them` (Redis), `saves_the_grids_changes_and_undoes_them` (search), `saves_the_grids_changes` (Cassandra) | ✅ |
| Modifica su Oracle (DATE, CLOB, righe molte) | qa-ora-ok-grid-edit, qa-ora-grid-date-literals, qa-ora-grid-long-clob; conformance `oracle_saves_many_edited_rows` | ✅ |
| Dati sensibili mascherati (Ctrl+Alt+M), copie con i valori | mask-sqlite, maskRules.test | ✅ |
| Chiusura della finestra con modifiche o transazione non salvate | qa-ui-close-window-unsaved | ✅ |
| Secondo giro (qa5): SQLite con colonne NUMERIC vuote, filtro rapido su FLOAT di MySQL, tipi scalari di PostgreSQL | qa5-grid-sqlite-empty-table-number-text, qa5-grid-mysql-filter-float, qa5-grid-ok-pg-scalar-types | ✅ |

## Analisi

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Grafici (barre, linee, dispersione, torta), PNG/SVG, "Other" | analysis-chart-sqlite, qa-ana-chart-empty, qa-ana-chart-null-average, qa-ana-chart-null-category, qa-ana-chart-pie-negative, qa-ana-chart-x-in-y, qa-perf-chart-other-label, chartConfig.test, chartScale.test | ✅ |
| Statistiche di colonna al passaggio del mouse | analysis-stats-sqlite, qa-ana-ok-stats-chart, qa-ana-stats-bigint, qa-ana-stats-infinity, qa-ana-stats-interval, qa-ana-stats-histogram-labels | ✅ |
| SQL sul risultato (`result`, `r1`, `r2`…), ingrandito, nuovo tab | analysis-sql-sqlite, qa-ana-ok-sql-results, qa-ana-sql-hugeint, qa-ana-sql-null-interval, qa-ana-sql-paren-condition, qa-grid-ok-sqlfield-sqlite, qa-grid-sqlfield-paren-sqlite, qa-grid-sqlfield-group-ordinal-sqlite, qa-perf-resultsql-copy-kept, qa-perf-resultsql-numeric-order, sqlField.test | ✅ |
| Confronto con l'esecuzione precedente | analysis-diff-sqlite, qa-ana-ok-diff-keyed, qa-ana-diff-type-change, diffModel.test | ✅ |
| Pivot… | analysis-pivot-sqlite, pivot.test | ✅ |
| Query across databases… | analysis-across-sqlite, acrossModel.test | 🟡 |
| Save result… e pannello Saved results | analysis-saved-results-sqlite, savedResultsModel.test | ✅ |
| Dashboard personalizzate | analysis-dashboard-sqlite, dashboard.test | ✅ |
| Profile data… | analysis-table-profile-sqlite, tableProfileModel.test | ✅ |
| Dati fittizi (Generate data…) per tipo e database | qa-xfer-fake-identity, qa-xfer-fake-integer-range, qa-xfer-fake-text-length, qa-xfer-fake-self-reference, qa-xfer-fake-unsupported-types, qa-xfer-fake-mariadb-defaults, qa-perf-fake-identity-mssql, qa2-dbs-*-generate-*, generators.test; conformance `fills_tables_with_fake_rows`, `fills_a_collection_with_fake_documents` | ✅ |
| Notebook con celle sui risultati delle altre | notebook-sqlite, notebook.test | ✅ |
| Query across con grandi volumi (512 MB) | nessun test sul limite | ❌ |

## Import / export

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Export CSV | export-formats-postgres (delimitatori, senza intestazione, righe già caricate, gzip e zstd, cartella inesistente), transfer-sqlite, profiles-export-sqlite, qa-xfer-csv-null-vs-empty, qa-xfer-export-tables-collide | ✅ |
| Export JSON / NDJSON | export-formats-postgres, qa-xfer-json-export-nested, profiles-export-sqlite | ✅ |
| Export Parquet | export-formats-postgres (piè di pagina letto: righe, colonne, tipi, codec zstd; reimportato; 0 e 50.000 righe), qa-xfer-ok-pg-export-roundtrip, profiles-export-sqlite | ✅ |
| Export Excel | export-formats-postgres (tipi delle celle, date come numeri seriali, NULL senza cella, 50.000 righe), qa-xfer-xlsx-export-dates | ✅ |
| Export SQL INSERT | export-formats-postgres (le istruzioni rieseguite danno le stesse righe), qa-ora-export-sql-replay, qa-xfer-ok-pg-export-roundtrip | ✅ |
| Export Markdown, HTML, XML | export-formats-postgres (pipe e markup in escape, NULL, risultato vuoto), qa-xfer-xml-control-chars | ✅ |
| Export in streaming, annullato, con limiti, errore col nome giusto | qa-xfer-ok-export-cancel-limits, qa-xfer-export-error-part-path, qa-xfer-cancel-without-notice; conformance `streams_a_million_rows_to_files`, `reads_and_exports_cells_of_tens_of_megabytes` | ✅ |
| Compressione gzip / zstd | qa-xfer-export-directive-extension (gzip), sql-file-dump-sqlite (gzip); zstd solo unit | 🟡 |
| Caricamento su URL firmato o bucket S3 / GCS / R2 / MinIO | conformance `upload.rs` (`uploads_an_export_to_a_bucket_and_reads_it_back`, `refuses_wrong_keys_and_a_missing_bucket` su un archivio S3 locale, profilo `s3`; `uploads_an_export_to_a_signed_url` su un server HTTP del test); unit in `platypus-transfer`; qa-xfer-upload-s3 (dal form, oggetto riletto con una GET firmata: nome con spazi e accenti, gzip, Parquet, 5 MB, chiavi sbagliate, bucket assente, endpoint non valido o chiuso); il messaggio di fine export non dice che il file è stato caricato | ✅ |
| Export di più tabelle o di uno schema | transfer-sqlite, qa-xfer-export-tables-collide, qa-xfer-mongo-export-tables | ✅ |
| Import CSV, TSV, JSON, NDJSON | import-sqlite, qa-xfer-csv-ragged-rows, qa-xfer-csv-windows-1252, qa-xfer-csv-quoted-empty, qa-xfer-csv-decimal-precision, qa-xfer-json-nested-import, qa-xfer-ok-import-modes | ✅ |
| Import Parquet ed Excel | import-parquet-xlsx-sqlite (Parquet scritto dall'export, foglio scelto dall'elenco, file illeggibili), qa-xfer-xlsx-date-in-text-column, qa2-dbs-ok-duckdb-files | ✅ |
| Import in tabella nuova con i tipi inferiti | import-sqlite, qa2-dbs-odbc-import-new-table | ✅ |
| Modalità di import: aggiungi, sostituisci, aggiorna per chiave | qa-xfer-ok-import-modes, qa-xfer-upsert-repeated-key, qa-ora-import-update-mode, qa-xfer-clickhouse-replace; conformance `imports_data_files`, `check_import_modes` | ✅ |
| Incolla CSV come tabella nuova | transfer-sqlite | ✅ |
| Copia di una tabella in un altro database (ogni coppia) | transfer-sqlite, qa-xfer-ok-transfer-mssql, qa-xfer-copy-table-keys, qa-ora-copy-to-oracle, qa-xfer-mongo-copy, qa2-dbs-cockroach-copy-array; conformance `transfers_tables_between_databases` | ✅ |
| Copia di uno schema con chiavi, indici, CHECK, viste, routine | transfer-sqlite, qa-xfer-migrate-fk-actions, qa-xfer-migrate-check-cast, qa-xfer-migrate-clickhouse-steps; conformance `migrates_tables_between_databases` | ✅ |
| Profili di trasferimento: salva, rilancia, pianifica, segreti fuori dal file | profiles-export-sqlite, profiles-transfer-sqlite, profiles-schedule-sqlite, transferProfiles.test | ✅ |
| Dati da file come database (CSV, JSON, Parquet, Excel) | data-files-as-databases-sqlite (JSON, NDJSON, Parquet trascinato, un foglio per tabella, file non valido), import-sqlite (CSV trascinato), qa2-dbs-ok-duckdb-files | ✅ |
| Run SQL file… (file `.sql`, `.gz`, `.zst`) e Dump to SQL file… | sql-file-dump-sqlite; conformance `dumps_and_runs_sql_files`, `dump_round_trip`, `dumps_types_routines_and_sequences`, `delimiter_across_chunks` | ✅ |
| Caricamenti ottimizzati (`COPY`, `LOAD DATA`, bulk copy SQL Server, appender DuckDB) | qa-perf-mssql-import-speed; conformance `bulk_loads_rows`, `value_fidelity.rs` (`mssql_bulk_*`) | ✅ |
| Nessuna scrittura su connessioni in sola lettura (import, dati fittizi, copia) | qa-safe-readonly-import, qa-safe-readonly-generate-data, qa-safe-readonly-migrate, restrictions-sqlite | ✅ |
| Connessione che vieta gli export | restrictions-sqlite | ✅ |
| Secondo giro (qa5): import di JSON con BOM in DuckDB; export Excel di decimali oltre 2^53; Export all tables con nomi lunghi | qa5-xfer-json-bom, qa5-xfer-xlsx-large-float | ✅ |

## Impostazioni e tab

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Impostazioni in schede, ricerca, ultima scheda ricordata | smoke-sqlite, qa-ui-settings-escape-blank, qa-ui-settings-reset-field, qa-ui-reset-tooltip, qa-ui-integer-settings, qa-ui-corrupt-settings, settingValue.test | ✅ |
| Temi (Platypus, Classic, Nord, Dracula, Solarized, alto contrasto), colori propri, dimensione, densità, raggio, font | theme-sqlite (anche alto contrasto: rapporti di contrasto), theme-upgrade-sqlite, qa-ui-theme-contrast, qa-ui-ok-shell-keys | ✅ |
| Layout: navigator, risultati, barra dei tab, pannelli laterali | customize-layout-sqlite, qa-ui-sidebar-width-regrow | ✅ |
| Icone (Lucide, Tabler, Phosphor) e animazioni | customize-motion-sqlite, iconSets.test | ✅ |
| File tema: esporta, importa, anteprima | theme-files, themeFile.test | ✅ |
| Sette lingue; tasti e numeri localizzati | languages-sqlite (tutte e sette, elenco delle lingue), qa-ui-live-language, qa-ui-italian-key-names, qa-ui-italian-store-errors, qa2-jrn-it-*, qa3-url-italian | 🟡 |
| Scorciatoie modificabili, conflitti, ricerca per azione | qa-ui-editor-shortcut-unbind, qa-ui-shortcut-clash-scope, qa-ui-shortcut-comma, qa-ui-shortcut-plain-key, qa-ui-shortcuts-behind-modal, qa-ui-tooltip-shortcuts, qa-ui-run-shortcut-listed, shortcutKeys.test | ✅ |
| Barra dell'editor: icone, nascondi pulsanti | shell-sqlite, qa-ui-toolbar-small-window, qa2-jrn-toolbar-wrap-transaction | ✅ |
| Zoom dell'interfaccia (`Ctrl+=` / `-` / `0`) e font di editor e griglia | qa-ui-zoom-keys (zoom: passi, limiti, tastierino, salvataggio, riavvio), shell-tabs-sqlite; i font di editor e griglia: solo unit | 🟡 |
| Progetti: rail, connessioni e tab per progetto, `Ctrl+1…` | projects-sqlite, projects-folder-field-sqlite | ✅ |
| Barra superiore: progetto, connessione, colore del tipo, barra del titolo di sistema | theme-sqlite, visual-sqlite, windowDrag.test | 🟡 |
| Palette dei comandi (`Ctrl+K`) | shell-sqlite, qa-ui-palette-highlight-emoji, qa2-jrn-palette-new-connection, fuzzy.test | ✅ |
| Navigator ridimensionabile, nascosto con `Ctrl+B` | shell-polish-sqlite, qa-ui-sidebar-width-regrow | ✅ |
| Tab: fissa, apri accanto, chiudi altri, raggruppa, riapri (`Ctrl+Maiusc+T`) | shell-sqlite, shell-tabs-sqlite, qa-ui-pinned-delete-key, qa-ui-close-tab-neighbour, qa-ui-tab-titles, qa2-jrn-restore-table-tab | ✅ |
| Tab con modifiche non salvate: chiusura | qa-ui-close-dirty-script, qa-ui-close-window-unsaved, qa-ui-workspace-save-flush | ✅ |
| Pannelli ridimensionabili con dimensioni ricordate | windows-sqlite, qa-ui-ok-restore | ✅ |
| Tab in una finestra a parte; "Move to the main window" | windows-sqlite | ✅ |
| Quick query (palette) | windows-sqlite | ✅ |
| Icona nel vassoio di sistema per Quick query | nessun test | ❌ |
| Trascinare tabelle e colonne nell'editor | shell-tabs-sqlite, pointerDrag.test | 🟡 |
| Activity log; notifiche | shell-tabs-sqlite | 🟡 |
| Tastiera: dialoghi con focus, Invio non distruttivo, menu contestuali | qa-ui-ok-dialogs, qa-ui-focus-restore, qa-ui-confirm-default-focus, qa-ui-context-menu-clipped, qa2-jrn-ok-keyboard | ✅ |
| Log e pacchetto di diagnostica dopo un crash | diagnostics-sqlite | ✅ |
| Condivisione di una query (link `platypus://` e file `.platypus`) | share-query-sqlite, shareQuery.test | ✅ |
| Git per gli script | scripts-git-sqlite, gitModel.test; `platypus-sync` `tests/git_flow.rs` (`local_flow`, `remote_flow`) | ✅ |
| Sync cifrata su cartella | sync-folder-sqlite; unit in `platypus-sync` | ✅ |
| Progetti condivisi (Share project…, Open shared project…) | projects-shared-sqlite; `git_flow.rs` (`a_shared_projects_scripts_use_the_repository_around_them`) | ✅ |
| Plugin WebAssembly (azioni, driver HTTP) | plugins-sqlite; `platypus-plugins` `tests/examples.rs` (`sql_tools_actions`, `toy_http_driver`, `a_driver_may_reach_the_host_of_its_connection`) | ✅ |
| Blocco dell'app per inattività con master password | app-lock-idle-sqlite (orologio della pagina spostato), app-lock-sqlite | ✅ |
| Secondo giro (qa5): AltGr che scrive le parentesi quadre e graffe, Ctrl+/ con tastiere dove `/` richiede Maiusc, Unfold all spostato da Ctrl+Alt+U | qa5-ui-altgr-brackets, qa5-ui-italian-comment-key | ✅ |

## Sicurezza e MCP / agenti

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Conferma di `UPDATE`/`DELETE` senza `WHERE` e `TRUNCATE`, con le righe | safety-sqlite, qa-safe-ok-guards-servers, qa-safe-guard-label, qa-safe-guard-target-name, qa-safe-guard-truncate-many, qa-sql-mongo-guard-text, qa-ora-guard-plsql-block, qa-ora-guard-row-count, qa2-dbs-mariadb-delete-history-guard, search-guards | ✅ |
| Connessioni di produzione: ogni modifica elencata e confermata | safety-sqlite, qa-safe-ok-prod-transfers, qa2-jrn-prod-drop-uncommitted | ✅ |
| Pulsante di blocco (tab di sola lettura) | safety-sqlite | ✅ |
| Sola lettura di una connessione, su ogni driver | qa-safe-ok-readonly-connection, qa-ora-ok-readonly, redis-readonly, search-readonly, qa3-mcp-readonly-matrix | ✅ |
| `LIMIT` automatico dalle impostazioni di sicurezza | safety-sqlite | 🟡 |
| Transaction log e Restore… | transaction-log-sqlite, txLog.test | ✅ |
| Istantanee prima di UPDATE/DELETE e "Snapshots before changes…" | snapshot-sqlite; conformance `snapshots_restore_changed_rows`, `snapshot_round_trip` | ✅ |
| Mascheramento (Ctrl+Alt+M) | mask-sqlite, maskRules.test | ✅ |
| Blocco dell'app e chiusura delle connessioni di produzione | app-lock-idle-sqlite (la produzione si riconnette, lo sviluppo no), app-lock-sqlite | ✅ |
| Divieto di modifica dei dati (solo SQL) | restrictions-sqlite | ✅ |
| Segreti mai in chiaro: file, log, anteprime, URL copiati | qa-safe-ok-credentials, qa-conn-ok-copy-url-secrets, qa-conn-host-credentials-leak, adminUsers.test (password mascherate) | ✅ |
| Server MCP (`platypus-db --mcp`), protocollo e strumenti | qa3-mcp-protocol, qa3-mcp-app-flag, qa3-mcp-app-mode, qa2-jrn-ok-agent-mcp; `platypus-mcp` `tests/stdio.rs` (`lists_tools_and_connections_without_secrets`, `serves_the_tools_over_stdio`, `describes_the_schema`) | ✅ |
| Permessi per connessione (disattivata / sola lettura / lettura e scrittura), Prod sempre in sola lettura | qa-safe-ok-mcp-access, qa3-mcp-readonly-matrix, qa3-mcp-readonly-postgres, qa-safe-ok-mcp-servers; `stdio.rs` (`runs_queries_within_the_access`) | ✅ |
| Limiti di righe, byte e timeout; cancellazione | qa3-mcp-limits, qa3-mcp-cancel; `stdio.rs` (`a_cancelled_request_cancels_its_statement`) | ✅ |
| Conferma nell'app per le scritture dell'agente | agents-bridge, agents-p2; `stdio.rs` (`asks_the_app_before_changes`) | ✅ |
| Ponte con l'app aperta e indicatore "agente collegato" | agents-bridge, qa3-mcp-app-mode | ✅ |
| `explain`, `sample_rows`, `propose_sql` | agents-p2; `stdio.rs` (`samples_rows_and_explains_statements`, `proposes_sql_for_the_app_to_open`) | ✅ |
| Contesto dell'utente su consenso | agent-context; `stdio.rs` (`shares_what_the_user_works_on_only_with_consent`), agentContext.test | ✅ |
| Schema come risorse MCP | `stdio.rs` (`serves_schemas_and_tables_as_resources`, `resource_reads_are_logged`) | 🟡 |
| Script salvati e cronologia in sola lettura, con colonne sensibili escluse | `stdio.rs` (`lists_and_reads_saved_scripts`, `searches_the_history`, `history_sql_is_redacted`) | 🟡 |
| Pagina Agents: configurazione in un clic, client, registro delle chiamate, filtro e CSV | qa3-mcp-agents-page, agents-sqlite, agents-p2 | ✅ |
| MCP con master password | qa3-mcp-vault | ✅ |
| CLI per agenti `platypus-cli` | `platypus-cli` `tests/saved_connections.rs` (`works_on_saved_connections_with_json_output`) | 🟡 |
| MCP su Oracle, SQL Server, ClickHouse, Redis, Elasticsearch | nessun test del server MCP su questi (solo SQLite, PostgreSQL, MySQL, MongoDB); conformance `agents_read_in_read_only_transactions`, `agents_ask_before_a_query_that_writes` | 🟡 |
| Secondo giro (qa5) e revisione di sicurezza: PRAGMA e SET su produzione, destinazione `kubectl` letta come opzione, proposte `propose_sql` in numero limitato, installazione negli JSON dei client senza toccare i numeri | qa5-safe-prod-pragma, qa5-safe-k8s-target-flag, qa5-mcp-propose-flood, qa5-mcp-install-bignumber; per SR1-SR17 vedi "Revisione di sicurezza" in fondo | ✅ |

## Schema

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Table editor (colonne, indici, chiavi esterne, UNIQUE, CHECK, anteprima, transazione) | table-editor-sqlite, object-tab-edit-table-sqlite, qa-nav-ok-pg-table-editor, qa-nav-ok-sqlserver-table-editor, qa-nav-mysql-generated-column, qa-nav-clickhouse-alter-type, qa-nav-sqlite-rebuild-loses-definitions; conformance `edits_table_designs`, `schema_designs`, `qa_fixnav_rebuild_keeps_what_the_design_does_not_show` | ✅ |
| Table editor su MongoDB (non offerto) | qa-nav-mongo-table-editor | ✅ |
| Diagramma ER: layout, minimappa, schermo intero, Refresh, esporta SVG/PNG, note | erd-layout-large-sqlite, erd-minimap-sqlite, erd-fullscreen-refresh-sqlite, qa3-schema-ok-erd-large-sqlite, qa3-schema-ok-erd-refresh-sqlite, qa3-schema-erd-cross-schema-pg, qa3-schema-erd-open-table-sqlite, erdLayout.test | ✅ |
| Disegna lo schema (Edit sul diagramma, FK trascinate, Generate DDL) | erd-edit-sqlite, erdEdits.test | ✅ |
| Check schema… | schema-tools-sqlite, qa3-schema-lint-pg-quoted, qa3-schema-lint-sqlite-affinity; conformance `qa3_schema_lint_and_checks_read_back`, `compares_and_lints_schemas` | ✅ |
| Compare with… (script, Flyway, Liquibase) | qa3-schema-ok-roundtrip-sqlite, qa3-schema-ok-roundtrip-mariadb, qa3-schema-compare-*, schema-changelog-sqlite; conformance `qa3_schema_compare_round_trips`, `qa3_oracle_schema_compare_round_trip` | ✅ |
| Compare data with… | schema-data-compare-sqlite; conformance `compares_table_data` | ✅ |
| Schema history | schema-history-sqlite, schemaHistory.test | ✅ |
| Migration status… (Flyway, Liquibase, Prisma, Alembic, Rails) | schema-migrations-sqlite (Flyway, Rails, Liquibase XML / YAML / SQL, Prisma, Alembic) | ✅ |
| Crea vista / funzione / trigger / sequenza | schema-objects-sqlite, schema-objects-postgres | ✅ |
| Commenti del database dalla scheda dell'oggetto | comments-postgres; conformance `writes_comments` | ✅ |
| Schemi di 300 tabelle: prestazioni degli strumenti | qa3-schema-perf-300-pg | ✅ |
| Indici: suggerimenti, `CREATE INDEX` da un risultato | indexSql.test; conformance `suggests_indexes_for_large_scans` | ✅ |
| Secondo giro (qa5): compare e table editor che perdevano opzioni (default con espressione, indici parziali e su espressione, chiavi differite, DEFERRABLE, NULLS NOT DISTINCT, MATCH FULL, EXCLUDE, partizioni, CHECK NOT ENFORCED, indici INVISIBLE, opzioni di tabella MySQL, FK disabilitate, tabelle temporali, system versioning) | qa5-schema-sqlite-default-expression, qa5-schema-sqlite-rebuild-deferred-fk, qa5-schema-sqlite-rebuild-on-conflict, qa5-schema-pg-edit-deferrable-fk e i `qa5-schema-compare-*` (sqlite, pg, mysql, mssql, mariadb); guardie `qa5-schema-ok-pg-object-forms`, `qa5-schema-ok-pg-rename-columns` | ✅ |

## Amministrazione

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Server activity: sessioni, filtri, apri lo statement nell'editor | admin-sessions-postgres, admin-sessions-mysql, admin-sessions-mariadb, admin-sessions-mssql, adminSessions.test; conformance `lists_and_ends_server_sessions` | ✅ |
| Cancel statement / End session (con conferma, rifiutati in sola lettura) | admin-sessions-postgres, admin-sessions-mysql | ✅ |
| Lock e catene di attesa | admin-sessions-mysql, admin-sessions-mariadb, admin-sessions-mssql; conformance `shows_blocking_chains` | ✅ |
| Statement più costosi | admin-statistics-postgres, admin-statistics-mysql, admin-statistics-mariadb; conformance `reads_server_statistics_and_maintains_tables` | 🟡 |
| Tabelle e indici (dimensioni, scansioni, dead rows, "Only those worth a look") | admin-statistics-postgres, admin-statistics-mysql | ✅ |
| Manutenzione (VACUUM, ANALYZE, REINDEX, OPTIMIZE, CHECK TABLE, UPDATE STATISTICS, DBCC) | conformance `reads_server_statistics_and_maintains_tables`; admin-maintenance-postgres, admin-maintenance-mysql, admin-maintenance-mssql | ✅ |
| Parametri del server | admin-parameters-postgres, adminParameters.test; conformance `reads_server_parameters` | 🟡 |
| Dashboard del server | admin-dashboard-postgres, adminDashboard.test; conformance `reads_server_metrics` | 🟡 |
| Utenti e ruoli: crea, modifica, elimina, privilegi | admin-users-postgres, admin-users-mysql, adminUsers.test; conformance `manages_users_roles_and_privileges` | 🟡 |
| Debug… di una funzione PL/pgSQL | admin-debugger-postgres; `debugger.rs` | ✅ |
| Backup… e Restore… (PostgreSQL, MySQL) | admin-backup-postgres, admin-backup-mysql | ✅ |
| Attività del server su SQL Server (utenti, statistiche, parametri) | admin-sessions-mssql, admin-parameters-mssql, qa4-admin-mssql-ft-unit, qa4-admin-activity-connect-failed, qa4-admin-mysql-emoji-statement, qa4-admin-mariadb-emoji-statement | ✅ |
| Utenti e ruoli su ClickHouse e SQL Server | admin-users-mssql, admin-users-clickhouse; conformance `manages_users_roles_and_privileges` | ✅ |
| Attività pianificate: script, export, backup; Run now, ritenzione, ritentativi | schedules-sqlite, schedules-missed-run-sqlite (orari persi, disabilitati, ritentativo registrato; il secondo tentativo non viene atteso), adminSchedules.test, profiles-schedule-sqlite | 🟡 |
| Attività pianificate: backup di database PostgreSQL / MySQL e fuso orario | schedules-backup-postgres (pg_dump e mysqldump veri: archivio PGDMP, SQL di uno schema, ritenzione, cartella impossibile, cartella mancante, schema inesistente, password sbagliata con ritentativo automatico al secondo giro, fuso orario di Tokyo) | ✅ |

## Driver e database

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| PostgreSQL 12+: più database per connessione, partizioni, PostGIS, array, tipi compositi | dev-postgres, navigator-search, arrays-postgres; conformance `conformance.rs` su `postgres`, `pg.rs`, versioni 12/14/16 con `PLATYPUS_TEST_ONLY` | ✅ |
| MySQL 5.7+ e MariaDB 10.6+ | qa-conn-reconnect-mysql, qa2-dbs-ok-mariadb-types; conformance su `mysql` e `mariadb`; `value_fidelity.rs` (`mysql_*`, `mariadb_*`) | ✅ |
| SQL Server 2017+ / Azure SQL | admin-sessions-mssql, qa-sql-ok-mssql-batches, qa-grid-mssql-*; conformance su `sqlserver`; `value_fidelity.rs` (`mssql_*`) | ✅ |
| ClickHouse | qa-sql-clickhouse-*, qa-xfer-clickhouse-*, qa-nav-clickhouse-alter-type; conformance su `clickhouse` | ✅ |
| SQLite | tutti gli scenari `-sqlite`; conformance su `sqlite` | ✅ |
| DuckDB (libreria scaricata al primo uso) | duckdb-download, qa2-dbs-ok-duckdb-files, qa-conn-duckdb-memory-url; conformance su `duckdb`; `value_fidelity.rs` (`duckdb_special_values_and_bits`) | ✅ |
| libSQL / Turso | qa2-dbs-ok-libsql-transaction, qa2-dbs-libsql-large-result, qa-conn-libsql-url-tls; conformance su `libsql` | ✅ |
| MongoDB 5+ | mongo, qa-sql-ok-mongo-shell, qa-grid-mongo-late-fields, qa-conn-mongo-authsource, qa-xfer-ok-mongo-fake-stop; conformance `mongo.rs` | ✅ |
| Redis 6+ | redis-browse, redis-editor, redis-edit, redis-readonly; conformance `redis.rs` | ✅ |
| Elasticsearch 8 / OpenSearch 2 | search-connect, search-browse, search-editor, search-paging, search-edit, search-guards, search-readonly; conformance `search.rs` | ✅ |
| Cassandra 4+ / ScyllaDB 5.4+ | cassandra; conformance `cassandra.rs` | ✅ |
| CockroachDB, YugabyteDB, TimescaleDB (profili `pgwire`) | qa2-dbs-ok-cockroach-edit, qa-nav-cockroach-system-schemas, qa2-dbs-yugabyte-ddl-sharding, qa2-dbs-timescale-*; conformance `qa_fixdbs_cockroach_*`, `qa_fixdbs_timescale_catalogs`, `qa_fixdbs_yugabyte_sharding` | ✅ |
| Supabase, Neon, AlloyDB (profili del driver PostgreSQL) | connectionUrl.test, pooler.test | 🟡 |
| Trino (ADBC) | trino-adbc, qa2-dbs-trino-*; conformance `bridges.rs` (`trino_*`, `qa_fixdbs_trino_*`) | ✅ |
| Snowflake, BigQuery, Databricks | nessun test (servono account cloud) | ❌ |
| ADBC con una libreria scelta | conformance `adbc_generic.rs` (driver ADBC di SQLite scaricato dal registro: query, errori, libreria mancante o non valida); drivers-registry (registro finto). Il driver di DuckDB non si carica: vedi "Bug trovati dal collaudo" | 🟡 |
| Oracle (Instant Client scaricato) | oracle-instantclient, qa-ora-*; conformance `oracle.rs` | ✅ |
| IBM Db2 (driver ODBC installato al primo uso) | nessun test: serve il driver ODBC di IBM (installazione manuale) e il container `icr.io/db2_community/db2` (oltre 2 GB, avvio di minuti): non pratico in locale | ❌ |
| ODBC generico | odbc-sqlserver, qa2-dbs-odbc-*; conformance `odbc.rs` | ✅ |
| Aggiornamento e versioni dei driver scaricati | drivers-versions, drivers-versions-auto, drivers-versions-quiet, drivers-versions-recent | ✅ |
| Compatibilità dei driver con versioni vecchie del server | driverCompat.test; conformance su `postgres-12`, `postgres-14`, `postgres-16`, `sqlserver-2017`, `mongo-5` (profilo `versions`) | ✅ |
| Errori col nome del server (`Unknown column 'x' (MySQL 5.7.44)`) | qa-conn-raw-error-text | 🟡 |

## Installer e aggiornamento

| Funzionalità | Test che la copre | Stato |
|---|---|---|
| Profilo scritto da 0.1.0 e 0.2.0 si apre con tutto | qa3-inst-upgrade-v010, qa3-inst-upgrade-v020; `platypus-store` `tests/qa3_inst_upgrade.rs` | ✅ |
| Banner di aggiornamento: versione, note, Later, Install and restart | update-banner, update-banner-current, update-banner-older, update-banner-malformed, update-banner-no-platform, update-banner-server-error, update-banner-unreachable | ✅ |
| Download e installazione vera dell'aggiornamento | nessun test (lo scenario non scarica nulla) | ❌ |
| Installer di Windows (MSI/NSIS), disinstallazione, dati dell'utente conservati | nessun test | ❌ |
| Aggiornamento da ogni versione pubblicata (connessioni, password nel keychain, script, impostazioni) | solo da 0.1.0 e 0.2.0, con i file e non con l'installer | 🟡 |
| Copia portatile e suo aggiornamento | portable-data-folder (riavvio e sostituzione dell'eseguibile) | ✅ |
| Pacchetti Linux (AppImage, deb, Flatpak, AUR) e macOS | nessun test (serve Linux / un Mac) | ❌ |

## Database: quali test li provano

I test di `conformance.rs` e quelli di `value_fidelity.rs`, `reconnect.rs`, `scale.rs` girano su tutti i database raggiungibili. Gli E2E li usano per nome (`-postgres`, `-mysql`, …) o dall'intestazione dello scenario (`qa-*`).

| Database | Conformance | E2E |
|---|---|---|
| PostgreSQL e varianti (12, 14, 16, PgBouncer) | `conformance.rs` (tutte le funzioni), `value_fidelity.rs` (`pg_*`), `reconnect.rs`, `scale.rs`, `debugger.rs`, `driver-pg/tests/pg.rs`, `pooler.rs` | admin-*-postgres, dev-postgres, navigator-objects, navigator-search, arrays-postgres, comments-postgres, edit-many-postgres, explain-*-postgres, generate-ddl-many-pg, schema-objects-postgres, vault-postgres, readme-postgres, qa-grid-*-pg, qa-nav-pg-*, qa-sql-pg-*, qa3-schema-*-pg, qa3-mcp-readonly-postgres, qa-perf-ok-cancel-postgres |
| CockroachDB, YugabyteDB, TimescaleDB | `conformance.rs` (`qa_fixdbs_cockroach_*`, `qa_fixdbs_yugabyte_sharding`, `qa_fixdbs_timescale_catalogs`) | qa2-dbs-ok-cockroach-edit, qa2-dbs-cockroach-*, qa2-dbs-yugabyte-ddl-sharding, qa2-dbs-timescale-*, qa-nav-cockroach-system-schemas |
| MySQL | `conformance.rs`, `value_fidelity.rs` (`mysql_*`), `reconnect.rs` | admin-*-mysql, qa-sql-*-mysql-*, qa-grid-ok-batch-mysql, qa-nav-mysql-*, qa-conn-reconnect-mysql, qa3-schema-compare-mysql-check, qa3-url-mysql-sslmode |
| MariaDB | `conformance.rs`, `value_fidelity.rs` (`mariadb_*`) | admin-sessions-mariadb, admin-statistics-mariadb, qa2-dbs-*-mariadb-*, qa-xfer-fake-mariadb-defaults, qa3-schema-ok-roundtrip-mariadb |
| SQL Server | `conformance.rs`, `value_fidelity.rs` (`mssql_*`), `reconnect.rs` | admin-sessions-mssql, qa-sql-*-mssql-*, qa-grid-mssql-*, qa-nav-ok-sqlserver-table-editor, qa-perf-*-mssql*, qa-xfer-ok-transfer-mssql, qa3-schema-compare-mssql-* |
| Oracle | `oracle.rs` | oracle-instantclient, qa-ora-* (30 scenari), qa3-schema-compare-oracle, qa-ora-copy-to-oracle |
| ClickHouse | `conformance.rs` | qa-sql-clickhouse-*, qa-xfer-clickhouse-*, qa-xfer-migrate-clickhouse-steps, qa-nav-clickhouse-alter-type, qa2-dbs-clickhouse-generate-fixedstring, qa3-url-ok-clickhouse |
| SQLite | `conformance.rs`, `value_fidelity.rs` (`sqlite_later_rows_of_other_types_keep_their_values`), `reconnect.rs` | tutti gli scenari `-sqlite` (100) e `qa3-mcp-*` |
| DuckDB | `conformance.rs`, `value_fidelity.rs` (`duckdb_special_values_and_bits`) | duckdb-download, readme-duckdb, qa-grid-duckdb-display, qa-nav-duckdb-enum-details, qa2-dbs-duckdb-*, qa2-dbs-ok-duckdb-files, qa-conn-duckdb-memory-url, qa-sql-ok-history-duckdb |
| libSQL | `conformance.rs` (`qa_fixdbs_libsql_large_results`) | qa2-dbs-libsql-large-result, qa2-dbs-ok-libsql-transaction, qa-conn-libsql-url-tls, qa3-url-libsql-tls-param |
| MongoDB | `mongo.rs`, `value_fidelity.rs` (`mongo_later_documents_keep_their_values_and_fields`) | mongo, qa-sql-ok-mongo-shell, qa-sql-mongo-*, qa-grid-mongo-late-fields, qa-xfer-mongo-*, qa-nav-mongo-table-editor, qa-conn-mongo-authsource, qa3-url-ok-mongo-srv |
| Redis | `redis.rs` | redis-browse, redis-editor, redis-edit, redis-readonly |
| Elasticsearch / OpenSearch | `search.rs` | search-connect, search-browse, search-editor, search-paging, search-edit, search-guards, search-readonly |
| Cassandra / ScyllaDB | `cassandra.rs` | cassandra |
| Trino (ADBC) | `bridges.rs` | trino-adbc, qa2-dbs-trino-* |
| ODBC (SQL Server) | `odbc.rs` | odbc-sqlserver, qa2-dbs-odbc-* |
| Snowflake, BigQuery, Databricks, Db2, ADBC con libreria propria | nessuno | nessuno (drivers-registry prova solo il registro) |

## Buchi

In ordine di impatto sull'utente. Ogni riga dice il tipo di test da scrivere.

| # | Buco | Stato | Test da scrivere |
|---|---|---|---|
| 1 | Installer di Windows e aggiornamento vero: installare, aggiornare da una versione pubblicata, disinstallare, con connessioni, password del keychain, script e impostazioni che restano | ❌ | Lista scritta in [COLLAUDO.md](COLLAUDO.md) (sezione 1), con lo script `scripts/collaudo-profile.ps1` che copia e confronta il profilo della versione vecchia; da spuntare su macchina vera; `qa4-inst-upgrade-real-installer` |
| 2 | Copia portatile: cartella `data` accanto all'eseguibile, dati e log lì dentro, aggiornamento sostituendo l'eseguibile | ✅ `portable-data-folder` | E2E `portable-data-folder` (avvio con `data` vuota, una connessione e uno script salvati, riavvio, controllo dei file) |
| 3 | Blocco per inattività con master password (chiude le connessioni di produzione) | ✅ `app-lock-idle-sqlite` | E2E `app-lock-idle-sqlite` con i minuti ridotti da impostazione di prova |
| 4 | Attività pianificate: backup PostgreSQL / MySQL, fuso orario, ritentativo dopo un errore, esecuzione di un orario perso alla riapertura | ✅ `schedules-missed-run-sqlite`, `schedules-backup-postgres` | E2E `schedules-backup-postgres`, `schedules-missed-run-sqlite` |
| 5 | Caricamento su bucket (S3, GCS, R2, MinIO) e su URL firmato | ✅ | Fatto: servizio `s3` (SeaweedFS: MinIO non pubblica più immagini con tag fisso) in `docker/compose.yml`, profilo `s3`, e conformance `upload.rs`. E2E `qa-xfer-upload-s3` dal form: fatto |
| 6 | Manutenzione, parametri, dashboard, utenti e statistiche su SQL Server, MariaDB e ClickHouse; utenti e ruoli solo provati a livello di conformance | ✅ | Fatti e verdi: `admin-users-mssql`, `admin-parameters-mysql` (anche MariaDB), `admin-parameters-mssql`, `admin-dashboard-mysql` (anche MariaDB). Scritti prima rossi per bug, ora corretti: `admin-maintenance-postgres`, `-mysql`, `-mssql` (B5, B6), `admin-users-clickhouse` (B7) |
| 7 | Tunnel SSH con agente SSH, e certificati client per PostgreSQL e MySQL | ✅ | Fatto: `ssh_agent_tunnel` in `platypus-net`; `client_certificate_postgres`/`_mysql` con i container `postgres-mtls` e `mysql-mtls` (profilo `tls`, certificati di prova in `docker/tls`) |
| 8 | Server MCP su Oracle, SQL Server, ClickHouse, Redis, Elasticsearch, Cassandra (sola lettura e conferma delle scritture) | 🟡 | Fatto `qa-mcp-drivers-matrix` per SQL Server, ClickHouse e Redis (sola lettura rifiuta DML, DDL, TRUNCATE e comandi Redis; la conferma chiede prima delle scritture). Oracle scritto ma salta senza Instant Client (`PLATYPUS_E2E_ORACLE_DRIVERS`); Cassandra ed Elasticsearch non coperti (container solo nei loro profili) |
| 9 | Import da DataGrip, pgAdmin e `pg_service.conf`; password da `.pgpass` | ✅ `import-datagrip-sqlite`, `import-pgadmin-sqlite`, `import-pg-service-sqlite`, `qa-conn-pgpass-postgres` | E2E `import-datagrip-sqlite`, `import-pgadmin-sqlite`, `import-pg-service-sqlite`, `qa-conn-pgpass-postgres` con file di prova |
| 10 | Export Markdown, HTML, Parquet, Excel, JSON, SQL INSERT: uno scenario che li scrive tutti e li rilegge (anche su un database di rete); compressione zstd | ✅ `export-formats-postgres` | E2E `export-formats-postgres` con riletture (lettore del piè di pagina Parquet in `parquet.mjs`); conformance `exports_every_format_and_reads_it_back` |
| 11 | Import Parquet ed Excel (fogli come tabelle) e dati da file come database (JSON, Parquet, Excel) | ✅ `import-parquet-xlsx-sqlite`, `data-files-as-databases-sqlite` | E2E `import-parquet-xlsx-sqlite`, `data-files-as-databases-sqlite` |
| 12 | Notifica di sistema per una query lunga; icona nel vassoio (Quick query); Linux e macOS | ❌ | Liste scritte in [COLLAUDO.md](COLLAUDO.md) (sezioni 2, 3 e 4: sistema, Linux, macOS), da spuntare su macchina vera; `windows-sqlite` già prova la finestra |
| 13 | Snowflake, BigQuery, Databricks, IBM Db2, ADBC con libreria propria | 🟡 | ADBC generico: fatto con il driver di SQLite del registro (`adbc_generic.rs`, anche quello di DuckDB con l'entrypoint `duckdb_adbc_init`). Db2: non pratico (driver ODBC manuale, container di oltre 2 GB). Snowflake, BigQuery, Databricks: solo a mano |
| 14 | Esegui script con `psql`/`mysql`/`sqlcmd`/`sqlite3` locali | 🟡 `editor-cli-script-postgres` fatto con il psql vero (cartella dei client in `clients.mjs`; salta se non c'è); bug B4 risolto | E2E `editor-cli-script-postgres`; restano `mysql`, `sqlcmd`, `sqlite3` |
| 15 | Migration status con Liquibase, Prisma, Alembic | ✅ `schema-migrations-sqlite` | Estendere `schema-migrations-sqlite` con tre cartelle di prova |
| 16 | Query across databases oltre i limiti (512 MB) e con più driver (PostgreSQL con MySQL) | ✅ `analysis-across-postgres-mysql` | Join tra PostgreSQL e MySQL, NULL, UTF-8, date, tabella mancante, tipi diversi, filtro errato, sorgente eliminata, taglio a 512 MB con il messaggio "Partial" |
| 17 | Pannello mappa su PostGIS e MySQL; pannello valore con XML, immagini e salvataggio | 🟡 `grid-value-panel-binary-sqlite` fatto (bug B3 risolto), `grid-map-postgres` fatto senza PostGIS vero (nessuna immagine di prova lo ha: EWKB e WKB inviati come testo/bytea) | E2E `grid-map-postgres`, `grid-value-panel-binary-sqlite` |
| 18 | Query builder visuale: filtri, colonne, esecuzione | ✅ `query-builder-sqlite` | E2E `query-builder-sqlite` (join, WHERE, esegui, apri nell'editor) |
| 19 | Temi con alto contrasto, zoom dell'interfaccia, sette lingue (oltre l'italiano) | ✅ `theme-sqlite`, `languages-sqlite`, `qa-ui-zoom-keys` | Estendere `theme-sqlite` e `languages-sqlite`; E2E `qa-ui-zoom-keys` |
| 20 | Linter e controllo ortografico, outline, cursori multipli | ✅ `editor-more-sqlite` | Estendere `editor-more-sqlite` con un caso per ognuno |

## Bug trovati dal collaudo

Test scritti dal gruppo S che falliscono perché è l'app a sbagliare: il test resta com'è e documenta il bug. Stato `aperto` finché la correzione non è unita in `main`. B9-B92 vengono dal giro per aree (scenari `qa4-*`, uno per bug, tutti corretti), B93-B142 dal secondo giro con tester nuovi (scenari `qa5-*`, tutti corretti), B1-B8 dai test dei buchi; in fondo le correzioni della revisione del codice (R1-R20) e quelle della revisione di sicurezza (SR1-SR17). Fra parentesi il commit della correzione.

| # | Area | Bug | Test o scenario | Stato |
|---|---|---|---|---|
| B1 | ADBC generico | La libreria scelta doveva esportare `AdbcDriverInit`: il driver ADBC di DuckDB (`duckdb_adbc_init`) non si caricava ("GetProcAddress failed"). Ora il form ha il campo facoltativo "Entrypoint" | `adbc_generic.rs` `generic_adbc_loads_duckdb` | risolto |
| B2 | Certificati client MySQL | Con un file del certificato client mancante l'errore non nominava il file (PostgreSQL sì). Ora MySQL controlla certificato e chiave e li nomina | `client_certificate.rs` `client_certificate_mysql` (controllo del nome per certificato e chiave, su entrambi) | risolto |
| B3 | Pannello valore (F7) | Un valore BLOB che è un'immagine (PNG, JPEG, …) non si vede: il pannello la mostra con un URL `blob:` e la CSP di `tauri.conf.json` ha `img-src 'self' data:`, senza `blob:`, quindi la WebView la blocca (l'elemento `img.value-image` resta a larghezza 0). Provato anche a mano: lo stesso PNG con un URL `data:` si carica. Correzione: aggiungere `blob:` a `img-src`, oppure creare l'URL `data:` in `ValuePanel.tsx`. | grid-value-panel-binary-sqlite | risolto (da verificare in E2E) |
| B4 | Script con il client da riga di comando | Annullare uno script lanciato con `psql` (Cancel) termina il processo del client ma non manda al server la richiesta di annullamento: un'istruzione lunga (`SELECT pg_sleep(120)`) resta attiva sul server fino alla fine. Il client si ferma e l'app dice "psql was stopped". Il gestore dell'annullamento normale (`pg_cancel_backend`) non è usato per questa strada. Ora psql parte con `PGAPPNAME` unico per l'esecuzione; a Cancel, dopo aver chiuso il client, l'app apre una connessione con le stesse impostazioni e termina la sessione del client (`pg_terminate_backend`). Lo stesso per `mysql` (sessione trovata dal `_pid` del client in `performance_schema.session_account_connect_attrs`, poi `KILL`) e per `sqlcmd` (`host_process_id` in `sys.dm_exec_sessions`, poi `KILL`; senza VIEW SERVER STATE e ALTER ANY CONNECTION non la vede e non la chiude). Test `cancelling_ends_the_statement_on_the_server` (psql e mysql veri contro i database di sviluppo) | editor-cli-script-postgres (l'ultima asserzione, eseguita dopo tutte le altre) | risolto (da verificare in E2E) |
| B5 | Manutenzione delle tabelle | Su una connessione in sola lettura Vacuum (PostgreSQL), Optimize (MySQL) e Reindex (SQL Server) partono e dicono "fatto": la finestra apre una sessione a parte che `state.writable` non tratta come in sola lettura. Causa: il comando `maintain_table` rifiutava già (`state.writable`, letto dalla connessione salvata anche per la sessione laterale), ma lo scenario apriva il menu della tabella omonima sotto la connessione scrivibile, la prima dell'albero. Ora `refusedReadOnly` sceglie la riga sotto la connessione in sola lettura (`treeMenu` con `rowIn`) e PostgreSQL controlla `last_vacuum` della tabella giusta; test Rust `read_only_connections_refuse_maintenance` (tutte le operazioni, sessione della scheda e laterale) e `maintenance_is_no_read` (VACUUM, OPTIMIZE, REINDEX, DBCC… scritti a mano non passano su una connessione in sola lettura) | `admin-maintenance-postgres`, `-mysql`, `-mssql` | risolto (da verificare in E2E) |
| B6 | Manutenzione MySQL | Su una tabella che non esiste `ANALYZE TABLE` restituisce righe di errore e l'app dice "Analyze done.": `run_maintenance` ignora le righe Error / "Operation failed" | `admin-maintenance-mysql` | risolto (da verificare in E2E) |
| B7 | Utenti ClickHouse | Ogni utente mostra la scadenza 1970-01-01 00:00:00: per "mai" `system.users.valid_until` vale `['1970-01-01 00:00:00']`, quindi `empty()` è falso; togliere la data in Modifica lascia il 1970 | `admin-users-clickhouse` | risolto (da verificare in E2E) |
| B8 | Utenti SQL Server | Il gruppo dei permessi a livello di database ha l'etichetta vuota nei dettagli dell'utente (`objectLabel` restituisce "" per la classe database) | (non verificato da uno scenario) | risolto (da verificare in E2E) |
| B9 | Connessioni | Test su una connessione "Chiedi ogni volta" falliva con "password missing" invece di chiedere la password | `qa4-conn-ask-password-test` | risolto (856e7b06) |
| B10 | Connessioni | Test su un file di testo scelto come SQLite diceva Connesso: ora il file viene letto e rifiutato | `qa4-conn-sqlite-not-a-database` | risolto (207bbb86) |
| B11 | Connessioni | Il campo file di SQLite/DuckDB rifiutava un percorso tra virgolette ("Copia come percorso") o con uno spazio iniziale | `qa4-conn-sqlite-quoted-path` | risolto (856e7b06, bfc2cd0f) |
| B12 | Connessioni | Tunnel SSH con host vuoto: Test aspettava 10 secondi con "ops@:22" invece di rifiutare subito | `qa4-conn-ssh-empty-host` | risolto (bfc2cd0f) |
| B13 | Connessioni | Host vuoto su PostgreSQL: errore "TLS handshake: invalid dns name" invece di "manca l'host" | `qa4-conn-empty-host-test` | risolto (bfc2cd0f) |
| B14 | Connessioni | SQL Server: l'host "localhost,51433" non veniva separato in host e porta (errore I/O grezzo) | `qa4-conn-mssql-host-comma` | risolto (bfc2cd0f) |
| B15 | Connessioni | Un URL incollato con `;` o `,` finale finiva nel nome del database | `qa4-conn-url-trailing-punct` | risolto (bfc2cd0f) |
| B16 | Connessioni | Gli errori dell'app stessi (Redis db, apertura SQLite, password mancante) restavano in inglese nell'interfaccia italiana | `qa4-conn-italian-driver-errors` | risolto (856e7b06, e1f3aa0f) |
| B17 | Connessioni | Due connessioni potevano avere lo stesso nome | `qa4-conn-duplicate-name` | risolto (bfc2cd0f) |
| B18 | Connessioni | L'import da DBeaver saltava ClickHouse, DuckDB, Oracle, libSQL e Snowflake come non supportati | `qa4-conn-import-dbeaver-clickhouse` | risolto (2ac89266) |
| B19 | Connessioni | Backup (`pg_dump`/`mysqldump`) e comando della password partivano senza `CREATE_NO_WINDOW`: ogni esecuzione apriva una console su Windows | (trovato dalla sessione principale) | risolto (7eef555a) |
| B20 | Navigator | Trascinare una routine nell'editor scriveva `schema."f_one(x integer)"`: ora scrive la chiamata | `qa4-nav-drag-routine-quoted` | risolto (c16ce390) |
| B21 | Navigator | Il filtro per nome dello schema nascondeva lo schema e `schema.tabella` non trovava nulla | `qa4-nav-filter-schema-name` | risolto (c16ce390) |
| B22 | Navigator | Dopo lo stop e il riavvio del server PostgreSQL la sessione aperta non si riprendeva (os error 10061) fino a Refresh: ora la sessione inattiva viene sondata prima dell'istruzione e Open data rilegge un tab fallito | `qa4-nav-pg-server-restart` | risolto (afd814bc, 17f10bf0) |
| B23 | Navigator | Il segnalibro restava dopo l'eliminazione dell'oggetto | `qa4-nav-bookmark-after-drop` | risolto (c16ce390) |
| B24 | Navigator | Copy name e Copy qualified name davano lo stesso testo su tabelle e viste | `qa4-nav-copy-name-same` | risolto (c16ce390) |
| B25 | Editor SQL | Un parametro negativo (`10-:p` con -5) diventava `10--5` e commentava il resto; stesso per `@set v=-5`: risultato sbagliato in silenzio | `qa4-sql-param-negative-comment` | risolto (62ee52ed) |
| B26 | Editor SQL | MySQL `SET autocommit=0` (e SQL Server `IMPLICIT_TRANSACTIONS ON`) seguito da INSERT: nessun badge Transaction, chiudere il tab perdeva la riga | `qa4-sql-mysql-autocommit-off` | risolto (21ed9f3b) |
| B27 | Editor SQL | PostgreSQL: una selezione di due istruzioni con Ctrl+Invio mostrava solo il primo risultato | `qa4-sql-pg-selection-many-statements` | risolto (8f229be2) |
| B28 | Editor SQL | La cronologia salvava in chiaro `CREATE ROLE … PASSWORD` | `qa4-sql-history-password` | risolto (7d3b4fcf) |
| B29 | Editor SQL | SQLite/ClickHouse: una selezione di più istruzioni dava il messaggio grezzo "Multiple statements provided" | `qa4-sql-selection-many-statements-message` | risolto (8f229be2) |
| B30 | Editor SQL | SQLite `?1 ?2 ?1` apriva tre campi e poi dava un errore di sintassi | `qa4-sql-sqlite-numbered-params` | risolto (62ee52ed) |
| B31 | Editor SQL | ClickHouse: l'operatore ternario `1 ? 'a' : 'b'` apriva la finestra dei parametri | `qa4-sql-clickhouse-ternary` | risolto (62ee52ed) |
| B32 | Editor SQL | Il formattatore spezzava `U&'..'` in `U & '...'` e `!<` di T-SQL in `! <` | `qa4-sql-format-unicode-string` | risolto (5da8431a) |
| B33 | Editor SQL | `@export` diceva "Exported 1 rows" al plurale | `qa4-sql-export-directive-plural` | risolto (db8b55ee) |
| B34 | Analisi | Grafici: i valori sotto 0,005 si leggevano come 0 | `qa4-ana-chart-small-values` | risolto (c76f6907) |
| B35 | Analisi | Profilo: le cifre delle schede erano troncate a 1280 px | `qa4-ana-profile-truncated-values` | risolto (21107747, d8468e14) |
| B36 | Analisi | Pivot: il NULL SQL e il testo 'NULL' finivano nella stessa colonna | `qa4-ana-pivot-null-vs-text` | risolto (c800d03c) |
| B37 | Analisi | SQL sul risultato: i timestamptz erano letti nel fuso locale, mentre la griglia mostra quello del database | `qa4-ana-result-sql-timezone` | risolto (df7884ef, 1146a0ea) |
| B38 | Analisi | ERD: il nome lungo di una colonna usciva dal riquadro | `qa4-ana-erd-long-column-name` | risolto (1b3ce5ad) |
| B39 | Analisi | Explain PostgreSQL: Crea indice perdeva lo schema della tabella | `qa4-ana2-explain-index-schema` | risolto (a37d2a05, 3432bf20) |
| B40 | Analisi | Notebook: l'export HTML scriveva 500 righe sotto "1.200 righe" senza avvisare | `qa4-ana2-notebook-export-truncated` | risolto (053491f9) |
| B41 | Analisi | Explain: `lower(email) =` suggeriva un indice su `(email)` invece che sull'espressione | `qa4-ana2-explain-index-expression` | risolto (f0478435, 8c985b85) |
| B42 | Analisi | Explain analyze di UPDATE/DELETE su MySQL mostrava `<not executable by iterator executor>` | `qa4-ana2-explain-mysql-dml-analyze` | risolto (137e56e0, 5335b559) |
| B43 | Analisi | Notebook: un elenco numerato che parte da 3 si leggeva da 1 | `qa4-ana2-notebook-markdown-list-start` | risolto (053491f9) |
| B44 | Analisi | Explain analyze PostgreSQL: `Planning` in JSON grezzo e `Triggers` vuoto | `qa4-ana2-explain-pg-planning-json` | risolto (a37d2a05) |
| B45 | Analisi | Notebook e editor: un valore di parametro come 007 o 0042 entrava come numero e dava righe sbagliate | `qa4-ana2-notebook-param-leading-zero` | risolto (62ee52ed) |
| B46 | Griglia | SQL Server: una colonna hierarchyid faceva fallire ogni SELECT ("invalid token type 0") | `qa4-grid-mssql-hierarchyid` | risolto (6a428157) |
| B47 | Griglia | L'ordinamento dall'intestazione su un join con nomi doppi dava `ORDER BY id` ambiguo | `qa4-grid-pg-sort-join-ambiguous` | risolto (0e4bb5a2) |
| B48 | Griglia | Il filtro Uguale falliva su json/xml di PostgreSQL e su text/ntext/xml/geography/sql_variant di SQL Server | `qa4-grid-pg-filter-json-equal` | risolto (1df4520d) |
| B49 | Griglia | PostgreSQL: `time` 24:00:00 mostrava un errore di cast | `qa4-grid-pg-time-2400` | risolto (5303e772) |
| B50 | Griglia | Svuotare una cella data, ora, booleana o JSON scriveva `''` e falliva: ora scrive NULL | `qa4-grid-pg-clear-date-cell` | risolto (fe93e45e) |
| B51 | Griglia | DuckDB: eliminare una riga con MAP dava uno script di annullamento che non la ripristinava | `qa4-grid2-duckdb-map-undo` | risolto (d4e0a7fd) |
| B52 | Griglia | SQLite: colonne DECIMAL/NUMERIC o senza tipo con soli interi erano trattate come intere e 9.99 veniva rifiutato | `qa4-grid2-sqlite-decimal-whole` | risolto (bd8e42b2) |
| B53 | Griglia | Lo stesso su libSQL | `qa4-grid2-libsql-decimal-whole` | risolto (bd8e42b2) |
| B54 | Griglia | DuckDB: un MAP mostrato come `{k: 1}` non si poteva riscrivere | `qa4-grid2-duckdb-map-edit` | risolto (d4e0a7fd) |
| B55 | Griglia | DuckDB: una data BC (`0044-03-15 BC`) non si riscriveva e l'annullamento falliva | `qa4-grid2-duckdb-bc-date-undo` | risolto (d4e0a7fd) |
| B56 | Griglia | DuckDB: le colonne GENERATED ALWAYS non erano riconosciute, Ctrl+D e annullamento fallivano | `qa4-grid2-duckdb-generated-duplicate` | risolto (f5ff0e15) |
| B57 | Griglia | DuckDB: UHUGEINT da 2^127 in su appariva negativo | `qa4-grid2-duckdb-uhugeint` | risolto (849f0bbc) |
| B58 | Griglia | DuckDB: TIMETZ perdeva l'offset | `qa4-grid2-duckdb-timetz` | risolto (849f0bbc, ef55982b) |
| B59 | Impostazioni e tab | Con il pulsante Cancel nascosto dalle impostazioni della barra un'istruzione in corso non si poteva fermare | `qa4-ui-cancel-hidden` | risolto (6972602c) |
| B60 | Impostazioni e tab | I tab non si potevano riordinare trascinandoli (ora anche Alt+Maiusc+Sinistra/Destra) | `qa4-ui-tab-reorder` | risolto (48993668) |
| B61 | Impostazioni e tab | Editor delle scorciatoie: Ctrl+Canc/Backspace cancellava la scorciatoia invece di registrarla | `qa4-ui-shortcut-delete-combo` | risolto (a1b4ab3d) |
| B62 | Impostazioni e tab | Il gruppo di scorciatoie mostrava il nome grezzo `dashboard.widget` | `qa4-ui-shortcuts-scope-header` | risolto (a1b4ab3d) |
| B63 | Impostazioni e tab | Un tab non valido nell'area di lavoro salvata lasciava l'app vuota | (test unitario del workspace) | risolto (2dd964f6) |
| B64 | Import / export | ClickHouse: l'export SQL INSERT di Map e Tuple scriveva JSON tra apici e non si poteva rieseguire | `qa4-xfer-clickhouse-sql-map-tuple` | risolto (22997cba, 3f800ed0) |
| B65 | Import / export | Copia da PostgreSQL a MySQL: l'ora diventava TIME senza frazione e 23:59:59.999999 diventava 24:00:00 | `qa4-xfer-mysql-copy-time-fraction` | risolto (f6ed4b2e) |
| B66 | Import / export | Copia di date da PostgreSQL a ClickHouse: i valori fuori intervallo (1000-01-01, 9999) erano ridotti in silenzio; ora si rifiutano | `qa4-xfer-clickhouse-copy-date-clamp` | risolto (504d5826) |
| B67 | Import / export | Un import CSV fallito in ClickHouse diceva "Nothing was imported" ma lasciava 19.932 righe: ora passa da una tabella di appoggio | `qa4-xfer-clickhouse-import-partial` | risolto (504d5826, 026c8960) |
| B68 | Import / export | Un campo CSV vuoto in una colonna ClickHouse non Nullable scriveva 0 o 1970-01-01 in silenzio: ora si rifiuta | `qa4-xfer-clickhouse-import-null-to-zero` | risolto (504d5826) |
| B69 | Sicurezza e MCP | Una CTE con DELETE/UPDATE … RETURNING seguita da SELECT aggirava il blocco delle scritture senza filtro | `qa4-safe-guard-cte-dml` | risolto (d17ae3f2) |
| B70 | Sicurezza e MCP | Le istruzioni degli agenti con PASSWORD erano salvate in chiaro nel registro delle chiamate e nella cronologia | `qa4-mcp-history-password` | risolto (7d3b4fcf, 574c7075) |
| B71 | Sicurezza e MCP | Redis in sola lettura: un agente leggeva `CONFIG GET requirepass` e `ACL LIST` (segreti) | `qa4-mcp-redis-config-secrets` | risolto (0ce3e612) |
| B72 | Sicurezza e MCP | ClickHouse in sola lettura: le funzioni tabella deltaLake, iceberg, hudi, fileCluster facevano richieste verso l'esterno | `qa4-mcp-clickhouse-table-functions` | risolto (42e5ede0, 2fec13b4) |
| B73 | Sicurezza e MCP | La finestra di conferma delle scritture degli agenti, alta al massimo 260 px, nascondeva un `OR 1=1` dopo righe vuote | `qa4-mcp-agent-write-hidden-tail` | risolto (4d315864) |
| B74 | Sicurezza e MCP | Un file di configurazione di un client con BOM UTF-8 era segnato come illeggibile | `qa4-mcp-client-bom` | risolto (49f3b384) |
| B75 | Schema | Compare PostgreSQL: una colonna generata diventava `DEFAULT (a*2)` e falliva | `qa4-schema-compare-pg-generated` | risolto (6a6ab799) |
| B76 | Schema | Compare PostgreSQL: l'identity si perdeva in silenzio | `qa4-schema-compare-pg-identity` | risolto (6a6ab799) |
| B77 | Schema | Compare PostgreSQL: gli indici su espressione fallivano, quelli parziali perdevano il WHERE, BRIN diventava btree | `qa4-schema-compare-pg-expression-index` | risolto (6a6ab799) |
| B78 | Schema | Compare SQL Server: identity diventava int, le colonne calcolate normali, si perdevano COLLATE, clustered, filtered e INCLUDE | `qa4-schema-compare-mssql-identity-computed` | risolto (6a6ab799) |
| B79 | Schema | Compare PostgreSQL: il tipo enum era qualificato con lo schema di origine | `qa4-schema-compare-pg-enum-type` | risolto (6a6ab799) |
| B80 | Schema | Compare PostgreSQL: una tabella partizionata diventava normale, senza avviso | `qa4-schema-compare-pg-partitioned` | risolto (6a6ab799) |
| B81 | Schema | Compare MariaDB: una colonna JSON produceva un CHECK doppio | `qa4-schema-compare-mariadb-json` | risolto (6a6ab799) |
| B82 | Schema | Compare MySQL: il prefisso dell'indice su testo `body(50)` si perdeva (ERR 1170), FULLTEXT diventava normale | `qa4-schema-compare-mysql-text-index` | risolto (6a6ab799, 9ec7f4d7) |
| B83 | Schema | Compare ClickHouse: la chiave di ordinamento compariva come indice "ORDER BY", PARTITION BY e TTL si perdevano | `qa4-schema-compare-clickhouse-keys` | risolto (6a6ab799) |
| B84 | Schema | PostgreSQL: un trigger BEFORE DELETE con altri eventi restituiva NEW e annullava ogni DELETE | `qa4-schema-trigger-pg-before-delete` | risolto (4c297b12) |
| B85 | Schema | SQL Server: ALTER COLUMN senza COLLATE azzerava la collation case-sensitive | `qa4-schema-mssql-alter-column-collation` | risolto (6a6ab799) |
| B86 | Schema | SQL Server: int in bigint con un default falliva (il vincolo di default non veniva tolto e rimesso) | `qa4-schema-mssql-alter-type-default` | risolto (6a6ab799) |
| B87 | Schema | MySQL: allargare una chiave primaria AUTO_INCREMENT falliva (ERR 1075) | `qa4-schema-mysql-pk-auto-increment` | risolto (6a6ab799, 975d979f) |
| B88 | Schema | SQLite: la ricostruzione della tabella azzerava `sqlite_sequence` | `qa4-schema-sqlite-rebuild-autoincrement` | risolto (c4360b37) |
| B89 | Amministrazione | SQL Server: nei parametri "(max)" e "(min)" diventavano unità di misura | `qa4-admin-mssql-ft-unit` | risolto (ed6eced0) |
| B90 | Amministrazione | MySQL: le attività mostravano le emoji come `?` | `qa4-admin-mysql-emoji-statement` | risolto (092f3c38) |
| B91 | Amministrazione | MariaDB: le attività mostravano le emoji come `????` | `qa4-admin-mariadb-emoji-statement` | risolto (092f3c38) |
| B92 | Amministrazione | Attività su una connessione irraggiungibile: 0 sessioni e solo un avviso | `qa4-admin-activity-connect-failed` | risolto (563b3b4e) |
| B93 | Connessioni | Un file `.env` con BOM UTF-8 perdeva la prima variabile | `qa5-conn-dotenv-bom` | risolto (a4cff777) |
| B94 | Connessioni | `pgpass.conf` con BOM non trovava mai la prima riga | `qa5-conn-pgpass-bom` | risolto (a4cff777) |
| B95 | Connessioni | `pg_service.conf` con BOM perdeva il primo servizio e `servers.json` di pgAdmin con BOM era rifiutato | `qa5-conn-import-bom` | risolto (a4cff777) |
| B96 | Connessioni | Un servizio di `pg_service.conf` senza `dbname` importava il nome del servizio come database invece del nome utente | `qa5-conn-import-service-database` | risolto (a4cff777) |
| B97 | Connessioni | Un URL `redis://:pw@host:6380/3` incollato dava alla connessione il nome "3" | `qa5-conn-redis-url-name` | risolto (c4d57e61) |
| B98 | Connessioni | La ricerca nel selettore dei driver non conosceva gli alias (mssql, pg, valkey, mongo) | `qa5-conn-picker-aliases` | risolto (b06392af) |
| B99 | Connessioni | Invio in un campo di testo del form di connessione non salvava | `qa5-conn-enter-saves` | risolto (532a68cf) |
| B100 | Navigator | Nessuna selezione multipla da tastiera nell'albero (Maiusc+frecce, Ctrl+Spazio) | `qa5-nav-keyboard-multiselect` | risolto (803684d7) |
| B101 | Navigator | PageSu e PageGiù non facevano nulla nell'albero | `qa5-nav-keyboard-page-keys` | risolto (803684d7) |
| B102 | Navigator | PostgreSQL: Generate DDL perdeva il COLLATE delle colonne | `qa5-nav-pg-ddl-collation` | risolto (de16938e) |
| B103 | Navigator | PostgreSQL: una colonna serial usava `nextval` senza creare la sequenza | `qa5-nav-pg-ddl-serial-sequence` | risolto (de16938e, 0d2c59b5) |
| B104 | Navigator | PostgreSQL: il DDL lasciava fuori reloptions, ROW LEVEL SECURITY e policy | `qa5-nav-pg-ddl-options-rls` | risolto (de16938e) |
| B105 | Navigator | PostgreSQL: il DDL di una tabella con INHERITS non indicava il genitore | `qa5-nav-pg-ddl-inherits` | risolto (de16938e) |
| B106 | Navigator | SQL Server: il DDL perdeva COLLATE, SPARSE, i nomi dei vincoli di default e le descrizioni | `qa5-nav-sqlserver-ddl-collation` | risolto (6b3ef184) |
| B107 | Navigator | PostgreSQL: i dettagli nascondevano il WHERE degli indici parziali, INCLUDE, DESC e le azioni ON DELETE / ON UPDATE / DEFERRABLE delle chiavi esterne | `qa5-nav-details-index-fk-hide` | risolto (6b3ef184) |
| B108 | Editor SQL | Il formattatore spezzava gli operatori PostgreSQL `<->`, `<#>`, `#-`, `@?`, `~~*`, `!~~` | `qa5-sql-format-pg-operators` | risolto (b17bf916) |
| B109 | Editor SQL | Il formattatore spezzava gli esponenti con segno (`1e-7` diventava `1e - 7`) | `qa5-sql-format-exponent` | risolto (b17bf916) |
| B110 | Editor SQL | Il formattatore spezzava `+=`, `-=`, `*=`, `/=`, `%=`, `|=` di T-SQL e `$action` di MERGE | `qa5-sql-format-mssql-compound` | risolto (b17bf916, 35e3a55a) |
| B111 | Editor SQL | Dopo la richiesta della password dall'albero l'editor non aveva il completamento (catalogo mai richiesto) | `qa5-sql-completion-password-prompt` | risolto (1e9b1e5d) |
| B112 | Griglia | SQLite: in una tabella vuota una colonna NUMERIC o DECIMAL accettava "1,5" e lo salvava come testo | `qa5-grid-sqlite-empty-table-number-text` | risolto (b3d92135) |
| B113 | Griglia | MySQL: il filtro rapido Uguale su un FLOAT trovava 0 righe | `qa5-grid-mysql-filter-float` | risolto (c102ee4a) |
| B114 | Import / export | Un file JSON con BOM UTF-8 era importato come una riga e una colonna | `qa5-xfer-json-bom` | risolto (64c86f3c) |
| B115 | Import / export | L'export Excel scriveva i decimali oltre 2^53 come celle di testo | `qa5-xfer-xlsx-large-float` | risolto (3da4a3e7) |
| B116 | Import / export | Export all tables: un nome di tabella troppo lungo per un file (o riservato di Windows) annullava tutto l'export | (test unitario dell'export) | risolto (e8b2ce14) |
| B117 | Impostazioni e tab | Con tastiere non americane AltGr+[ e AltGr+] arrivavano come Ctrl+Alt, cioè Fold all / Unfold all: le parentesi quadre e graffe non si potevano scrivere nell'editor | `qa5-ui-altgr-brackets` | risolto (cbc9113b, 3b7873c4) |
| B118 | Impostazioni e tab | Ctrl+/ non commentava dove `/` richiede Maiusc (tastiera italiana) | `qa5-ui-italian-comment-key` | risolto (cbc9113b) |
| B119 | Sicurezza e MCP | Su una connessione di produzione `PRAGMA user_version=42` e `writable_schema=1` partivano senza conferma (SET e PRAGMA contati come "altro") | `qa5-safe-prod-pragma` | risolto (d1551290) |
| B120 | Sicurezza e MCP | La destinazione `kubectl` era passata come argomento nudo: `--kubeconfig=\\evil\share\kc` veniva letta come opzione | `qa5-safe-k8s-target-flag` | risolto (bbbe9665) |
| B121 | Sicurezza e MCP | `propose_sql` senza limite: 300 proposte aprivano 300 tab | `qa5-mcp-propose-flood` | risolto (c5fb4253) |
| B122 | Sicurezza e MCP | Settings > Agents, Install riscriveva i numeri nel JSON del client (interi grandi in notazione scientifica, `1E5` in `100000.0`) | `qa5-mcp-install-bignumber` | risolto (90e24732) |
| B123 | Sicurezza e MCP | L'errore MCP per un `connections.toml` non valido mostrava il percorso completo del file | (test unitario) | risolto (536c1757) |
| B124 | Schema | SQLite: il table editor scriveva `DEFAULT datetime('now')` senza parentesi e la ricostruzione falliva | `qa5-schema-sqlite-default-expression` | risolto (9ea425dd, fc91a9c6) |
| B125 | Schema | Compare SQLite: un indice su espressione era scritto come colonna | `qa5-schema-compare-sqlite-expression-index` | risolto (9ea425dd, fc91a9c6) |
| B126 | Schema | Compare SQLite: il WHERE di un indice parziale si perdeva | `qa5-schema-compare-sqlite-partial-index` | risolto (9ea425dd, fc91a9c6) |
| B127 | Schema | Compare SQLite: un WHERE modificato non era visto | `qa5-schema-compare-sqlite-partial-index-changed` | risolto (9ea425dd, fc91a9c6) |
| B128 | Schema | Compare SQLite: DEFERRABLE INITIALLY DEFERRED si perdeva | `qa5-schema-compare-sqlite-deferred-fk` | risolto (9ea425dd, fc91a9c6) |
| B129 | Schema | SQLite: la ricostruzione del table editor perdeva la chiave esterna differita | `qa5-schema-sqlite-rebuild-deferred-fk` | risolto (9ea425dd, fc91a9c6) |
| B130 | Schema | SQLite: la ricostruzione perdeva UNIQUE ON CONFLICT REPLACE | `qa5-schema-sqlite-rebuild-on-conflict` | risolto (9ea425dd, fc91a9c6) |
| B131 | Schema | Compare PostgreSQL: DEFERRABLE delle chiavi esterne si perdeva | `qa5-schema-compare-pg-deferrable-fk` | risolto (9ea425dd, fc91a9c6) |
| B132 | Schema | PostgreSQL: il table editor toglieva DEFERRABLE a una chiave esterna | `qa5-schema-pg-edit-deferrable-fk` | risolto (9ea425dd, fc91a9c6) |
| B133 | Schema | Compare PostgreSQL: UNIQUE NULLS NOT DISTINCT diventava UNIQUE normale | `qa5-schema-compare-pg-nulls-not-distinct` | risolto (9ea425dd, fc91a9c6) |
| B134 | Schema | Compare PostgreSQL: MATCH FULL diventava MATCH SIMPLE | `qa5-schema-compare-pg-match-full` | risolto (9ea425dd, fc91a9c6) |
| B135 | Schema | Compare PostgreSQL: i vincoli EXCLUDE erano lasciati fuori | `qa5-schema-compare-pg-exclusion` | risolto (9ea425dd, fc91a9c6) |
| B136 | Schema | Compare MySQL: una tabella partizionata diventava normale, senza avviso | `qa5-schema-compare-mysql-partitioned` | risolto (9ea425dd, fc91a9c6) |
| B137 | Schema | Compare MySQL: un CHECK NOT ENFORCED diventava attivo | `qa5-schema-compare-mysql-check-not-enforced` | risolto (9ea425dd, fc91a9c6) |
| B138 | Schema | Compare MySQL: un indice INVISIBLE diventava visibile | `qa5-schema-compare-mysql-invisible-index` | risolto (9ea425dd, fc91a9c6) |
| B139 | Schema | Compare MySQL: ENGINE e DEFAULT CHARSET / COLLATE della tabella si perdevano | `qa5-schema-compare-mysql-table-options` | risolto (9ea425dd, fc91a9c6) |
| B140 | Schema | Compare SQL Server: una chiave esterna o un CHECK con NOCHECK arrivava attivo | `qa5-schema-compare-mssql-disabled-fk` | risolto (9ea425dd, fc91a9c6) |
| B141 | Schema | Compare SQL Server: una tabella temporale diventava due tabelle normali | `qa5-schema-compare-mssql-temporal` | risolto (9ea425dd, fc91a9c6) |
| B142 | Schema | Compare MariaDB: WITH SYSTEM VERSIONING si perdeva | `qa5-schema-compare-mariadb-system-versioning` | risolto (9ea425dd, fc91a9c6) |

### Revisione del codice (R1-R20)

La revisione del codice del giro per aree (`cb477074..HEAD`) ha trovato venti problemi; tutti corretti, ognuno con il test del suo commit.

| # | Problema | File | Stato |
|---|---|---|---|
| R1 | Importazione in ClickHouse in modalità Replace: se lo spostamento dalla tabella di appoggio falliva dopo il TRUNCATE le righe erano perse | `import.rs` | risolto (026c8960) |
| R2 | La finestra di conferma delle scritture degli agenti mostrava i caratteri bidirezionali Unicode: l'SQL mostrato poteva differire da quello eseguito; ora sono segni visibili | `writes.ts` | risolto (55797687) |
| R3 | Il mascheramento delle credenziali non copriva Redis (`CONFIG SET requirepass`, `ACL SETUSER`, `AUTH`), DuckDB `CREATE SECRET`, SQL Server `CREATE CREDENTIAL`, le funzioni tabella ClickHouse | `security.rs` | risolto (5d3e4bea) |
| R4 | Il form rifiutava l'host vuoto anche per Oracle e per i driver plugin: ora una sola lista dei driver che lo ammettono, per form e backend | `ConnectionDialog.tsx` | risolto (d170701c) |
| R5 | Il controllo dell'indirizzo del backend pretendeva l'host anche per i plugin | `connections.rs` | risolto (d170701c) |
| R6 | MySQL: le chiavi `UNIQUE KEY` perdevano lunghezze di prefisso, ordine ed espressioni | `create.rs` | risolto (9ec7f4d7) |
| R7 | ClickHouse: un motore Replicated* con percorso ZooKeeper fisso univa la copia al gruppo di replica della sorgente | `catalog.rs` | risolto (975d979f) |
| R8 | Compare PostgreSQL: una partizione presente solo nella destinazione diventava DROP TABLE; ora il compare non cancella righe | `ddl.rs` | risolto (975d979f) |
| R9 | MySQL: `SET autocommit = 1` chiudeva la transazione anche se l'autocommit era già attivo (un `START TRANSACTION` restava invisibile) | `transaction.rs` | risolto (83865af8) |
| R10 | MySQL: togliere una colonna dalla chiave e renderla nullable dava ER_PRIMARY_CANT_HAVE_NULL | `ddl.rs` | risolto (975d979f) |
| R11 | SQL sul risultato: ogni caricamento impostava il fuso globale di DuckDB, l'ultimo vinceva; ora solo se ci sono timestamp con fuso | `duck.rs` | risolto (fe873dda) |
| R12 | DuckDB: con colonne TIMETZ la query girava tre volte (effetti collaterali ripetuti); ora una sola | `lib.rs` del driver | risolto (ef55982b) |
| R13 | Export ClickHouse: tuple a un campo, numeri grandi e ordine delle chiavi dei Map | `text.rs` | risolto (3f800ed0) |
| R14 | Import ClickHouse senza diritto CREATE: ora ricade sul percorso diretto, dicendo che non è atomico | `import.rs` | risolto (026c8960) |
| R15 | Import ClickHouse: il controllo dell'intervallo guardava solo la data e non gli anni oltre 9999 o i server vecchi | `import.rs` | risolto (026c8960) |
| R16 | Compare SQL Server: indici XML e SPATIAL producevano un CREATE non valido; ora si saltano con un avviso | `catalog.rs` | risolto (975d979f) |
| R17 | MCP in sola lettura su ClickHouse: `IS DISTINCT FROM` era letto come posizione di tabella e rifiutava una query valida | `access.rs` | risolto (5b2f9f67) |
| R18 | Un errore di accesso senza le parole attese (libSQL "Unauthorized", HTTP 401 dei plugin) non richiedeva la password | `App.tsx` | risolto (ffce7c35) |
| R19 | Modifica di massa su SQLite: svuotare una cella DATE/JSON/BOOLEAN scriveva NULL anche dove non è ammesso | `massEdit.ts` | risolto (8b3a0ac3) |
| R20 | Explain: l'indice suggerito su un'espressione perdeva i letterali e gli schemi delle funzioni | `indexSql.ts` | risolto (8c985b85) |

### Revisione di sicurezza (SR1-SR17)

La revisione di sicurezza del codice (credenziali, MCP, sola lettura, SQL generato, file letti e scritti) ha trovato diciassette problemi; tutti corretti, con test o scenari nei commit. Le correzioni stanno nel ramo `s-security-fixes` (unito in `5e9c7b13`); l'isolamento dei dati del keychain tra gli scenari E2E in `84c53e72`, `7f79f32e`, `a6313416`.

| # | Problema | Stato |
|---|---|---|
| SR1 | Le connessioni in sola lettura erano imposte solo dall'analisi dell'istruzione: `SELECT purge_all()` o `dblink_exec` giravano. Ora la sessione è in sola lettura anche sul server | risolto (ffe5e5cb) |
| SR2 | Gli agenti con scrittura e conferma eseguivano senza chiedere una query con effetti fuori dalla transazione (`dblink_exec`, funzioni di Snowflake, Trino) | risolto (1e8bd53a) |
| SR3 | Le istruzioni di avvio di un progetto condiviso girano per agenti e CLI senza l'approvazione dell'utente | risolto (2175cd91) |
| SR4 | Il ripristino di un `.sql` passava `\!` e `system` a `psql` e `mysql` | risolto (db967a27, 5dcda7e7) |
| SR5 | `git` in una cartella di progetto condivisa eseguiva i comandi del repository (`core.fsmonitor`, `core.pager`, filtri) | risolto (3f29c466) |
| SR6 | `${env:NOME}` si espandeva anche in host, utente e opzioni arrivati da un file condiviso o da un link | risolto (42b63ce6) |
| SR7 | I controlli di sicurezza erano calcolati prima di riempire le variabili `${nome}` del testo | risolto (3a0c19c8) |
| SR8 | Driver ODBC e stringa di connessione viaggiavano nei file condivisi (caricamento di librerie native, `FILEDSN` verso un percorso di rete) | risolto (42b63ce6) |
| SR9 | Una nuova connessione condivisa poteva dare agli agenti l'accesso in scrittura | risolto (d09758d7) |
| SR10 | Un database chiamato `--init-command=…` era letto come opzione da `mysql`, `mysqldump` e `sqlite3` | risolto (db967a27) |
| SR11 | La porta di debug dei test (`PLATYPUS_DEVTOOLS_PORT`) è accettata nelle versioni di rilascio: ora solo su loopback e documentata come rischio accettato | risolto (bc247f31) |
| SR12 | `PLATYPUS_DRIVER_REGISTRY` accettava qualsiasi URL, anche `http://` | risolto (1a854da6) |
| SR13 | Argon2id con i valori predefiniti e nessuna lunghezza minima della password di vault e sincronizzazione | risolto (90e03da3) |
| SR14 | Il test di una connessione usava la password salvata anche con un server diverso da quello salvato; stessa cosa per i segreti SSH | risolto (97128df1, 32e0605f) |
| SR15 | L'export CSV scriveva come formule le celle che iniziano con `=`, `+`, `-`, `@` | risolto (7096fbef) |
| SR16 | I file del token e del ponte degli agenti restavano con i permessi vecchi dopo una riscrittura | risolto (292490bd) |
| SR17 | Un client MCP con accesso ai file poteva allargare da sé la politica degli agenti (`agent_access`, `confirm_writes`, `unfiltered`) | risolto (3d376866, 87062689) |
