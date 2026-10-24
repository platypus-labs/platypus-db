# PlatypusDB — Roadmap post-MVP

Pianificazione delle funzionalità dopo la v0.1.0, divisa in **gruppi indipendenti** che più persone
possono portare avanti in parallelo. Ogni gruppo ha un'area di codice propria, così i conflitti di
merge restano rari.

Fonti: la documentazione e il confronto edizioni di DBeaver (Community, Lite, Enterprise, Ultimate),
il backlog v0.2 di [MVP_PLAN.md](MVP_PLAN.md) e le idee che DBeaver non ha.

**Priorità**
- **P1**: serve per usare PlatypusDB al posto di DBeaver ogni giorno.
- **P2**: importante, ma si può rimandare.
- **P3**: nicchia, oppure da valutare.

Nella colonna **Origine**:
- **D**: presente in DBeaver Community.
- **D+**: solo nelle edizioni a pagamento di DBeaver.
- **N**: non presente in DBeaver.

---

## 0. Già presente nella v0.1.0

Questi punti non vanno ripianificati: al massimo si migliorano dentro il gruppo indicato.

| Funzionalità | Dove | Gruppo che la estende |
|---|---|---|
| Doppio clic su una tabella del navigator apre il data viewer | `Navigator.tsx` | C |
| Ordinamento lato server cliccando l'intestazione, barra filtro `WHERE` | `QueryTab.tsx` | B |
| Copia della selezione in TSV | `DataGrid.tsx` | B |
| Più result set per script, Fetch all, cancel | `QueryTab.tsx`, `jobs.rs` | — |
| Colore, cartella e flag read-only per connessione | `platypus-store` | F |
| Cerca/sostituisci, commento con Ctrl+/, Alt+X, errore evidenziato alla posizione | `SqlEditor.tsx` | D |
| Autocomplete di keyword, tabelle e `alias.colonna` | `SqlEditor.tsx`, `platypus-sql` | D |
| Storico ricercabile, file `.sql`, sessione dei tab ripristinata | `HistoryPanel.tsx`, `workspace.ts` | — |
| Autocommit, commit/rollback, avviso alla chiusura con una transazione aperta | `commands.rs` | L |
| Export CSV/JSON/NDJSON/Parquet in streaming | `platypus-export` | H |
| Tema chiaro/scuro, auto-update | `theme.ts`, `UpdateBanner.tsx` | — |

---

## 1. Fase 0 — Fondamenta per il lavoro in parallelo (1 persona, circa 1 settimana)

**Stato: completata.** Come usare i punti di estensione: [EXTENDING.md](EXTENDING.md).

Alcuni file facevano da collo di bottiglia: [commands.rs](../apps/desktop/src-tauri/src/commands/mod.rs),
[QueryTab.tsx](../apps/desktop/src/components/QueryTab.tsx),
[DataGrid.tsx](../apps/desktop/src/grid/DataGrid.tsx),
[Navigator.tsx](../apps/desktop/src/components/Navigator.tsx) e [App.tsx](../apps/desktop/src/App.tsx).
Se più gruppi ci lavorano insieme, i conflitti sono continui. Prima di aprire i gruppi:

| # | Intervento | Perché |
|---|---|---|
| 0.1 | Dividere `commands.rs` in `commands/{connections,query,meta,transactions,export,files}.rs`. Ogni gruppo aggiunge il proprio modulo | Elimina i conflitti sul file più modificato |
| 0.2 | **Registro delle azioni** (`actions.ts`): id, etichetta, scorciatoia, contesto (navigator, griglia, editor, tab) e handler. Menu contestuali, scorciatoie e command palette leggono da qui | I gruppi registrano azioni senza toccare `App.tsx` né `Navigator.tsx` |
| 0.3 | **Tipi di tab**: `TabModel` diventa un'unione discriminata (`query`, `table`, `object`, `erd`, `explain`, `admin`, …) con un registro `kind → componente` | Ogni gruppo aggiunge il proprio tipo di tab senza toccare `QueryTab.tsx` |
| 0.4 | **Estensioni di `DataGrid`**: API per selezione, cell renderer, overlay di modifica e colonne extra | I gruppi A, B e M lavorano sulla griglia senza toccarne il cuore |
| 0.5 | **`Dialect`** in `platypus-sql`: quoting, `LIMIT`/`TOP`, letterali, generazione di DML e DDL, sintassi `EXPLAIN` | Base comune per i gruppi A, C, D, E, G, H e I |
| 0.6 | **Capacità opzionali del driver**: nuovi metodi del trait `Connection` con default `Error::Unsupported` (`explain`, `object_ddl`, `sessions`, …) e `fn capabilities()` | I driver si aggiornano in modo indipendente e la UI nasconde ciò che non è supportato |
| 0.7 | **Settings store** tipizzato (Rust e TS), con una pagina Impostazioni a sezioni registrabili | Molti gruppi hanno preferenze proprie |
| 0.8 | Test E2E in CI: portare gli script di scenario (CDP su WebView2) in `apps/desktop/e2e/` | Ogni gruppo aggiunge scenari propri; le regressioni UI emergono subito |

**Regole per tutti i gruppi**
- Un gruppo modifica solo i file della propria **area**. Se gli serve qualcosa da un file
  condiviso, lo fa in un commit piccolo e separato (estensione di un'API, non logica di feature).
- Le funzionalità di database passano dal `Dialect` o dalle capacità del driver, mai da
  `if driver === "postgres"` sparsi nella UI.
- Ogni gruppo aggiunge test alla conformance suite per ciò che tocca nei driver.
- **Refactor continuo, qualità e performance prima di tutto.** Chi tocca un'area la lascia
  migliore di come l'ha trovata: rimuove duplicazioni, semplifica, rinomina ciò che è poco
  chiaro e sceglie la soluzione più efficiente (niente allocazioni, copie, query o render
  superflui; nessun lavoro per riga dove basta un lavoro per blocco). Un refactor che rende il
  codice più leggibile o più veloce è sempre benvenuto, anche dentro un branch di feature, purché
  resti nell'area del gruppo e non cambi il comportamento senza test.

---

## 2. Gruppi di lavoro

### Lock dei gruppi

Chi lavora su un gruppo lo segna qui e pusha su `main` prima di scrivere codice. Formato:
`🔒 <nome> · <branch> · <AAAA-MM-GG>`. Un lock senza commit sul branch da più di 3 giorni è
scaduto. Procedura completa: [CLAUDE.md](../CLAUDE.md).

| Gruppo | Lock |
|---|---|
| A — Modifica dei dati | 🔒 Delfa · group-a-arrays · 2026-10-24 |
| B — Griglia avanzata | ✅ fatto (P1, P2) |
| C — Navigator e oggetti | 🔒 Paolo · group-c-navigator · 2026-10-24 |
| D — Editor SQL | ✅ fatto (P1) |
| E — Explain e performance | libero |
| F — Connettività e credenziali | ✅ fatto (P1) |
| G — Nuovi driver | libero |
| H — Import, export e trasferimento | libero |
| I — Strumenti di schema | libero |
| J — Amministrazione del server | libero |
| K — Shell ed ergonomia | ✅ fatto (P1) |
| L — Sicurezza operativa | ✅ fatto (P1) |
| M — Analisi dei dati | libero |
| N — Accesso per agenti (MCP) | libero |
| O — Collaborazione e sync | libero |
| P — Piattaforma, qualità e release | libero |
| Progetti (spazi di lavoro) | ✅ fatto (parte B) |

### Gruppo A — Modifica dei dati

**Stato: P1 e P2 completati** (modifica, righe nuove/duplicate/eliminate, anteprima SQL,
chiave obbligatoria o virtuale, NULL/default ed editor per tipo, salvataggio tutto-o-niente,
incolla da foglio di calcolo, risultati di query di una tabella, dictionary delle FK, script
di undo). Resta il P3.

**Area:** `apps/desktop/src/grid/edit/`, `commands/data.rs`, `platypus-sql/src/dml.rs`
**Dipende da:** 0.3, 0.4, 0.5

| Funzionalità | Prio | Origine |
|---|---|---|
| Modifica delle celle (doppio clic o F2), celle modificate evidenziate, Salva (Ctrl+S) e Annulla | P1 | D |
| Aggiunta, duplicazione ed eliminazione di righe | P1 | D |
| Anteprima dell'SQL generato (UPDATE/INSERT/DELETE) prima del salvataggio | P1 | D |
| Modifica consentita solo con PK o chiave unica; altrimenti sola lettura, con il motivo indicato | P1 | D |
| Impostare a NULL o al valore di default, editor per tipo (data, booleano, enum, JSON) | P1 | D |
| Salvataggio in transazione: tutto o niente, rispettando la modalità manuale del tab | P1 | D |
| **Chiavi virtuali**: chiave logica definita dall'utente per le tabelle senza PK | P2 | D |
| Incolla da Excel/CSV come nuove righe | P2 | D |
| Modifica di risultati di query da una sola tabella (con alias e JOIN esclusi) | P2 | D |
| **Dictionary data**: le FK mostrano la colonna descrittiva della tabella collegata, con un menu a tendina in modifica | P2 | D |
| Array PostgreSQL e tipi composti modificabili | P3 | D |
| Script di undo generato per le modifiche salvate | P2 | N |

**Done:** modifica, inserimento ed eliminazione su PG, MySQL, MariaDB e SQLite, con test di
conformance per la DML generata (quoting, NULL, binari, timestamp con fuso).

---

### Gruppo B — Griglia avanzata (sola lettura)

**Stato: P1 e P2 completati** (copia avanzata, ricerca Ctrl+F, pannello valori F7, filtri
rapidi, vista record, layout delle colonne salvato per tabella, aggregati della selezione,
navigazione delle FK e pannello References, pannello Grouping, formati di visualizzazione).
Restano i P3.

**Area:** `apps/desktop/src/grid/features/`, `apps/desktop/src/panels/`
**Dipende da:** 0.4

| Funzionalità | Prio | Origine |
|---|---|---|
| Copia avanzata: con intestazioni, come INSERT, CSV, Markdown, JSON, HTML, IN-list | P1 | D |
| Ricerca nella griglia (Ctrl+F) con evidenziazione | P1 | D |
| **Pannello valori**: testo lungo, JSON e XML formattati, hex, immagini dai BLOB, salva su file | P1 | D |
| Vista record: una riga in verticale, con navigazione avanti e indietro | P2 | D |
| Colonne fisse, nascoste e riordinate, larghezza salvata per tabella | P2 | D |
| Aggregati della selezione nella status bar (count, sum, avg, min, max) | P2 | D |
| Filtri rapidi dal menu contestuale (`= valore`, `IS NULL`, `IN selezione`) | P1 | D |
| Navigazione delle FK: apre la riga referenziata; pannello **References** con le righe che puntano a quella corrente | P2 | D |
| Pannello **Metadata**: tipo, nullabilità e tabella di origine della colonna | P3 | D |
| Pannello **Grouping**: conteggi per valore delle colonne selezionate | P2 | D |
| Pannello **Calc**: calcoli sulle celle selezionate | P3 | D |
| Formati di visualizzazione: data, numeri, locale, fuso; tempo relativo opzionale | P2 | D |
| Refresh automatico ogni N secondi | P3 | D |
| Viewer GIS: geometrie PostGIS e MySQL su mappa | P3 | D |
| Confronto di due righe selezionate | P3 | N |

---

### Gruppo C — Navigator e oggetti del database

**Area:** `apps/desktop/src/components/Navigator.tsx`, `apps/desktop/src/objects/` (tab `object`), `meta.rs` e `ddl.rs` dei driver, `platypus-core/src/metadata.rs`, `platypus-sql/src/generate.rs`
**Dipende da:** 0.2, 0.3, 0.6

**Stato: P1, P2 e P3 completati.** "MERGE" è un UPSERT per dialetto (`ON CONFLICT` in PostgreSQL e SQLite, `ON DUPLICATE KEY` in MySQL). Righe stimate e dimensioni nel navigator (PostgreSQL e MySQL; dimensione degli indici solo PostgreSQL; SQLite non ha statistiche). La ricerca nei metadati è la capacità opzionale `Connection::search_metadata`, nel tab `search` con la ricerca nei dati (una tabella alla volta, `Dialect::any_column_contains`). Filtri e segnalibri stanno nel local storage della web view, la cache dei metadati in IndexedDB (scade dopo 30 giorni). Partizioni sotto la tabella padre solo in PostgreSQL: in MySQL non sono tabelle a sé.

| Funzionalità | Prio | Origine |
|---|---|---|
| Nell'albero anche funzioni, procedure, trigger, sequenze, tipi, estensioni ed eventi | P1 | D |
| Tab oggetto con sezioni Proprietà, Colonne, Indici, Vincoli, FK, Dipendenze e **DDL** | P1 | D |
| Menu "Genera SQL": SELECT, INSERT, UPDATE, DELETE, MERGE, DDL | P1 | D |
| Rinomina, elimina (con cascade e anteprima), svuota tabella, copia nome qualificato | P1 | D |
| Conteggio righe stimato e dimensione di tabelle e indici | P2 | D |
| Ricerca nei metadati di tutto il database (oggetti, colonne, sorgenti delle funzioni) | P2 | D |
| Ricerca full-text nei dati di più tabelle | P3 | D |
| Segnalibri sugli oggetti | P3 | D |
| Filtri sul navigator salvati per connessione (es. nascondere gli schemi `tmp_*`) | P2 | D |
| Vista semplice e vista avanzata del navigator | P3 | D |
| Più database per connessione (PG: elenco dei database, cambio senza nuova connessione) | P1 | D |
| Partizioni mostrate sotto la tabella padre | P3 | D |
| Cache dei metadati su disco: navigazione anche offline e avvio più rapido | P2 | N |

---

### Gruppo D — Editor SQL

**Area:** `apps/desktop/src/editor/`, `platypus-sql` (formatter, parser), `apps/desktop/src/scripts/`
**Dipende da:** 0.2, 0.5

**Stato:** le P1 sono fatte. Il formatter lavora sui token (`platypus-sql/src/format.rs`): cambia solo spazi e maiuscole delle parole chiave, e lascia com'è il corpo di funzioni, procedure e trigger. Le opzioni sono il case delle parole chiave e l'indentazione. I comandi dell'editor sono azioni dello scope `editor`. L'autocomplete dei JOIN usa le FK del catalogo (`Connection::catalog_foreign_keys`). Le P2/P3 sono da fare.

| Funzionalità | Prio | Origine |
|---|---|---|
| Formattatore SQL (Ctrl+Shift+F) per dialetto, configurabile | P1 | D |
| Duplica riga, sposta riga, multi-cursore, selezione del blocco statement | P1 | D |
| **Parametri e variabili** (`:id`, `?`, `${var}`) con un dialog dei valori e un **pannello variabili** | P2 | D |
| **Comandi client-side**: `@set`, `@include file.sql`, `@export`, `@echo` | P2 | D |
| Template e snippet con Tab, modificabili | P2 | D |
| Script salvati in un pannello progetti: cartelle, link a una connessione, rinomina. Gli script vanno nei progetti del gruppo K (`apps/desktop/src/projects/`) | P2 | D |
| **Modalità terminale SQL**: output testuale invece della griglia | P3 | D |
| **Query builder visuale**: tabelle trascinate, JOIN sulle FK, SQL generato | P3 | D+ |
| Autocomplete dei JOIN dalle FK (`JOIN orders o ON` completa la condizione) | P1 | N |
| Autocomplete di funzioni, con firma e documentazione | P2 | D |
| Esecuzione dello script con `psql` o `mysql` locali | P3 | D |
| Folding, outline degli statement, vai alla definizione di un oggetto (Ctrl+clic) | P2 | D |
| Esecuzione dello statement in un nuovo tab di risultati (Ctrl+\\) | P2 | D |
| Controllo ortografico di commenti e stringhe | P3 | D |
| Linter SQL: colonna inesistente, `SELECT *` in produzione, JOIN senza condizione | P3 | N |

---

### Gruppo E — Explain e performance delle query

**Area:** `apps/desktop/src/explain/` (tab `explain`), `explain` in ogni driver, `commands/explain.rs`
**Dipende da:** 0.3, 0.6

| Funzionalità | Prio | Origine |
|---|---|---|
| Explain plan visuale ad albero con costi, righe stimate e reali (PG `EXPLAIN (FORMAT JSON, ANALYZE)`, MySQL `FORMAT=JSON`) | P1 | D+ |
| Explain in forma testuale e tabellare | P1 | D |
| Heatmap dei nodi più costosi e righe stimate contro righe reali | P2 | N |
| Suggerimenti di indice (seq scan su tabelle grandi con filtro) | P3 | N |
| **Query manager**: log di tutte le query eseguite dall'app (anche metadati), con durata ed esito | P2 | D |
| **Query trace**: tempi di rete, server e fetch per ogni esecuzione | P3 | D+ |
| Avviso per query lente, oltre una soglia configurabile | P3 | N |

---

### Gruppo F — Connettività e sicurezza delle credenziali — ✅ P1 fatti

**Stato: P1 completati** (tunnel SSH con password, chiave, agent e jump host; tipi
Dev/Test/Prod con colore e commit manuale in Prod; import da DBeaver, DataGrip, pgAdmin e
`pg_service.conf`). La conferma prima di scrivere in Prod è nel gruppo L. Restano i P2/P3.

**Area:** nuovo crate `platypus-net`, `platypus-store`, `apps/desktop/src/connections/` (`ConnectionDialog.tsx`)
**Dipende da:** 0.7

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ **Tunnel SSH** (`russh`): password, chiave, agent, jump host | P1 | D |
| ✅ **Tipi di connessione** Dev/Test/Prod: colore, conferma prima di scrivere, autocommit off di default in Prod | P1 | D |
| Proxy SOCKS5 e HTTP | P3 | D |
| **Profili di rete** riutilizzabili tra connessioni | P2 | D |
| Inizializzazione: schema e database di default, query di bootstrap, fuso orario | P2 | D |
| Comandi shell su eventi (prima della connessione, dopo la disconnessione), ad esempio un VPN | P3 | D |
| Autenticazione: `.pgpass`, password da comando shell, chiedi ogni volta | P2 | D |
| Kerberos, SSPI, Entra ID, IAM AWS e GCP | P3 | D+ |
| **Master password** per le credenziali salvate (oltre al keychain) | P2 | D |
| Gestione certificati SSL: CA personalizzate, certificato client | P2 | D |
| ✅ Import delle connessioni da DBeaver (`data-sources.json`), DataGrip, pgAdmin, `~/.pg_service.conf` | P1 | D |
| Indicatore di salute della connessione (latenza, keep-alive, riconnessione) | P2 | N |
| Kubernetes port-forward, AWS SSM | P3 | D+ |

---

### Gruppo G — Nuovi driver

**Area:** un crate `platypus-driver-*` per driver e i relativi test di conformance. È il gruppo più
parallelizzabile: **una persona per driver**.
**Dipende da:** 0.5, 0.6

| Driver | Prio | Note |
|---|---|---|
| SQL Server (`tiberius`) | P1 | Dialetto `TOP`, `[quoting]`, più result set |
| DuckDB | P1 | Arrow nativo; serve anche al gruppo M |
| ClickHouse | P2 | HTTP con formato Arrow |
| File come database: CSV, Parquet, JSON, XLSX (tramite DuckDB) | P2 | Si aprono con il trascinamento sulla finestra |
| CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB | P2 | Wire PG: profili e test, non driver nuovi |
| ADBC e ODBC generici (Snowflake, Oracle, Db2, Databricks, BigQuery, Trino) | P2 | Una persona per il bridge, poi profili |
| Oracle nativo | P3 | Licensing OCI da valutare |
| Redis, MongoDB | P3 | Richiedono una UI non tabellare (gruppo dedicato in seguito) |
| libSQL/Turso, Cloudflare D1 | P3 | Varianti di SQLite |

---

### Gruppo H — Import, export e trasferimento dati

**Area:** `platypus-export` (da rinominare in `platypus-transfer`), `apps/desktop/src/transfer/`, `commands/transfer.rs`
**Dipende da:** 0.5

| Funzionalità | Prio | Origine |
|---|---|---|
| Import CSV, TSV, JSON, NDJSON, Parquet e XLSX in una tabella, con mappatura e anteprima | P1 | D |
| Import con creazione della tabella dai tipi dedotti | P1 | D |
| Import e sostituzione (truncate e load), upsert sulla chiave | P2 | D |
| Export XLSX, SQL INSERT, Markdown, HTML, XML | P2 | D |
| Export di più tabelle o di un intero schema in una volta | P2 | D |
| **Trasferimento tra database** (es. da PG a MySQL) con mappatura dei tipi, in streaming | P2 | D |
| Migrazione di schema e dati tra database diversi | P3 | D+ |
| Generatore di dati finti per colonna (nomi, email, date, FK coerenti) | P3 | D+ |
| Export compresso (gzip, zstd) e su storage cloud (S3, GCS) | P3 | D+ |
| Incolla CSV dagli appunti per creare subito una tabella | P2 | N |

---

### Gruppo I — Strumenti di schema

**Area:** `apps/desktop/src/erd/` (tab `erd`), `apps/desktop/src/table-editor/`, nuovo crate `platypus-schema` (modello di schema, diff, DDL)
**Dipende da:** 0.3, 0.5; per il DDL completo serve il gruppo C

| Funzionalità | Prio | Origine |
|---|---|---|
| Diagramma ER in sola lettura dalle FK, layout automatico, export PNG/SVG | P2 | D |
| Diagrammi personalizzati: si scelgono le tabelle, con note | P3 | D |
| ERD in modifica: si disegna lo schema e si genera il DDL | P3 | D+ |
| **Editor tabella grafico**: crea e modifica colonne, indici, vincoli e FK, con anteprima dell'ALTER | P1 | D |
| Creazione di viste, trigger, funzioni e sequenze da form | P3 | D |
| **Confronto schemi** con script di migrazione | P3 | D+ |
| **Confronto dati** tra due tabelle, con script di sincronizzazione | P3 | D+ |
| Changelog Liquibase/Flyway generati dal confronto | P3 | D+ |
| Timeline delle modifiche di schema (snapshot periodici e diff) | P3 | N |

---

### Gruppo J — Amministrazione del server

**Area:** `apps/desktop/src/admin/` (tab `admin`), `commands/admin.rs`, query di sistema nei driver
**Dipende da:** 0.3, 0.6

| Funzionalità | Prio | Origine |
|---|---|---|
| **Session manager**: sessioni attive, query in corso, kill e cancel | P2 | D |
| **Lock manager**: blocchi e catene di attesa | P2 | D |
| Backup e restore con `pg_dump`/`pg_restore` e `mysqldump`, con avanzamento | P2 | D |
| Manutenzione: VACUUM, ANALYZE, REINDEX, OPTIMIZE e CHECK TABLE dal menu | P2 | D+ |
| Utenti, ruoli e permessi (GRANT/REVOKE da form) | P3 | D |
| Dashboard del server: connessioni, TPS, cache hit, dimensioni, replica | P3 | D+ |
| **Task scheduler**: script, export e backup pianificati | P3 | D+ |
| Debugger PL/pgSQL | P3 | D+ |

---

### Gruppo K — Shell dell'applicazione ed ergonomia — ✅ P1 fatti

**Stato: P1 completati** (rotella sui tab, command palette con ricerca fuzzy su azioni,
connessioni, tabelle e query recenti, riapertura del tab chiuso, pagina Impostazioni con
sezioni, scorciatoie personalizzabili, barra degli strumenti configurabile). Restano i P2/P3.
Fatta anche la **nuova grafica**: temi Platypus Dark e Light, font Inter e JetBrains Mono
inclusi, icone Lucide, navigator, tab, toolbar e dialog ridisegnati
([spec](superpowers/specs/2026-10-24-grafica-e-progetti-design.md), parte A). Fatti anche i
**progetti come spazi di lavoro** (parte B): barra dei progetti a sinistra, connessioni in
più progetti, tab, cronologia, palette e titolo della finestra per progetto, `Ctrl+1`…
per cambiare progetto, `connections.toml` versione 2.

**Area:** `App.tsx`, `apps/desktop/src/shell/` (tab bar, palette, impostazioni), `styles.css`, `theme.ts`
**Dipende da:** 0.2, 0.3, 0.7

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ **Rotella del mouse sui tab = scroll orizzontale** | P1 | N |
| ✅ **Command palette** (Ctrl+K o Ctrl+Shift+P): azioni, oggetti, connessioni e query recenti, ricerca fuzzy | P1 | N |
| ✅ Riapri l'ultimo tab chiuso (Ctrl+Shift+T) | P1 | N |
| Tab colorati per connessione, raggruppabili e fissabili; menu "chiudi altri" | P2 | N |
| Editor diviso, con due tab affiancati | P2 | N |
| Trascinare una tabella dal navigator nell'editor inserisce il nome o un `SELECT` | P2 | N |
| ✅ Pagina Impostazioni, scorciatoie personalizzabili, barra degli strumenti configurabile | P1 | D |
| Lingua dell'interfaccia: italiano e inglese, con l'infrastruttura i18n | P2 | D |
| Accessibilità: navigazione da tastiera completa, screen reader, contrasto | P2 | D |
| Pannelli trascinabili e layout salvato | P3 | D |
| Modalità portable, con impostazioni accanto all'eseguibile | P3 | N |
| Mini-finestra di query rapida dalla system tray | P3 | N |
| Modalità Vim nell'editor | P3 | N |
| Log degli errori e attività in background visibili, per la diagnosi | P2 | D |

---

### Gruppo L — Sicurezza operativa — ✅ P1 fatti

**Stato: P1 completati** (conferma o blocco di UPDATE/DELETE senza WHERE e TRUNCATE con le
righe stimate, conferma delle modifiche sulle connessioni Prod anche dal data editor, timer
della transazione aperta con avviso). Restano i P2/P3.

**Area:** `platypus-sql/src/analysis.rs` (analisi dello statement prima dell'esecuzione), `apps/desktop/src/guards/`
**Dipende da:** 0.5; il gruppo F per i tipi di connessione

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Blocco o conferma per UPDATE/DELETE senza WHERE, con il numero di righe stimato | P1 | N |
| ✅ Conferma per DDL e DML sulle connessioni Prod, con il riepilogo degli statement | P1 | D |
| ✅ Timer della transazione aperta: avviso dopo N minuti, indicatore sempre visibile | P1 | N |
| **Pending transactions**: elenco degli statement non ancora committati, per tab | P2 | D |
| **Transaction log** della sessione | P3 | D |
| `LIMIT` automatico sulle SELECT senza limite, configurabile | P2 | N |
| Mascheramento dei dati sensibili per screen share (regole su nome e tipo di colonna) | P3 | N |
| Restrizioni di sicurezza per connessione: niente export, niente modifica | P3 | D+ |

---

### Gruppo M — Analisi dei dati

**Area:** `apps/desktop/src/analysis/`, crate `platypus-local` (DuckDB in-process)
**Dipende da:** 0.4; DuckDB del gruppo G

| Funzionalità | Prio | Origine |
|---|---|---|
| Grafici dai risultati (barre, linee, dispersione, torta), salvabili | P2 | D |
| Statistiche della colonna al passaggio del mouse: % NULL, distinti, min/max, mini istogramma | P2 | N |
| **SQL sui risultati**: interrogare il result set corrente con DuckDB | P2 | N |
| Join tra database diversi tramite DuckDB (tabella PG con tabella MySQL) | P3 | D+ |
| Diff tra due result set (stessa query prima e dopo) | P2 | N |
| Snapshot dei risultati salvati e consultabili offline | P3 | N |
| Pivot dei risultati | P3 | N |
| Notebook: celle SQL, markdown e grafici, esportabili | P3 | N |
| Dashboard personalizzate dalle query salvate | P3 | D+ |

---

### Gruppo N — Accesso per agenti esterni (MCP)

PlatypusDB non integra un proprio assistente AI. Diventa invece uno strumento che gli agenti già
in uso (Claude Code, Codex, Cursor, altri client MCP) possono collegare: usano le connessioni
salvate, leggono lo schema ed eseguono query con i permessi decisi dall'utente. Le credenziali
restano nel keychain e non arrivano mai all'agente.

**Area:** nuovo crate `platypus-mcp` (binario, server MCP), `apps/desktop/src-tauri/src/agent/`
(ponte con l'app aperta), `apps/desktop/src/agents/` (impostazioni e conferme)
**Dipende da:** `platypus-store` (connessioni e segreti), `platypus-sql` (controllo di sola
lettura, `Dialect`), 0.6, 0.7; il gruppo L per le conferme sulle scritture

| Funzionalità | Prio | Origine |
|---|---|---|
| **Server MCP via stdio** (`platypus-mcp`): l'agente lo avvia da sé, senza che l'app sia aperta, e usa le connessioni salvate | P1 | D+ |
| Tool di lettura: elenco connessioni, schemi, oggetti, dettagli di una tabella (colonne, chiavi, FK, indici), DDL, ricerca di oggetti per nome | P1 | D+ |
| Tool `run_query`: esegue SQL con limite di righe, di byte e timeout; risultato compatto (tabella testuale o JSON) adatto al contesto di un LLM | P1 | D+ |
| **Permessi per connessione**: disattivata per gli agenti, sola lettura (default), lettura e scrittura. Le connessioni Prod sono sempre in sola lettura per gli agenti | P1 | N |
| Sola lettura garantita: controllo dello statement (`is_read_only`) e, dove il database lo permette, transazione `READ ONLY` | P1 | N |
| Segreti mai esposti: nessun tool restituisce password, URL completi o chiavi SSH | P1 | N |
| Registro delle query degli agenti nella cronologia, con l'agente come origine e filtro dedicato | P1 | N |
| Installazione guidata: pagina "Agenti" nelle Impostazioni con i comandi pronti (`claude mcp add platypus -- platypus-mcp`, voce `[mcp_servers.platypus]` per Codex) e un pulsante Copia | P1 | N |
| CLI per agenti senza MCP: `platypus-cli --connection <nome>` dalle connessioni salvate, output JSON stabile e documentato | P1 | N |
| `explain` e campione di righe (`sample_rows`) come tool dedicati, con i limiti di `run_query` | P2 | D+ |
| **Ponte con l'app aperta** (MCP su HTTP locale con token): l'agente vede le sessioni aperte e l'indicatore "agente collegato" compare nella barra | P2 | N |
| Conferma nell'app per le scritture: dialog con lo statement e le righe stimate (riusa le guardie del gruppo L), con "consenti per questa sessione" | P2 | N |
| Tool `propose_sql`: apre la query proposta dall'agente in un tab dell'editor, da rivedere ed eseguire a mano | P2 | N |
| Contesto dell'utente su consenso: testo dell'editor, selezione, risultato corrente della griglia | P3 | N |
| Schema come risorse MCP (`platypus://<connessione>/<schema>/<tabella>`) per i client che le supportano | P3 | D+ |
| Plugin pronti: plugin per Claude Code e configurazione per Codex con le istruzioni d'uso (quando usare quale tool, limiti) | P3 | N |

---

### Gruppo O — Collaborazione e sincronizzazione

**Area:** nuovo crate `platypus-sync`, `apps/desktop/src/projects/`
**Dipende da:** il pannello script del gruppo D

| Funzionalità | Prio | Origine |
|---|---|---|
| Integrazione Git per gli script: stato, diff, commit e pull | P3 | D+ |
| Sincronizzazione cifrata end-to-end di impostazioni, connessioni senza password e script | P3 | N |
| Progetti condivisi in un team: connessioni e script, senza credenziali | P3 | D+ |
| Plugin in WASM per azioni e driver personalizzati | P3 | N |
| Condivisione di una query come link o file `.platypus` | P3 | N |

---

### Gruppo P — Piattaforma, qualità e release

**Area:** `.github/`, `docs/`, `apps/desktop/e2e/`, benchmark
**Dipende da:** nulla; parte subito

| Attività | Prio |
|---|---|
| Verifica su macOS e Linux: build, avvio, memoria, scroll; correzioni di piattaforma | P1 |
| Prima release su GitHub e test reale dell'auto-update (0.1.0 → 0.1.1) | P1 |
| Firma del codice: Windows (certificato o Azure Trusted Signing), macOS con notarizzazione | P2 |
| Benchmark affiancato con DBeaver, aggiornando `BENCHMARKS.md` | P2 |
| Benchmark in CI con soglie (avvio, memoria, decode) per bloccare le regressioni | P2 |
| Crash report opt-in e log su file con rotazione | P2 |
| Distribuzione: winget, Homebrew cask, Flatpak, AUR | P3 |
| Sito e documentazione utente | P3 |

---

## 3. Mappa dei conflitti

Le "x" indicano i gruppi che toccano la stessa area. Dove c'è conflitto si serve il contratto
della fase 0 (un'API piccola), non la condivisione del file.

| Area condivisa | Gruppi | Contratto |
|---|---|---|
| `DataGrid.tsx` | A, B, M | 0.4: estensioni della griglia |
| Trait `Connection` | C, E, G, J | 0.6: capacità opzionali |
| `platypus-sql` | A, D, H, I, L | 0.5: `Dialect` e moduli separati |
| `App.tsx`, tab | C, E, I, J, K | 0.3: registro dei tipi di tab |
| Menu e scorciatoie | tutti | 0.2: registro delle azioni |
| `ConnectionDialog.tsx` | F, G | Sezioni per driver registrabili |
| `platypus-store` (connessioni, segreti, cronologia) | F, N | API pubblica dello store; il formato dei file cambia solo con una migrazione |

---

## 4. Ondate consigliate

Con 4–6 persone. Ogni gruppo può partire quando la sua dipendenza è pronta, anche prima della fine
dell'ondata.

| Ondata | Gruppi | Obiettivo | Release | Stato |
|---|---|---|---|---|
| **0** | Fase 0 (1 persona) e **P** in parallelo | Codice pronto al lavoro parallelo, prima release pubblica | v0.1.x | Fase 0 fatta; P in corso |
| **1** | **A**, **B** (P1), **C** (P1), **F** (SSH, tipi, import), **K** (P1), **L** (P1) | Sostituire DBeaver nell'uso quotidiano | v0.2 | **Fatta** (P1 di A, B, C, F, K e L; P2 di A e B), release v0.2.0 del 24 ottobre 2026 |
| **2** | **D**, **E**, **G** (SQL Server, DuckDB), **H** (import) | Produttività e più database | v0.3 | D: P1 fatte |
| **3** | **I**, **J**, **M**, **N**, i P2 rimasti | Strumenti avanzati e differenzianti | v0.4 | Da fare |
| **4** | **O**, i P3 | Collaborazione ed ecosistema | v0.5+ | Da fare |

Per ogni gruppo, la Definition of Done segue quella dell'MVP:
- test di conformance o E2E per ciò che è stato aggiunto;
- clippy e fmt puliti;
- nessuna regressione sui target di [BENCHMARKS.md](BENCHMARKS.md);
- codice rivisto per semplicità e performance: refactor fatti, non rimandati;
- funzionalità documentata nel README o nella documentazione utente.
