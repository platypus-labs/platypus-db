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

Nella prima colonna delle tabelle:
- ✅: fatto.
- 🆕: aggiunto il 25 ottobre 2026, ancora da fare.

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
| Export CSV/JSON/NDJSON/Parquet in streaming | `platypus-transfer` | H |
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
scaduto. `✅ completato` vuol dire che P1, P2 e P3 sono tutte fatte; `✅ fatto (…)` elenca le
priorità finite, e il resto del gruppo si può ancora prendere. Procedura completa:
[CLAUDE.md](../CLAUDE.md).

| Gruppo | Lock |
|---|---|
| A — Modifica dei dati | ✅ fatto (P1; P2 e P3 in parte) |
| B — Griglia avanzata | ✅ fatto (P1; P2 e P3 in parte) |
| C — Navigator e oggetti | ✅ completato |
| D — Editor SQL | ✅ fatto (P1, P3; P2 in parte) |
| E — Explain e performance | ✅ fatto (P1; P2 e P3 in parte) |
| F — Connettività e credenziali | ✅ fatto (P1, P2) |
| G — Nuovi driver | 🔒 Delfa · group-g-adbc-odbc · 2026-10-25 |
| H — Import, export e trasferimento | ✅ fatto (P1; P2 e P3 in parte) |
| I — Strumenti di schema | ✅ fatto (P1) |
| J — Amministrazione del server | libero |
| K — Shell ed ergonomia | ✅ fatto (P1) |
| L — Sicurezza operativa | ✅ fatto (P1) |
| M — Analisi dei dati | libero |
| N — Accesso per agenti (MCP) | ✅ fatto (P1) |
| O — Collaborazione e sync | libero |
| P — Piattaforma, qualità e release | libero |
| Q — Documentazione e catalogo dei dati | libero |
| Progetti (spazi di lavoro) | ✅ fatto (parte B) |

### Gruppo A — Modifica dei dati

**Stato: P1 completate, P2 e P3 in parte** (modifica, righe nuove/duplicate/eliminate, anteprima SQL,
chiave obbligatoria o virtuale, NULL/default ed editor per tipo, salvataggio tutto-o-niente,
incolla da foglio di calcolo, risultati di query di una tabella, dictionary delle FK, script
di undo; P3: array e tipi composti PostgreSQL modificati un elemento o campo per riga, gli
array annidati come testo). Da fare: modifica di massa della selezione, BLOB caricati da file.

**Area:** `apps/desktop/src/grid/edit/`, `commands/data.rs`, `platypus-sql/src/dml.rs`
**Dipende da:** 0.3, 0.4, 0.5

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Modifica delle celle (doppio clic o F2), celle modificate evidenziate, Salva (Ctrl+S) e Annulla | P1 | D |
| ✅ Aggiunta, duplicazione ed eliminazione di righe | P1 | D |
| ✅ Anteprima dell'SQL generato (UPDATE/INSERT/DELETE) prima del salvataggio | P1 | D |
| ✅ Modifica consentita solo con PK o chiave unica; altrimenti sola lettura, con il motivo indicato | P1 | D |
| ✅ Impostare a NULL o al valore di default, editor per tipo (data, booleano, enum, JSON) | P1 | D |
| ✅ Salvataggio in transazione: tutto o niente, rispettando la modalità manuale del tab | P1 | D |
| ✅ **Chiavi virtuali**: chiave logica definita dall'utente per le tabelle senza PK | P2 | D |
| ✅ Incolla da Excel/CSV come nuove righe | P2 | D |
| ✅ Modifica di risultati di query da una sola tabella (con alias e JOIN esclusi) | P2 | D |
| ✅ **Dictionary data**: le FK mostrano la colonna descrittiva della tabella collegata, con un menu a tendina in modifica | P2 | D |
| ✅ Array PostgreSQL e tipi composti modificabili | P3 | D |
| ✅ Script di undo generato per le modifiche salvate | P2 | N |
| 🆕 **Modifica di massa**: un valore su tutte le celle selezionate, riempi verso il basso, sostituisci testo nella selezione, sequenza numerica | P2 | D |
| 🆕 BLOB e testi lunghi caricati da file nella cella | P3 | D |

**Done:** modifica, inserimento ed eliminazione su PG, MySQL, MariaDB e SQLite, con test di
conformance per la DML generata (quoting, NULL, binari, timestamp con fuso).

---

### Gruppo B — Griglia avanzata (sola lettura)

**Stato: P1 completate, P2 e P3 in parte** (copia avanzata, ricerca Ctrl+F, pannello valori F7, filtri
rapidi, vista record, layout delle colonne salvato per tabella, aggregati della selezione,
navigazione delle FK e pannello References, pannello Grouping, formati di visualizzazione,
pannelli Metadata e Calc, confronto di due righe, refresh automatico fuori dalla cronologia,
viewer GIS su piano cartesiano senza mappa di sfondo). Da fare: formattazione condizionale,
colonne virtuali calcolate.

**Area:** `apps/desktop/src/grid/features/`, `apps/desktop/src/panels/`
**Dipende da:** 0.4

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Copia avanzata: con intestazioni, come INSERT, CSV, Markdown, JSON, HTML, IN-list | P1 | D |
| ✅ Ricerca nella griglia (Ctrl+F) con evidenziazione | P1 | D |
| ✅ **Pannello valori**: testo lungo, JSON e XML formattati, hex, immagini dai BLOB, salva su file | P1 | D |
| ✅ Vista record: una riga in verticale, con navigazione avanti e indietro | P2 | D |
| ✅ Colonne fisse, nascoste e riordinate, larghezza salvata per tabella | P2 | D |
| ✅ Aggregati della selezione nella status bar (count, sum, avg, min, max) | P2 | D |
| ✅ Filtri rapidi dal menu contestuale (`= valore`, `IS NULL`, `IN selezione`) | P1 | D |
| ✅ Navigazione delle FK: apre la riga referenziata; pannello **References** con le righe che puntano a quella corrente | P2 | D |
| ✅ Pannello **Metadata**: tipo, nullabilità e tabella di origine della colonna | P3 | D |
| ✅ Pannello **Grouping**: conteggi per valore delle colonne selezionate | P2 | D |
| ✅ Pannello **Calc**: calcoli sulle celle selezionate | P3 | D |
| ✅ Formati di visualizzazione: data, numeri, locale, fuso; tempo relativo opzionale | P2 | D |
| ✅ Refresh automatico ogni N secondi | P3 | D |
| ✅ Viewer GIS: geometrie PostGIS e MySQL su mappa | P3 | D |
| ✅ Confronto di due righe selezionate | P3 | N |
| 🆕 **Formattazione condizionale**: colore di cella o riga da regole (valore, intervallo, NULL, scala di colore sui numeri), salvata per tabella | P2 | D |
| 🆕 Colonne virtuali calcolate: espressione sulle colonne della riga, solo in visualizzazione | P3 | D |

---

### Gruppo C — Navigator e oggetti del database

**Area:** `apps/desktop/src/components/Navigator.tsx`, `apps/desktop/src/objects/` (tab `object`), `meta.rs` e `ddl.rs` dei driver, `platypus-core/src/metadata.rs`, `platypus-sql/src/generate.rs`
**Dipende da:** 0.2, 0.3, 0.6

**Stato: completato** (P1, P2 e P3). "MERGE" è un UPSERT per dialetto (`ON CONFLICT` in PostgreSQL e SQLite, `ON DUPLICATE KEY` in MySQL). Righe stimate e dimensioni nel navigator (PostgreSQL e MySQL; dimensione degli indici solo PostgreSQL; SQLite non ha statistiche). La ricerca nei metadati è la capacità opzionale `Connection::search_metadata`, nel tab `search` con la ricerca nei dati (una tabella alla volta, `Dialect::any_column_contains`). Filtri e segnalibri stanno nel local storage della web view, la cache dei metadati in IndexedDB (scade dopo 30 giorni). Partizioni sotto la tabella padre solo in PostgreSQL: in MySQL non sono tabelle a sé.

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Nell'albero anche funzioni, procedure, trigger, sequenze, tipi, estensioni ed eventi | P1 | D |
| ✅ Tab oggetto con sezioni Proprietà, Colonne, Indici, Vincoli, FK, Dipendenze e **DDL** | P1 | D |
| ✅ Menu "Genera SQL": SELECT, INSERT, UPDATE, DELETE, MERGE, DDL | P1 | D |
| ✅ Rinomina, elimina (con cascade e anteprima), svuota tabella, copia nome qualificato | P1 | D |
| ✅ Conteggio righe stimato e dimensione di tabelle e indici | P2 | D |
| ✅ Ricerca nei metadati di tutto il database (oggetti, colonne, sorgenti delle funzioni) | P2 | D |
| ✅ Ricerca full-text nei dati di più tabelle | P3 | D |
| ✅ Segnalibri sugli oggetti | P3 | D |
| ✅ Filtri sul navigator salvati per connessione (es. nascondere gli schemi `tmp_*`) | P2 | D |
| ✅ Vista semplice e vista avanzata del navigator | P3 | D |
| ✅ Più database per connessione (PG: elenco dei database, cambio senza nuova connessione) | P1 | D |
| ✅ Partizioni mostrate sotto la tabella padre | P3 | D |
| ✅ Cache dei metadati su disco: navigazione anche offline e avvio più rapido | P2 | N |

---

### Gruppo D — Editor SQL

**Area:** `apps/desktop/src/editor/`, `platypus-sql` (formatter, parser), `apps/desktop/src/scripts/`
**Dipende da:** 0.2, 0.5

**Stato: P1 e P3 completate, P2 in parte** (da fare: esecuzione su più connessioni). Il formatter lavora sui token (`platypus-sql/src/format.rs`): cambia solo spazi e maiuscole delle parole chiave, e lascia com'è il corpo di funzioni, procedure e trigger. Le opzioni sono il case delle parole chiave e l'indentazione. I comandi dell'editor sono azioni dello scope `editor`. L'autocomplete dei JOIN usa le FK del catalogo (`Connection::catalog_foreign_keys`). Parametri (`:id`, `?`) e variabili (`${nome}`) passano dal dialog dei valori e dal pannello Variabili, sostituiti prima dell'esecuzione; i comandi client (`@set`, `@echo`, `@include`, `@export`) sono righe che iniziano con `@` all'inizio di uno statement e non arrivano al database. Gli script salvati sono file `.sql` in `scripts/<progetto>` nella cartella dati, con i link alle connessioni in `.links.json`. Il linter usa i fatti di `platypus_sql::lint` e il catalogo dell'autocomplete; l'esecuzione con `psql`, `mysql` o `sqlite3` usa le impostazioni della sessione e il suo tunnel SSH, e Cancel lo interrompe.

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Formattatore SQL (Ctrl+Shift+F) per dialetto, configurabile | P1 | D |
| ✅ Duplica riga, sposta riga, multi-cursore, selezione del blocco statement | P1 | D |
| ✅ **Parametri e variabili** (`:id`, `?`, `${var}`) con un dialog dei valori e un **pannello variabili** | P2 | D |
| ✅ **Comandi client-side**: `@set`, `@include file.sql`, `@export`, `@echo` | P2 | D |
| ✅ Template e snippet con Tab, modificabili | P2 | D |
| ✅ Script salvati in un pannello progetti: cartelle, link a una connessione, rinomina. Gli script vanno nei progetti del gruppo K (`apps/desktop/src/projects/`) | P2 | D |
| ✅ **Modalità terminale SQL**: output testuale invece della griglia | P3 | D |
| ✅ **Query builder visuale**: tabelle trascinate, JOIN sulle FK, SQL generato | P3 | D+ |
| ✅ Autocomplete dei JOIN dalle FK (`JOIN orders o ON` completa la condizione) | P1 | N |
| ✅ Autocomplete di funzioni, con firma e documentazione | P2 | D |
| ✅ Esecuzione dello script con `psql` o `mysql` locali | P3 | D |
| ✅ Folding, outline degli statement, vai alla definizione di un oggetto (Ctrl+clic) | P2 | D |
| ✅ Esecuzione dello statement in un nuovo tab di risultati (Ctrl+\\) | P2 | D |
| ✅ Controllo ortografico di commenti e stringhe | P3 | D |
| ✅ Linter SQL: colonna inesistente, `SELECT *` in produzione, JOIN senza condizione | P3 | N |
| 🆕 **Esecuzione su più connessioni**: lo stesso script su N connessioni scelte (es. un database per cliente), risultati uniti con una colonna della connessione ed esito per connessione | P2 | D |

---

### Gruppo E — Explain e performance delle query

**Area:** `apps/desktop/src/explain/` (tab `explain`), `explain` in ogni driver, `commands/explain.rs`
**Dipende da:** 0.3, 0.6

**Stato: P1 completate, P2 e P3 in parte** (da fare: confronto di due piani, piani incollati). Ogni driver legge il proprio piano in un albero di `PlanNode` (`platypus-core/src/explain.rs`): PostgreSQL `EXPLAIN (FORMAT JSON)`, MySQL `FORMAT=TREE` ed `EXPLAIN ANALYZE` (il JSON di ANALYZE arriva solo con la 8.3), MariaDB `EXPLAIN`/`ANALYZE FORMAT=JSON`, SQLite `EXPLAIN QUERY PLAN` (senza costi né ANALYZE). Con ANALYZE lo statement gira in una transazione annullata (un savepoint dentro una transazione aperta), sulla sessione propria del tab `explain`. Il tab ha le viste albero, testo e tabella; si apre con `Ctrl+Shift+E` o con Explain analyze dal menu dell'editor, che applica i controlli di una normale esecuzione. `Explain::new` calcola per ogni passo costo e tempo propri, senza i figli (`self_cost`, `self_ms`): la heatmap colora i passi per la loro quota e segnala le stime di righe sbagliate di 10 volte o più. Il query manager (`Ctrl+Shift+M`) legge un log in memoria (`apps/desktop/src-tauri/src/querylog.rs`, le ultime 2000 richieste): ogni sessione ed export avvolge la connessione in `Logged`, che registra statement, richieste di metadati e comandi di transazione con durata, righe ed esito. P3: i driver segnano le scansioni complete con filtro (`PlanNode::full_scan`) e `Explain::new` suggerisce un indice per quelle che leggono almeno 10.000 righe (su PostgreSQL solo con ANALYZE: il piano semplice stima solo le righe tenute); ogni voce del log ha il tempo fino alla prima risposta e il round trip dell'ultimo keep-alive, che non entra più nel log; la soglia delle query lente è l'impostazione `performance.slow_query_seconds`.

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Explain plan visuale ad albero con costi, righe stimate e reali (PG `EXPLAIN (FORMAT JSON, ANALYZE)`, MySQL `FORMAT=JSON`) | P1 | D+ |
| ✅ Explain in forma testuale e tabellare | P1 | D |
| ✅ Heatmap dei nodi più costosi e righe stimate contro righe reali | P2 | N |
| ✅ Suggerimenti di indice (seq scan su tabelle grandi con filtro) | P3 | N |
| ✅ **Query manager**: log di tutte le query eseguite dall'app (anche metadati), con durata ed esito | P2 | D |
| ✅ **Query trace**: tempi di rete, server e fetch per ogni esecuzione | P3 | D+ |
| ✅ Avviso per query lente, oltre una soglia configurabile | P3 | N |
| 🆕 **Confronto di due piani** affiancati (prima e dopo un indice o una riscrittura), con le differenze di costo e tempo per nodo | P2 | N |
| 🆕 Piano incollato da testo o JSON (es. da un log del server o da un collega), aperto nel tab `explain` senza connessione | P3 | N |

---

### Gruppo F — Connettività e sicurezza delle credenziali — ✅ P1 fatti, P2 in parte

**Stato: P1 completati, P2 in parte** (da fare: connessione da URL incollato, variabili
d'ambiente nei campi) (tunnel SSH con password, chiave, agent e jump host; tipi
Dev/Test/Prod con colore e commit manuale in Prod; import da DBeaver, DataGrip, pgAdmin e
`pg_service.conf`, con cartelle e progetti da un export `.dbp` di DBeaver; cartelle di
connessioni nei progetti e cartelle di progetti nella barra; profili di rete; schema, fuso orario e query di bootstrap dopo ogni
connessione; password da `.pgpass`, da comando shell o chiesta ogni volta; master password
con file cifrato; certificati client; latenza, keep-alive e riconnessione). La conferma
prima di scrivere in Prod è nel gruppo L. Restano i P2 nuovi e i P3.

**Area:** nuovo crate `platypus-net`, `platypus-store`, `apps/desktop/src/connections/` (`ConnectionDialog.tsx`)
**Dipende da:** 0.7

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ **Tunnel SSH** (`russh`): password, chiave, agent, jump host | P1 | D |
| ✅ **Tipi di connessione** Dev/Test/Prod: colore, conferma prima di scrivere, autocommit off di default in Prod | P1 | D |
| Proxy SOCKS5 e HTTP | P3 | D |
| ✅ **Profili di rete** riutilizzabili tra connessioni | P2 | D |
| ✅ Inizializzazione: schema e database di default, query di bootstrap, fuso orario | P2 | D |
| Comandi shell su eventi (prima della connessione, dopo la disconnessione), ad esempio un VPN | P3 | D |
| ✅ Autenticazione: `.pgpass`, password da comando shell, chiedi ogni volta | P2 | D |
| Kerberos, SSPI, Entra ID, IAM AWS e GCP | P3 | D+ |
| ✅ **Master password** per le credenziali salvate (oltre al keychain) | P2 | D |
| ✅ Gestione certificati SSL: CA personalizzate, certificato client | P2 | D |
| ✅ Import delle connessioni da DBeaver (`data-sources.json`), DataGrip, pgAdmin, `~/.pg_service.conf` | P1 | D |
| ✅ Indicatore di salute della connessione (latenza, keep-alive, riconnessione) | P2 | N |
| ✅ Import di progetti DBeaver (`.dbp`) con cartelle; cartelle di connessioni nei progetti e cartelle di progetti nella barra | P2 | D |
| Kubernetes port-forward, AWS SSM | P3 | D+ |
| 🆕 Connessione da URL incollato (`postgres://…`, `mysql://…`, `Server=…;Database=…`): i campi si compilano da soli | P2 | D |
| 🆕 Variabili d'ambiente e file `.env` nei campi della connessione (`${env:DB_PASSWORD}`) | P2 | D |
| 🆕 Credenziali da gestori esterni: 1Password CLI, Bitwarden, HashiCorp Vault, AWS Secrets Manager | P3 | D+ |
| 🆕 Test di tutte le connessioni di un progetto in una volta, con l'esito per ognuna | P3 | N |

---

### Gruppo G — Nuovi driver

**Area:** un crate `platypus-driver-*` per driver e i relativi test di conformance. È il gruppo più
parallelizzabile: **una persona per driver**.
**Dipende da:** 0.5, 0.6

**Stato: P1 completate, P2 e P3 in parte** (P2: ClickHouse e i server con protocollo PostgreSQL;
P3: libSQL/Turso e MongoDB; ADBC con Snowflake, BigQuery, Databricks e Trino, driver scaricati al primo uso. Da fare: ODBC (Oracle, Db2), Redis). SQL Server usa `tiberius` copiato
in `vendor/` con le patch per la negoziazione della cifratura; il dialetto Transact-SQL scrive `TOP (n)` e `[nomi]`,
e la transazione segue `@@TRANCOUNT`. DuckDB è incorporato come SQLite, con risultati Arrow nativi;
senza l'estensione ICU non ha fuso orario di sessione. Una nuova connessione parte dalla scelta del
database in una griglia di loghi. Conformance su PostgreSQL, MySQL, MariaDB, SQL Server, SQLite e
DuckDB (SQL Server dal profilo `sqlserver` di `docker/compose.yml`).
ClickHouse (P2) via HTTP (`platypus-driver-clickhouse`, `reqwest` + TLS condiviso
in `platypus-tls`): una sessione HTTP per connessione (`SET`, `USE`, tabelle temporanee),
risultati `ArrowStream` decodificati man mano, cancel con `KILL QUERY` per `query_id`. Le query
sono descritte prima (`DESCRIBE`) per tipizzare `DateTime` e leggere come testo i tipi che
Arrow non ha (UUID, JSON, enum, IP). Niente transazioni né chiavi esterne: niente modifica dei
dati in griglia. Nuovo tipo di oggetto `dictionary`. Servizio `clickhouse` (porta 58123) tra
quelli di default in `docker/compose.yml`, conformance compresa.

CockroachDB, YugabyteDB, TimescaleDB, Supabase e Neon (P2) passano dal driver PostgreSQL, che
riconosce CockroachDB e YugabyteDB da `version()`; nel picker sono profili con porta, utente,
database o SSL di default. CockroachDB non ha dimensioni delle tabelle né explain. Conformance su
CockroachDB, YugabyteDB e TimescaleDB dal profilo `pgwire` di `docker/compose.yml`. YugabyteDB fa
fallire con `40001` il DDL che ne incrocia un altro dentro una transazione: i test di conformance
girano uno alla volta.

libSQL/Turso (P3): crate `platypus-driver-libsql`, protocollo Hrana 3 su HTTP (`/v3/pipeline`) con
`reqwest` e la configurazione rustls di `platypus-tls`, invece del crate `libsql` (molte più
dipendenze). Il token va nella password o nel parametro `authToken` degli URL `libsql://`. Le
istruzioni girano su uno stream del server tenuto aperto dal suo baton (transazioni e stato di
sessione); metadati, explain e tipi dei risultati riusano il codice del driver SQLite. Limiti: niente
annullamento (HTTP non ferma un'istruzione), niente tabelle temporanee su `sqld`, risultati letti
interi. Conformance dal profilo `libsql` (`sqld` con autenticazione JWT). Cloudflare D1: valutato,
non implementato: la sua API HTTP (`/accounts/<id>/d1/database/<id>/query`) chiede un account e un
token Cloudflare, non ha un server locale per i test (wrangler simula D1 solo nei Worker) e non ha
transazioni tra richieste.

MongoDB (`platypus-driver-mongo`, driver ufficiale con rustls): l'editor scrive mongosh, che un
parser proprio traduce in comandi; i risultati sono una griglia dei campi di primo livello con
l'albero del documento selezionato accanto. Niente transazioni, explain né modifica dei dati;
conformance dedicata in `tests/mongo.rs` (profilo `mongo` di `docker/compose.yml`).

| Driver | Prio | Note |
|---|---|---|
| ✅ SQL Server (`tiberius`) | P1 | Dialetto `TOP`, `[quoting]`, più result set |
| ✅ DuckDB | P1 | Arrow nativo; serve anche al gruppo M |
| ✅ ClickHouse | P2 | HTTP con formato Arrow |
| ✅ File come database: CSV, Parquet, JSON, XLSX (tramite DuckDB) | P2 | Fatto con il gruppo H: viste DuckDB sul file, un foglio XLSX per tabella (calamine); si aprono col trascinamento sulla finestra |
| ✅ CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon; AlloyDB no | P2 | Wire PG: profili e test, non driver nuovi |
| ADBC e ODBC generici (Snowflake, Oracle, Db2, Databricks, BigQuery, Trino) | P2 | ✅ ADBC (Snowflake, BigQuery, Databricks, Trino, ADBC generico; driver scaricati dal registro dbc). Da fare: ODBC (Oracle, Db2, ODBC generico) |
| Aggiornamento automatico dei driver di tutti i database (scaricati ADBC/ODBC e nativi), con verifica che i dialetti restino compatibili; scelta di versioni precedenti di un driver | P3 | Scaricati: registro con più versioni. Nativi: compilati nell'app, si aggiornano con l'app; versioni vecchie = più versioni dello stesso crate o driver caricabili a parte. Test di compatibilità per versione |
| Oracle nativo | P3 | Licensing OCI da valutare |
| Redis, ✅ **MongoDB** | P3 | MongoDB: sintassi mongosh, griglia dei campi + albero del documento; Redis da fare |
| ✅ libSQL/Turso; Cloudflare D1 valutato, non fatto | P3 | Varianti di SQLite |
| 🆕 Elasticsearch/OpenSearch, Cassandra/ScyllaDB | P3 | Query DSL e CQL; stesso schema di MongoDB (griglia + albero) |

---

### Gruppo H — Import, export e trasferimento dati

**Area:** `platypus-transfer`, `apps/desktop/src/transfer/`, `commands/transfer.rs`
**Dipende da:** 0.5

**Stato: P1 completate, P2 e P3 in parte** (da fare: caricamento bulk nativo, profili di
trasferimento salvati).

**P1 completate.** I file (CSV, TSV, JSON, NDJSON, Parquet, XLSX) si leggono con una
connessione DuckDB sul file, la stessa del "file come database" del gruppo G; l'import in una
tabella esistente o nuova (`platypus-transfer::import`) scrive INSERT da 500 righe in una sola
transazione, e una tabella creata e poi fallita si elimina. I tipi della tabella nuova vengono da
`Dialect::column_type`. Conformance su tutti i database ed E2E `import-sqlite`.

**P2 completate.** L'import aggiunge, sostituisce (`DELETE` nella stessa transazione, non
`TRUNCATE`, che MySQL committa) o fa l'upsert sulla chiave scelta (`Dialect::upsert_rows`:
`ON CONFLICT`, `MERGE`). In MySQL, dove `ON DUPLICATE KEY` scatta su qualunque indice unico,
le righe passano da una tabella temporanea: si aggiornano quelle con la chiave, si inseriscono
le altre, e un doppione su un altro indice unico dà errore. Il trasferimento tra database riusa
l'import con una connessione qualsiasi come sorgente; le colonne testuali prendono il tipo
dichiarato. Export in XLSX (`rust_xlsxwriter`, a memoria costante; i decimali con più di 15
cifre restano testo esatto), SQL INSERT, Markdown, HTML e XML; di più tabelle o di uno schema
in una cartella. "Incolla CSV" salva gli appunti in un file temporaneo, cancellato alla
chiusura dell'import. Conformance: modalità su tutti i database e trasferimento tra tutte le 36
coppie; E2E `transfer-sqlite` (il selettore di cartella e gli appunti, di sistema, li risponde
la pagina).

**P3 completate.** La migrazione (`platypus-transfer::migrate`) crea ogni tabella con colonne,
NOT NULL, chiave primaria e identity (una chiave intera da sola: `GENERATED BY DEFAULT AS
IDENTITY`, `AUTO_INCREMENT`, `IDENTITY` con `IDENTITY_INSERT`, il rowid di SQLite, una
sequenza in DuckDB; la numerazione riparte dopo le righe copiate), la riempie in una
transazione sua, poi aggiunge default (numeri, stringhe, booleani, ora corrente), indici
secondari e vincoli unici, e alla fine chiavi esterne e CHECK. SQLite e DuckDB dichiarano chiavi
esterne e CHECK con la tabella: le tabelle si creano in ordine di dipendenza, e i CHECK passano
solo da un database dello stesso tipo. Viste, funzioni, procedure e trigger si copiano dal loro
DDL tra database dello stesso tipo (con lo schema riscritto e senza `DEFINER` in MySQL). Quello
che non passa finisce tra gli avvisi. Il driver SQLite legge i CHECK dal SQL della tabella.
Il generatore (`fake`) sceglie per nome e tipo della colonna, dà chiavi dopo la più grande,
chiavi esterne (anche composte, da una sola riga) prese dalla tabella referenziata e valori
distinti alle colonne UNIQUE. L'export si comprime in gzip o zstd e si carica con un `PUT` su
un URL firmato o su un bucket S3, GCS (chiavi HMAC), R2 o MinIO con le sue chiavi, firmato con
Signature V4 (`platypus-transfer::s3`); la chiave segreta non si salva. Conformance: migrazione
tra le 30 coppie di database diversi, generatore su tutti; l'upload sul bucket si prova con
`PLATYPUS_TEST_S3` contro un server S3 locale (es. `rustfs/rustfs`). Limiti noti: gli indici parziali
perdono il `WHERE`; un indice su TEXT in MySQL copre i primi 255 caratteri e in SQL Server la
colonna diventa `NVARCHAR(450)`; le chiavi esterne puntano alla tabella con lo stesso nome nello
schema copiato; nelle viste si riscrive ogni nome uguale allo schema seguito da un punto.

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Import CSV, TSV, JSON, NDJSON, Parquet e XLSX in una tabella, con mappatura e anteprima | P1 | D |
| ✅ Import con creazione della tabella dai tipi dedotti | P1 | D |
| ✅ Import e sostituzione (truncate e load), upsert sulla chiave | P2 | D |
| ✅ Export XLSX, SQL INSERT, Markdown, HTML, XML | P2 | D |
| ✅ Export di più tabelle o di un intero schema in una volta | P2 | D |
| ✅ **Trasferimento tra database** (es. da PG a MySQL) con mappatura dei tipi, in streaming | P2 | D |
| ✅ Migrazione di schema e dati tra database diversi | P3 | D+ |
| ✅ Generatore di dati finti per colonna (nomi, email, date, FK coerenti) | P3 | D+ |
| ✅ Export compresso (gzip, zstd) e su storage cloud (S3, GCS) | P3 | D+ |
| ✅ Incolla CSV dagli appunti per creare subito una tabella | P2 | N |
| 🆕 **Caricamento bulk nativo** per import e trasferimento: `COPY` in PostgreSQL, `LOAD DATA LOCAL` in MySQL, bulk copy in SQL Server, appender in DuckDB; INSERT a blocchi come ripiego | P2 | D |
| 🆕 Profili di import, export e trasferimento salvati e riutilizzabili (anche dal task scheduler del gruppo J) | P3 | D |

---

### Gruppo I — Strumenti di schema

**Stato: P1 completata** (editor tabella grafico). Il crate `platypus-schema` legge il design di
una tabella (`load`: dettagli del driver più le azioni delle FK, gli `auto_increment` e i default
letterali di MySQL, l'`AUTOINCREMENT` di SQLite) e genera lo script da un design all'altro
(`script`): le parti cambiate si eliminano e si ricreano, le colonne si rinominano e si
modificano sul posto (`CHANGE COLUMN` in MySQL, `sp_rename` e il vincolo di default per nome
in SQL Server). SQLite e DuckDB, che oltre a rinominare e aggiungere colonne non modificano
quasi nulla, ricreano la tabella copiando le righe (SQLite con `foreign_keys` spento; i suoi
trigger vanno ricreati a mano). `apply` esegue tutto in una transazione dove il DDL si annulla
(non MySQL). Il tab `table-editor` mostra l'anteprima dello script prima di salvare. Limiti: i
commenti di colonna non si scrivono in SQL Server (proprietà estese) né in SQLite; le colonne
generate e gli indici su espressioni restano come sono solo se non si toccano.

**Area:** `apps/desktop/src/erd/` (tab `erd`), `apps/desktop/src/table-editor/`, nuovo crate `platypus-schema` (modello di schema, diff, DDL)
**Dipende da:** 0.3, 0.5; per il DDL completo serve il gruppo C

| Funzionalità | Prio | Origine |
|---|---|---|
| Diagramma ER in sola lettura dalle FK, layout automatico, export PNG/SVG | P2 | D |
| Diagrammi personalizzati: si scelgono le tabelle, con note | P3 | D |
| ERD in modifica: si disegna lo schema e si genera il DDL | P3 | D+ |
| ✅ **Editor tabella grafico**: crea e modifica colonne, indici, vincoli e FK, con anteprima dell'ALTER | P1 | D |
| Creazione di viste, trigger, funzioni e sequenze da form | P3 | D |
| **Confronto schemi** con script di migrazione | P3 | D+ |
| **Confronto dati** tra due tabelle, con script di sincronizzazione | P3 | D+ |
| Changelog Liquibase/Flyway generati dal confronto | P3 | D+ |
| Timeline delle modifiche di schema (snapshot periodici e diff) | P3 | N |
| 🆕 **Linter dello schema**: tabelle senza PK, FK senza indice, indici duplicati o contenuti in altri, tipi diversi tra FK e colonna referenziata | P2 | N |
| 🆕 Stato delle migrazioni di un progetto (Flyway, Liquibase, Prisma, Alembic, Rails): applicate, pendenti, divergenti | P3 | N |

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
| 🆕 **Query più costose del server**: `pg_stat_statements`, `performance_schema`, Query Store di SQL Server, con apertura nell'editor e nel tab `explain` | P2 | N |
| 🆕 Statistiche di tabelle e indici: indici mai usati, bloat, tabelle con molte scansioni complete, ultimo VACUUM/ANALYZE | P2 | N |
| 🆕 Parametri del server (`pg_settings`, `SHOW VARIABLES`) con ricerca e differenze dai valori di default | P3 | N |

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
| 🆕 Notifica di sistema alla fine di una query lunga, quando la finestra non è in primo piano | P2 | D |
| 🆕 Zoom dell'interfaccia e dimensione del font di editor e griglia (Ctrl+= / Ctrl+-) | P2 | D |
| 🆕 Tab staccato in una finestra separata (secondo monitor) | P3 | D |

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
| 🆕 Sola lettura temporanea per un tab o una sessione, senza cambiare la connessione salvata | P2 | N |
| 🆕 **Snapshot prima di UPDATE/DELETE manuali**: le righe toccate salvate in locale prima dello statement, con ripristino in un clic | P3 | N |
| 🆕 Blocco dell'app dopo N minuti di inattività (con la master password), disconnessione delle sessioni Prod | P3 | N |

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
| 🆕 Profilo di una tabella intera: per colonna distribuzione, distinti, pattern (email, date, numeri come testo), outlier | P3 | N |

---

### Gruppo N — Accesso per agenti esterni (MCP)

**Stato: P1 completate.** Il crate `platypus-mcp` ha la libreria degli strumenti (`Agent`) e
il binario `platypus-mcp`, server MCP su stdio con `rmcp`: legge il profilo dell'app (o
`PLATYPUS_DATA_DIR`) e apre le connessioni come l'app (profili di rete, tunnel SSH, password
da keychain, comando o `.pgpass`); con la master password i segreti restano bloccati e la
connessione risponde con un errore chiaro. Strumenti: `list_connections` (senza host, utente
né segreti), `list_schemas`, `list_objects`, `describe_table`, `get_ddl`, `search_objects` e
`run_query` (uno statement, limiti di righe, byte e tempo, tabella testuale o JSON).
`agent_access` per connessione in `connections.toml` (versione 4): `off`, `read_only`
(default), `read_write`; Prod e connessioni read-only al massimo in sola lettura. In sola
lettura passa solo ciò che `platypus_sql::analyze` vede come query, eseguito nella
transazione di sola lettura del database (`Dialect::read_only_begin`: `READ ONLY` in
PostgreSQL, MySQL e MariaDB, `query_only` in SQLite, rollback in DuckDB e SQL Server); in
lettura e scrittura niente controllo delle transazioni e niente UPDATE/DELETE senza WHERE o
TRUNCATE se l'impostazione Safety chiede conferma. Errori ripuliti dai segreti; ogni query
nella cronologia con l'agente come origine (colonna `origin`) e un filtro nel pannello. Pagina
Impostazioni "Agents" con i comandi per Claude Code e Codex e l'accesso di ogni connessione;
selettore "Agent access" nel dialog di connessione. CLI `platypus --connection <nome>` con
output JSON documentato nel README. Gli installer includono `platypus-mcp` come external
binary di Tauri (`--config src-tauri/tauri.mcp.conf.json`). Restano P2 e P3.

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
| ✅ **Server MCP via stdio** (`platypus-mcp`): l'agente lo avvia da sé, senza che l'app sia aperta, e usa le connessioni salvate | P1 | D+ |
| ✅ Tool di lettura: elenco connessioni, schemi, oggetti, dettagli di una tabella (colonne, chiavi, FK, indici), DDL, ricerca di oggetti per nome | P1 | D+ |
| ✅ Tool `run_query`: esegue SQL con limite di righe, di byte e timeout; risultato compatto (tabella testuale o JSON) adatto al contesto di un LLM | P1 | D+ |
| ✅ **Permessi per connessione**: disattivata per gli agenti, sola lettura (default), lettura e scrittura. Le connessioni Prod sono sempre in sola lettura per gli agenti | P1 | N |
| ✅ Sola lettura garantita: controllo dello statement (`is_read_only`) e, dove il database lo permette, transazione `READ ONLY` | P1 | N |
| ✅ Segreti mai esposti: nessun tool restituisce password, URL completi o chiavi SSH | P1 | N |
| ✅ Registro delle query degli agenti nella cronologia, con l'agente come origine e filtro dedicato | P1 | N |
| ✅ Installazione guidata: pagina "Agenti" nelle Impostazioni con i comandi pronti (`claude mcp add platypus -- platypus-mcp`, voce `[mcp_servers.platypus]` per Codex) e un pulsante Copia | P1 | N |
| ✅ CLI per agenti senza MCP: `platypus-cli --connection <nome>` dalle connessioni salvate, output JSON stabile e documentato | P1 | N |
| `explain` e campione di righe (`sample_rows`) come tool dedicati, con i limiti di `run_query` | P2 | D+ |
| **Ponte con l'app aperta** (MCP su HTTP locale con token): l'agente vede le sessioni aperte e l'indicatore "agente collegato" compare nella barra | P2 | N |
| Conferma nell'app per le scritture: dialog con lo statement e le righe stimate (riusa le guardie del gruppo L), con "consenti per questa sessione" | P2 | N |
| Tool `propose_sql`: apre la query proposta dall'agente in un tab dell'editor, da rivedere ed eseguire a mano | P2 | N |
| Contesto dell'utente su consenso: testo dell'editor, selezione, risultato corrente della griglia | P3 | N |
| Schema come risorse MCP (`platypus://<connessione>/<schema>/<tabella>`) per i client che le supportano | P3 | D+ |
| Plugin pronti: plugin per Claude Code e configurazione per Codex con le istruzioni d'uso (quando usare quale tool, limiti) | P3 | N |
| 🆕 Tool di sola lettura su script salvati e cronologia del progetto, con le colonne sensibili del gruppo Q escluse o mascherate | P3 | N |

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
| ✅ Prima release su GitHub (v0.1.0, poi v0.2.0) | P1 |
| Test reale dell'auto-update da una release pubblicata alla successiva | P1 |
| Firma del codice: Windows (certificato o Azure Trusted Signing), macOS con notarizzazione | P2 |
| Benchmark affiancato con DBeaver, aggiornando `BENCHMARKS.md` | P2 |
| Benchmark in CI con soglie (avvio, memoria, decode) per bloccare le regressioni | P2 |
| Crash report opt-in e log su file con rotazione | P2 |
| **Un solo eseguibile anche per gli agenti**: l'app fa da server MCP con un'opzione (`platypus-db --mcp`) invece di un secondo binario `platypus-mcp` che ricontiene tutti i driver (circa 50-60 MB in meno nell'installer) | P2 |
| **DuckDB scaricato al primo uso** invece che incluso nell'installer (circa 35 MB): chi non apre file CSV, Parquet o DuckDB non lo scarica mai | P2 |
| Build di sviluppo più leggere: meno informazioni di debug per le dipendenze in `Cargo.toml` e pulizia periodica della cartella `target` | P3 |
| Distribuzione: winget, Homebrew cask, Flatpak, AUR | P3 |
| Sito e documentazione utente | P3 |
| 🆕 Test di scala: schemi con 10.000 tabelle, tabelle da milioni di righe, celle da decine di MB (navigator, autocomplete, griglia, export) | P2 |
| 🆕 Pacchetto di diagnostica per le segnalazioni: log, versione, impostazioni senza segreti, in un file zip | P3 |

---

### Gruppo Q — Documentazione e catalogo dei dati

Sapere cosa contiene un database senza chiederlo a chi l'ha scritto: note e tag sugli oggetti,
commenti del database, documentazione esportabile, colonne sensibili segnate una volta e
rispettate da tutta l'app.

**Area:** `apps/desktop/src/catalog/`, `commands/catalog.rs`; note e tag nello store del progetto
(`platypus-store`, con una migrazione del formato)
**Dipende da:** il tab oggetto del gruppo C, 0.5, 0.6

| Funzionalità | Prio | Origine |
|---|---|---|
| 🆕 **Note e tag locali** su tabelle e colonne (non scritti nel database), mostrati nel navigator, nell'intestazione della griglia e nell'autocomplete | P2 | N |
| 🆕 Modifica dei commenti del database (`COMMENT ON`, `COMMENT` di MySQL, proprietà estese di SQL Server) dal tab oggetto | P2 | D |
| 🆕 **Documentazione dello schema** esportata in HTML o Markdown: tabelle, colonne, chiavi, commenti, note e diagramma ER | P2 | N |
| 🆕 **Colonne sensibili** (PII, segreti): regole su nome e tipo più scelte a mano, usate dal mascheramento del gruppo L, dagli agenti del gruppo N e dagli export | P3 | N |
| 🆕 Dove si usa una colonna: viste, funzioni, trigger e script salvati che la nominano | P3 | N |

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
| `platypus-store` (connessioni, segreti, cronologia) | F, N, Q | API pubblica dello store; il formato dei file cambia solo con una migrazione |
| Tab oggetto (`apps/desktop/src/objects/`) | C, Q | Sezioni del tab registrabili, aggiunte con un commit separato |

---

## 4. Ondate consigliate

Con 4–6 persone. Ogni gruppo può partire quando la sua dipendenza è pronta, anche prima della fine
dell'ondata.

| Ondata | Gruppi | Obiettivo | Release | Stato |
|---|---|---|---|---|
| **0** | Fase 0 (1 persona) e **P** in parallelo | Codice pronto al lavoro parallelo, prima release pubblica | v0.1.x | Fase 0 fatta; P in corso |
| **1** | **A**, **B** (P1), **C** (P1), **F** (SSH, tipi, import), **K** (P1), **L** (P1) | Sostituire DBeaver nell'uso quotidiano | v0.2 | **Fatta** (P1 di A, B, C, F, K e L; P2 di A e B), release v0.2.0 del 24 ottobre 2026 |
| **2** | **D**, **E**, **G** (SQL Server, DuckDB), **H** (import) | Produttività e più database | v0.3 | D: P1 fatte; E: completato; H: completato |
| **3** | **I**, **J**, **M**, **N**, **Q**, i P2 rimasti | Strumenti avanzati e differenzianti | v0.4 | Da fare |
| **4** | **O**, i P3 | Collaborazione ed ecosistema | v0.5+ | Da fare |

Per ogni gruppo, la Definition of Done segue quella dell'MVP:
- test di conformance o E2E per ciò che è stato aggiunto;
- clippy e fmt puliti;
- nessuna regressione sui target di [BENCHMARKS.md](BENCHMARKS.md);
- codice rivisto per semplicità e performance: refactor fatti, non rimandati;
- funzionalità documentata nel README o nella documentazione utente.
