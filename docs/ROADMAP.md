# PlatypusDB — Roadmap post-MVP

Pianificazione delle funzionalità dopo la v0.1.0, divisa in **gruppi indipendenti** che più persone
possono portare avanti in parallelo. Ogni gruppo ha un'area di codice propria, così i conflitti di
merge restano rari.

Fonti: la documentazione e il confronto edizioni di DBeaver (Community, Lite, Enterprise, Ultimate),
il backlog v0.2 di [MVP_PLAN.md](MVP_PLAN.md), le idee che DBeaver non ha e, dal 28 ottobre 2026,
il confronto con [Tabularis](https://tabularis.dev).

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
- 🆕: aggiunto dal 25 ottobre 2026 in poi, ancora da fare.

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

### Correzioni aperte

Problemi trovati lavorando su altro, da correggere prima di prendere un gruppo (vedi
[CLAUDE.md](../CLAUDE.md)). Chi ne corregge uno toglie la sua riga.

| Problema | Trovato |
|---|---|
| Debugger PL/pgSQL del gruppo J mai provato su un server vero: nel container cloud apt è bloccato e l'immagine `docker/postgres-pldebug` non si costruisce. Avviare `docker compose -f docker/compose.yml --profile pldebug up -d --build --wait`, eseguire `PLATYPUS_TEST_PLDEBUG_URL=… cargo test -p platypus-conformance --test debugger` (i quattro test di debug, oggi saltati) e lo scenario `admin-debugger-postgres`, correggere quello che non va, poi segnare la riga ✅ nel gruppo J | gruppo J, 2026-10-30 |
| Scenari E2E delle P3 dei gruppi I e M scritti in un container Linux e mai eseguiti: `erd-fullscreen-refresh-sqlite`, `erd-edit-sqlite`, `schema-changelog-sqlite`, `schema-objects-sqlite`, `schema-objects-postgres`, `schema-migrations-sqlite`, `schema-data-compare-sqlite`, `schema-history-sqlite`, `analysis-pivot-sqlite`, `analysis-across-sqlite`, `analysis-saved-results-sqlite`, `analysis-dashboard-sqlite`, `analysis-table-profile-sqlite`. Eseguirli su Windows dopo la build di release e correggere scenari o codice | 2026-10-30, chiusura dei gruppi I e M |
| Scenari E2E delle correzioni del 30 ottobre scritti in un container Linux e mai eseguiti: `object-tab-edit-table-sqlite` (Modifica tabella dal tab oggetto e dal data viewer), `generate-ddl-many-pg` (Genera il DDL su 300 tabelle, avanzamento, Stop), `navigator-select-sqlite` (adattato al tab che si riempie), `erd-layout-large-sqlite` (disposizione automatica e linee su 57 tabelle, con screenshot), `profiles-schedule-sqlite` (il task di un profilo prende il primo profilo arrivato), e quelli del gruppo G `cassandra`, `drivers-versions`, `drivers-versions-auto`, `drivers-versions-quiet`, `drivers-versions-recent`, e quelli del gruppo J `admin-parameters-postgres`, `admin-dashboard-postgres`, `admin-users-postgres`, `admin-users-mysql`. Eseguirli su Windows dopo la build di release e correggere scenari o codice | correzioni, 2026-10-30 |
| Scenari E2E del gruppo Q (P3) scritti in un container Linux e mai eseguiti: `sensitive-columns-sqlite` (segni, mascheramento, intestazioni, export), `column-usage-sqlite` (Used by: viste, trigger, script, Refresh). Eseguirli su Windows dopo la build di release e correggere scenari o codice | gruppo Q, 2026-10-30 |

### Lock dei gruppi

Chi lavora su un gruppo lo segna qui e pusha su `main` prima di scrivere codice. Formato:
`🔒 <nome> · <branch> · <AAAA-MM-GG>`. Un lock senza commit sul branch da più di 3 giorni è
scaduto. `✅ completato` vuol dire che P1, P2 e P3 sono tutte fatte; `✅ fatto (…)` elenca le
priorità finite, e il resto del gruppo si può ancora prendere, come un gruppo libero. Non si
prendono solo i gruppi `✅ completato`, `🔒` e `⏸`. Procedura completa:
[CLAUDE.md](../CLAUDE.md).

| Gruppo | Lock |
|---|---|
| A — Modifica dei dati | ✅ completato |
| B — Griglia avanzata | ✅ completato |
| C — Navigator e oggetti | ✅ completato |
| D — Editor SQL | ✅ completato |
| E — Explain e performance | ✅ completato |
| F — Connettività e credenziali | ✅ fatto (P1, P2; P3 in parte) |
| G — Nuovi driver | ✅ completato |
| H — Import, export e trasferimento | ✅ completato |
| I — Strumenti di schema | ✅ completato |
| J — Amministrazione del server | ✅ fatto (P2; P3 in parte) |
| K — Shell ed ergonomia | ✅ completato |
| L — Sicurezza operativa | ✅ completato |
| M — Analisi dei dati | ✅ completato |
| N — Accesso per agenti (MCP) | 🔒 Delfa · group-n-masking · 2026-10-30 |
| O — Collaborazione e sync | 🔒 Delfa · group-o-collab · 2026-10-30 |
| P — Piattaforma, qualità e release | 🔒 Claude · claude/exciting-galileo-ukai8j · 2026-10-30 |
| Q — Documentazione e catalogo dei dati | ✅ completato |
| R — Revisione grafica | ⏸ in attesa: si prende quando tutti gli altri gruppi sono completati |
| S — Collaudo finale | ⏸ in attesa: si prende dopo il gruppo R |
| Progetti (spazi di lavoro) | ✅ fatto (parte B) |

### Gruppo A — Modifica dei dati

**Stato: completato** (modifica, righe nuove/duplicate/eliminate, anteprima SQL,
chiave obbligatoria o virtuale, NULL/default ed editor per tipo, salvataggio tutto-o-niente,
incolla da foglio di calcolo, risultati di query di una tabella, dictionary delle FK, script
di undo, modifica di massa della selezione (valore/NULL/DEFAULT controllato per tipo, riempi
verso il basso con Ctrl+Alt+D (⌃⇧D su macOS), sostituisci testo anche con espressioni regolari, sequenza
numerica); P3: array e tipi composti PostgreSQL modificati un elemento o campo per riga, gli
array annidati come testo, celle caricate da file: byte nelle colonne binarie, testo UTF-8
nelle colonne di testo, conferma sopra i 50 MB, rifiutati sopra i 200 MB).

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
| ✅ **Modifica di massa**: un valore su tutte le celle selezionate, riempi verso il basso, sostituisci testo nella selezione, sequenza numerica | P2 | D |
| ✅ BLOB e testi lunghi caricati da file nella cella | P3 | D |

**Done:** modifica, inserimento ed eliminazione su PG, MySQL, MariaDB e SQLite, con test di
conformance per la DML generata (quoting, NULL, binari, timestamp con fuso).

---

### Gruppo B — Griglia avanzata (sola lettura) — ✅ completato

**Stato: completato** (copia avanzata, ricerca Ctrl+F, pannello valori F7, filtri
rapidi, vista record, layout delle colonne salvato per tabella, aggregati della selezione,
navigazione delle FK e pannello References, pannello Grouping, formati di visualizzazione,
pannelli Metadata e Calc, confronto di due righe, refresh automatico fuori dalla cronologia,
viewer GIS su piano cartesiano senza mappa di sfondo, formattazione condizionale, JSON nelle
colonne di testo). Formattazione condizionale: `grid/formatting.ts` (regole, valutazione,
salvataggio per tabella in `localStorage` come il layout delle colonne) e
`grid/FormattingDialog.tsx`; i colori passano dal `decorate` della griglia, sotto le modifiche
e la ricerca; la scala di colori usa minimo e massimo di `column_profile` sulle righe caricate.
I numeri si leggono col formato della griglia solo nelle colonne numeriche. JSON nel testo: il
campo `text_json` della connessione (`plain` per tenerlo com'è scritto). Colonne calcolate
(P3): `grid/computed.ts` (linguaggio di espressioni simile a SQL, valutato senza `eval`,
salvato per tabella in `localStorage`) e `grid/ComputedDialog.tsx`; restano nel frontend: la
griglia le mostra dopo le colonne del risultato (`resultColumns` di `DataGrid`) e
`GridSelection.cols` le lascia fuori, così le funzioni che leggono il risultato sul backend
(copia come, totali, ricerca, modifica, ordinamento) lavorano solo sulle colonne vere; `Ctrl+C`
le copia da `GridSelection.shown`, coi valori calcolati sulle righe caricate.

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
| ✅ **Formattazione condizionale**: colore di cella o riga da regole (valore, intervallo, NULL, scala di colore sui numeri), salvata per tabella | P2 | D |
| ✅ 🆕 Colonne virtuali calcolate: espressione sulle colonne della riga, solo in visualizzazione | P3 | D |
| ✅ **JSON nelle colonne di testo**: un TEXT o VARCHAR che contiene JSON valido si apre formattato nel pannello valori, con un'opzione per connessione | P3 | D |

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

**Stato: completato.** Esecuzione su più connessioni: `editor/MultiRunDialog.tsx` apre una sessione per connessione col flusso delle schede (`workbench().openSession`, in autocommit anche in produzione), passa i controlli di Sicurezza di ogni connessione (`guards/check`), legge ogni riga dei risultati e copia l'ultimo con righe di ognuna in un DuckDB in memoria (come "Query with SQL"); una scheda nuova li unisce con `UNION ALL BY NAME` e la colonna della connessione (`editor/multiRun.ts`). Il formatter lavora sui token (`platypus-sql/src/format.rs`): cambia solo spazi e maiuscole delle parole chiave, e lascia com'è il corpo di funzioni, procedure e trigger. Le opzioni sono il case delle parole chiave e l'indentazione. I comandi dell'editor sono azioni dello scope `editor`. L'autocomplete dei JOIN usa le FK del catalogo (`Connection::catalog_foreign_keys`). Parametri (`:id`, `?`) e variabili (`${nome}`) passano dal dialog dei valori e dal pannello Variabili, sostituiti prima dell'esecuzione; i comandi client (`@set`, `@echo`, `@include`, `@export`) sono righe che iniziano con `@` all'inizio di uno statement e non arrivano al database. Gli script salvati sono file `.sql` in `scripts/<progetto>` nella cartella dati, con i link alle connessioni in `.links.json`. Il linter usa i fatti di `platypus_sql::lint` e il catalogo dell'autocomplete; l'esecuzione con `psql`, `mysql` o `sqlite3` usa le impostazioni della sessione e il suo tunnel SSH, e Cancel lo interrompe.

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
| ✅ **Esecuzione su più connessioni**: lo stesso script su N connessioni scelte (es. un database per cliente), risultati uniti con una colonna della connessione ed esito per connessione | P2 | D |

---

### Gruppo E — Explain e performance delle query

**Area:** `apps/desktop/src/explain/` (tab `explain`), `explain` in ogni driver, `commands/explain.rs`
**Dipende da:** 0.3, 0.6

**Stato: completato.** Ogni driver legge il proprio piano in un albero di `PlanNode` (`platypus-core/src/explain.rs`): PostgreSQL `EXPLAIN (FORMAT JSON)`, MySQL `FORMAT=TREE` ed `EXPLAIN ANALYZE` (il JSON di ANALYZE arriva solo con la 8.3), MariaDB `EXPLAIN`/`ANALYZE FORMAT=JSON`, SQLite `EXPLAIN QUERY PLAN` (senza costi né ANALYZE). Con ANALYZE lo statement gira in una transazione annullata (un savepoint dentro una transazione aperta), sulla sessione propria del tab `explain`. Il tab ha le viste albero, testo e tabella; si apre con `Ctrl+Shift+E` o con Explain analyze dal menu dell'editor, che applica i controlli di una normale esecuzione. `Explain::new` calcola per ogni passo costo e tempo propri, senza i figli (`self_cost`, `self_ms`): la heatmap colora i passi per la loro quota e segnala le stime di righe sbagliate di 10 volte o più. Il query manager (`Ctrl+Shift+M`) legge un log in memoria (`apps/desktop/src-tauri/src/querylog.rs`, le ultime 2000 richieste): ogni sessione ed export avvolge la connessione in `Logged`, che registra statement, richieste di metadati e comandi di transazione con durata, righe ed esito. P3: i driver segnano le scansioni complete con filtro (`PlanNode::full_scan`) e `Explain::new` suggerisce un indice per quelle che leggono almeno 10.000 righe (su PostgreSQL solo con ANALYZE: il piano semplice stima solo le righe tenute); ogni voce del log ha il tempo fino alla prima risposta e il round trip dell'ultimo keep-alive, che non entra più nel log; la soglia delle query lente è l'impostazione `performance.slow_query_seconds`. Piani letti da un documento: ogni driver ha `read_plan` puro (PostgreSQL JSON e testo di `EXPLAIN` con le cifre lette da `step_figures`, CockroachDB, MySQL tree, MariaDB JSON, SQLite `QUERY PLAN`, SQL Server `SHOWPLAN_TEXT`); `plain_plan` toglie la cornice dei client (intestazione e `+` di psql, tabella e `\n` del client MySQL), e il comando `read_plan` riconosce il formato o usa quello scelto. *Open query plan…* (palette e menu della lista connessioni) apre il piano in un tab `explain` su una connessione temporanea, senza Refresh. *Compare…* allinea i passi con il piano dell'esecuzione precedente o con un altro piano (`explain/compare.ts`: stessi operazione e oggetto in ordine, gli altri affiancati), sul tempo proprio se entrambi i piani sono stati eseguiti, altrimenti sul costo. La vista Grafo (`explain/graph.ts`, SVG) mette le foglie in colonne e ogni passo sopra i figli, con la heatmap e linee spesse quanto le righe; *Adatta* lo riduce fino al 40% e porta in vista la radice.

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Explain plan visuale ad albero con costi, righe stimate e reali (PG `EXPLAIN (FORMAT JSON, ANALYZE)`, MySQL `FORMAT=JSON`) | P1 | D+ |
| ✅ Explain in forma testuale e tabellare | P1 | D |
| ✅ Heatmap dei nodi più costosi e righe stimate contro righe reali | P2 | N |
| ✅ Suggerimenti di indice (seq scan su tabelle grandi con filtro) | P3 | N |
| ✅ **Query manager**: log di tutte le query eseguite dall'app (anche metadati), con durata ed esito | P2 | D |
| ✅ **Query trace**: tempi di rete, server e fetch per ogni esecuzione | P3 | D+ |
| ✅ Avviso per query lente, oltre una soglia configurabile | P3 | N |
| ✅ **Confronto di due piani** affiancati (prima e dopo un indice o una riscrittura), con le differenze di costo e tempo per nodo | P2 | N |
| ✅ Piano incollato da testo o JSON, o aperto da file (es. da un log del server o da un collega), nel tab `explain` senza connessione | P2 | N |
| ✅ **Vista a grafo** del piano: nodi collegati con costo, righe e tempo, la stessa heatmap delle altre viste | P2 | D+ |

---

### Gruppo F — Connettività e sicurezza delle credenziali — ✅ P1 e P2 fatti, P3 in parte

**Stato: P1 e P2 completati; dei P3 fatti proxy, comandi su eventi, gestori di credenziali,
test di tutte le connessioni, token IAM AWS e GCP ed Entra ID, accesso Windows (SSPI) a SQL
Server, port forward Kubernetes e AWS SSM** (resta Kerberos/GSSAPI per PostgreSQL e MySQL, che
i loro driver Rust non hanno). Accesso senza password: opzione `auth` (`platypus_core::Auth`):
`aws_iam`, `gcp_iam` ed `entra_id` sono token che `platypus-net/src/token.rs` chiede a ogni
connessione alla CLI (`aws rds generate-db-auth-token`, `gcloud sql generate-login-token`,
`az account get-access-token`, con la risorsa di SQL Server o quella di PostgreSQL e MySQL di
Azure) e mette come password, mai salvata; MySQL li manda con `mysql_clear_password`, SQL
Server come token Entra ID (`AuthMethod::AADToken`). `windows` è l'accesso integrato di
tiberius (feature `winauth`, solo su Windows). `sign_in` serve anche ai client CLI (`psql`,
`mysql --enable-cleartext-plugin`, `sqlcmd -E`/`-G`). Import e stringhe incollate con login
Windows o Entra ID tengono `auth`. Port forward: opzione `forward` = `kubernetes`
(`kubectl port-forward --address 127.0.0.1 [--context] [--namespace] <k8s_target>
<locale>:<porta del server>`) o `ssm` (`aws ssm start-session` col documento
`AWS-StartPortForwardingSessionToRemoteHost` verso host e porta della connessione, dall'istanza
`ssm_target`); `platypus-net/src/forward.rs` sceglie una porta locale libera, aspetta che la
CLI scriva che ascolta (`Forwarding from`, `Waiting for connections`: provare la porta
aprirebbe una connessione al database; 30 secondi al massimo, l'errore cita stderr) e con la
connessione chiude la CLI e i processi che avvia, come il plugin di SSM (gruppo di processi
su Unix, `taskkill /T` su Windows). Non si combina con tunnel SSH o proxy. Un modo di accesso
che il database non accetta (`Auth::fits`) è un errore. Limite noto: annullare una query
MySQL con un token scaduto non riesce, perché il `KILL QUERY` apre una connessione nuova col
token iniziale. `aws_region` e `aws_profile` valgono per IAM e SSM. La
conformance (`tests/cloud_cli.rs`) mette sul `PATH` un `aws` e un `kubectl` finti contro
PostgreSQL e MySQL.
Proxy: opzione `proxy` della connessione, un URL `socks5://` o `http://` con login facoltativo;
`platypus-net/src/proxy.rs` fa l'handshake SOCKS5 (nome risolto dal proxy, login RFC 1929) o
`CONNECT` HTTP con Basic. Senza tunnel la connessione passa da una porta locale inoltrata
attraverso il proxy (`Tunnel::through_proxy`, provato una volta all'apertura); con il tunnel
passa dal proxy il primo server SSH (`Tunnel::open_via`). `Tunnel::of` sceglie il percorso
per l'app, il server MCP e i client CLI. Comandi su eventi: opzioni `before_connect` e
`after_disconnect` (`platypus-net/src/hooks.rs`), eseguite da `cmd /S /C` o `sh -c` e
contate per coppia di comandi: la prima parte con la prima connessione che la usa, si aspetta
per un minuto al massimo e il suo errore cita stderr; la seconda parte quando si chiude
l'ultima (o subito, se nessuna si è aperta). I loro `${VAR}` restano alla shell. Il driver non
vede né il proxy né i comandi. Gestori di credenziali: `credentialManagers.ts` scrive il comando della CLI dal
riferimento del segreto (1Password `op read`, Bitwarden `bw get password`, Vault
`vault kv get -field=…`, AWS `secretsmanager get-secret-value`) e lo salva come comando della
password, riconosciuto alla riapertura; un comando che stampa JSON dà il campo `password`
(`platypus-store/src/auth.rs`). Test delle connessioni: `TestConnectionsDialog.tsx`, quattro
alla volta. Variabili d'ambiente: `platypus-core/src/env.rs`
sostituisce `${env:NOME}` (`$${` per scriverlo) in host, database, utente, password,
opzioni, certificati e tunnel SSH, cercando prima nell'ambiente del processo e poi nel file
`.env` dell'opzione `env_file`; si applica alla connessione (app, server MCP, CLI), mentre lo
store e *Copia come URL* tengono il riferimento; un host `${env:…}` si controlla al salvataggio
con un segnaposto e di nuovo dopo la sostituzione. Una password si legge dall'ambiente solo
se è tutta un riferimento (`${env:PW}`): altrimenti resta com'è scritta. Il file `.env` va
indicato con un percorso assoluto o che inizia con `~`. Modalità pooler: opzione `pooler` = `transaction` (o `?pgbouncer=true`
nell'URL); in autocommit il driver PostgreSQL descrive lo statement in una transazione sua
(`BEGIN`/`ROLLBACK`) e lo esegue senza nome (`query_typed_raw`, `execute_typed`), e i nomi
dei tipi si leggono con `query_typed`; in una transazione aperta resta tutto com'era.
`vendor/tokio-postgres` (patch in `[patch.crates-io]`) svuota con la cache dei tipi anche gli
statement con nome che li cercano, che il pooler perde. Il
dialogo propone la modalità per il pooler di Supabase sulla 6543 e per gli host `-pooler` di
Neon. `docker/compose.yml` ha PgBouncer (profilo `pooler`, porta 56432) in transaction mode
con `DISCARD ALL` dopo ogni transazione. Prima: (tunnel SSH con password, chiave, agent e jump host; tipi
Dev/Test/Prod con colore e commit manuale in Prod; import da DBeaver, DataGrip, pgAdmin e
`pg_service.conf`, con cartelle e progetti da un export `.dbp` di DBeaver; cartelle di
connessioni nei progetti e cartelle di progetti nella barra; profili di rete; schema, fuso orario e query di bootstrap dopo ogni
connessione; password da `.pgpass`, da comando shell o chiesta ogni volta; master password
con file cifrato; certificati client; latenza, keep-alive e riconnessione; connessione da URL,
URL JDBC o stringa ADO.NET/ODBC incollata, con `ConnectionConfig::parse_any` in
`platypus-core`, e *Copia come URL* senza password). Il driver di una connessione nuova si
sceglie solo nel picker: il form non lo cambia. La conferma
prima di scrivere in Prod è nel gruppo L. Restano i P3.

**Area:** nuovo crate `platypus-net`, `platypus-store`, `apps/desktop/src/connections/` (`ConnectionDialog.tsx`)
**Dipende da:** 0.7

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ **Tunnel SSH** (`russh`): password, chiave, agent, jump host | P1 | D |
| ✅ **Tipi di connessione** Dev/Test/Prod: colore, conferma prima di scrivere, autocommit off di default in Prod | P1 | D |
| ✅ Proxy SOCKS5 e HTTP | P3 | D |
| ✅ **Profili di rete** riutilizzabili tra connessioni | P2 | D |
| ✅ Inizializzazione: schema e database di default, query di bootstrap, fuso orario | P2 | D |
| ✅ Comandi shell su eventi (prima della connessione, dopo la disconnessione), ad esempio un VPN | P3 | D |
| ✅ Autenticazione: `.pgpass`, password da comando shell, chiedi ogni volta | P2 | D |
| ✅ SSPI (Kerberos o NTLM) per SQL Server su Windows, Entra ID, IAM AWS e GCP | P3 | D+ |
| Kerberos (GSSAPI) per PostgreSQL e MySQL | P3 | D+ |
| ✅ **Master password** per le credenziali salvate (oltre al keychain) | P2 | D |
| ✅ Gestione certificati SSL: CA personalizzate, certificato client | P2 | D |
| ✅ Import delle connessioni da DBeaver (`data-sources.json`), DataGrip, pgAdmin, `~/.pg_service.conf` | P1 | D |
| ✅ Indicatore di salute della connessione (latenza, keep-alive, riconnessione) | P2 | N |
| ✅ Import di progetti DBeaver (`.dbp`) con cartelle; cartelle di connessioni nei progetti e cartelle di progetti nella barra | P2 | D |
| ✅ Kubernetes port-forward, AWS SSM | P3 | D+ |
| ✅ Connessione da URL incollato (`postgres://…`, `mysql://…`, `Server=…;Database=…`): i campi si compilano da soli | P2 | D |
| ✅ Variabili d'ambiente e file `.env` nei campi della connessione (`${env:DB_PASSWORD}`) | P2 | D |
| ✅ 🆕 Credenziali da gestori esterni: 1Password CLI, Bitwarden, HashiCorp Vault, AWS Secrets Manager | P3 | D+ |
| ✅ 🆕 Test di tutte le connessioni di un progetto in una volta, con l'esito per ognuna | P3 | N |
| ✅ **Modalità pooler** per PostgreSQL (PgBouncer, Supavisor, pooler di Neon) in transaction o session mode: in transaction mode niente prepared statement con nome, che il pooler rompe; proposta di default nei profili Supabase e Neon | P2 | N |

---

### Gruppo G — Nuovi driver

**Area:** un crate `platypus-driver-*` per driver e i relativi test di conformance. È il gruppo più
parallelizzabile: **una persona per driver**.
**Dipende da:** 0.5, 0.6

**Stato: completato** (P2: ClickHouse, i server con protocollo PostgreSQL,
ADBC con Snowflake, BigQuery, Databricks e Trino, driver scaricati al primo uso, ODBC con Db2 e
ODBC generico, Redis; P3: libSQL/Turso, MongoDB, Oracle nativo con Instant Client scaricato al primo
uso, Elasticsearch/OpenSearch, Cassandra/ScyllaDB, aggiornamento dei driver scaricati).
Cassandra/ScyllaDB (30 ottobre 2026, `platypus-driver-cassandra` sul crate `scylla`): CQL con
script divisi rispettando stringhe, `$$` e `BEGIN BATCH … APPLY BATCH`, colonne Arrow tipizzate
(collezioni, tuple e UDT come JSON), pagine con il paging state, LWT con `[applied]`; keyspace come
schemi, tabelle, viste materializzate, tipi, funzioni e aggregati da `system_schema`, con il DDL
ricostruito; modifica in griglia per chiave primaria completa con anteprima e script di
annullamento; controlli di sicurezza tramite l'analisi SQL. Con un solo host (o un tunnel) tutto
passa da quel nodo. Provato su Cassandra 5.0 e ScyllaDB 2025.1 (servizi `cassandra` e `scylla` di
`docker/compose.yml`). Limiti: una scrittura CQL dice "0 righe" (il protocollo non le conta);
annullare smette di aspettare ma la query continua sul server; il data viewer non conta, ordina
né filtra (CQL non ha `OFFSET`); inserimenti e modifiche sono upsert; niente terminale (cqlsh
vuole la password sulla riga di comando); `verify-full` fallisce con certificati per nome
(usare `verify-ca`). Aggiornamento dei driver (stessa data, `platypus-drivers/src/versions.rs`):
più versioni di un driver ADBC scaricato una accanto all'altra con la versione attiva in
`ACTIVE`, controllo delle novità una volta al giorno (impostazioni `drivers.check_updates` e
`drivers.auto_update`), *Versions…* per installare o tornare a un'altra versione, e dopo ogni
cambio la prova delle connessioni salvate che lo usano con *Go back*. Limiti: i driver nativi si
aggiornano con l'app; Oracle Instant Client, DuckDB e Db2 restano a una versione per release;
le sessioni aperte tengono la libreria già caricata. Elasticsearch/OpenSearch (`platypus-driver-search`, un solo driver: il server dice quale è): richieste nella forma della console di Kibana e SQL (`_sql`, `_plugins/_sql`) con colonne tipizzate e cursore, ricerche lette a pagine (point in time e `search_after`, scroll su OpenSearch) con colonne fisse e `_other` per i campi non mappati, indici come tabelle e alias/data stream come viste, modifica in griglia con anteprima e script di annullamento, conferma per `_delete_by_query`/`_update_by_query` senza filtro e `DELETE /_all`, sola lettura, MCP, Cancel sul server, login Basic o chiave API. Provato su Elasticsearch 8.15 e OpenSearch 2.17 (profilo `search` di `docker/compose.yml`, conformance e scenari E2E `search-*`), non su Elasticsearch 7.x. SQL Server usa `tiberius` copiato
in `vendor/` con le patch per la negoziazione della cifratura; il dialetto Transact-SQL scrive `TOP (n)` e `[nomi]`,
e la transazione segue `@@TRANCOUNT`. DuckDB è incorporato come SQLite, con risultati Arrow nativi;
senza l'estensione ICU non ha fuso orario di sessione. Una nuova connessione parte dalla scelta del
database in una griglia di loghi. Conformance su PostgreSQL, MySQL, MariaDB, SQL Server, SQLite e
DuckDB (SQL Server dal profilo `sqlserver` di `docker/compose.yml`).
Versioni vecchie dei server (P2): `platypus_core::ServerVersion` legge la versione dal testo
del server e `minimum_version` dà la più vecchia provata (PostgreSQL 12, MySQL 5.7, MariaDB
10.6, SQL Server 2017, MongoDB 5); sotto, la sessione si apre con un messaggio. Il driver MySQL
ripiega in chiaro in `prefer` anche quando il TLS del server è troppo vecchio per rustls, legge i
CHECK solo da 8.0.16 e i piani da `EXPLAIN FORMAT=JSON` prima di 8.0.16. Il profilo `versions` di
`docker/compose.yml` avvia le versioni vecchie e `node scripts/conformance-versions.mjs` fa girare
la conformance su ognuna.
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

Redis (`platypus-driver-redis`, crate `redis` su uno stream TCP/rustls proprio, RESP3 con ripiego
su RESP2): una connessione per database (niente `SELECT` su una connessione condivisa); l'editor
scrive comandi, uno per riga, con le virgolette di `redis-cli`; `BROWSE [db<n>.]tipo [MATCH p]
[LIMIT n]`, comando del client, legge le chiavi di un tipo con `SCAN … TYPE` e le chiavi grandi a
pezzi (`HSCAN`, `LRANGE`…) man mano che la griglia scorre. Tabelle virtuali per tipo (anche
`stream`, e `json` con RedisJSON), con il numero di chiavi fino a 10.000. Le modifiche in griglia
diventano comandi (`edit::script`) eseguiti in un solo `MULTI`/`EXEC`, con anteprima e script di
annullamento; una chiave, un campo o un membro nuovo o rinominato che esiste già ferma il
salvataggio prima che parta (`guard`). `MULTI` nell'editor accoda sul client. Valori non UTF-8 mostrati con escape e
riletti nelle modifiche. Conformance in `tests/redis.rs` (servizio `redis`, Redis Stack, tra quelli
di default di `docker/compose.yml`). Niente explain né `SUBSCRIBE`/`MONITOR`.

MongoDB (`platypus-driver-mongo`, driver ufficiale con rustls): l'editor scrive mongosh, che un
parser proprio traduce in comandi; i risultati sono una griglia dei campi di primo livello con
l'albero del documento selezionato accanto. Niente transazioni, explain né modifica dei dati;
conformance dedicata in `tests/mongo.rs` (profilo `mongo` di `docker/compose.yml`).

Oracle (`platypus-driver-oracle`): crate `oracle` (ODPI-C), che carica Oracle Instant Client a
runtime: quello scaricato dall'app (`platypus-drivers`, Basic Light 23.26.4 per Windows x64 e
Linux, SHA-256 di Oracle, licenza mostrata nel popup) o quello del sistema; niente ODBC né UAC.
Un thread per connessione, cancel vero con `break_execution` (attraverso il port forwarding di
Docker Desktop un `DBMS_SESSION.SLEEP` non lo sente: le query sì), metadati dalle viste `ALL_*`,
DDL da `DBMS_METADATA`, numeri con precisione come interi o decimali, `NUMBER` senza precisione
come testo esatto. Dialetto Oracle e splitter PL/SQL (fine al `/` su riga propria). Senza
Visual C++ Redistributable (Windows) o `libaio` (Linux) l'errore lo dice. Conformance sul
profilo `oracle` (`gvenzl/oracle-free`), saltata senza Instant Client
(`PLATYPUS_TEST_ORACLE_CLIENT=download` lo scarica). ODBC (`platypus-driver-odbc`, `odbc-api` +
`arrow-odbc`, solo Windows: altrove bisognerebbe linkare unixODBC): Db2 col driver IBM
(clidriver 12.1.4, registrato con `db2cli install -setup` come amministratore) e ODBC generico
(DSN, driver e connection string suggeriti dal sistema); niente `SQLCancel` senza unsafe, come
ADBC. Conformance ODBC su SQL Server col driver `SQL Server` di Windows; Db2 senza container.

| Driver | Prio | Note |
|---|---|---|
| ✅ SQL Server (`tiberius`) | P1 | Dialetto `TOP`, `[quoting]`, più result set |
| ✅ DuckDB | P1 | Arrow nativo; serve anche al gruppo M |
| ✅ ClickHouse | P2 | HTTP con formato Arrow |
| ✅ File come database: CSV, Parquet, JSON, XLSX (tramite DuckDB) | P2 | Fatto con il gruppo H: viste DuckDB sul file, un foglio XLSX per tabella (calamine); si aprono col trascinamento sulla finestra |
| ✅ CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon; AlloyDB no | P2 | Wire PG: profili e test, non driver nuovi |
| ✅ ADBC e ODBC generici (Snowflake, Oracle, Db2, Databricks, BigQuery, Trino) | P2 | ADBC (Snowflake, BigQuery, Databricks, Trino, ADBC generico; driver scaricati dal registro dbc). Oracle con driver nativo (crate `oracle`, Instant Client scaricato da Oracle al primo uso, cancel con break, metadati da `ALL_*`). ODBC (`platypus-driver-odbc`, solo Windows): Db2 col driver IBM installato al primo uso (registrazione con UAC), ODBC generico con DSN o connection string |
| ✅ **Versioni vecchie dei server**: versioni minime dichiarate per ogni database (es. PostgreSQL 12+, MySQL 5.7+, MariaDB 10.6+, SQL Server 2017+, MongoDB 5+), query di catalogo, admin e DDL adattate alla versione letta alla connessione (es. `prokind` da PG 11, `attgenerated` da PG 12), messaggio chiaro sotto la minima | P2 | Conformance su una matrice di versioni in `docker/compose.yml` (un profilo per le versioni vecchie, ad esempio `postgres:12`…`16`, `mysql:5.7`, `mysql:8.0`); oggi i test girano solo sull'ultima versione |
| ✅ Aggiornamento automatico dei driver di tutti i database (scaricati ADBC/ODBC e nativi), con verifica che i dialetti restino compatibili; scelta di versioni precedenti di un driver | P3 | Scaricati: registro con più versioni. Nativi: compilati nell'app, si aggiornano con l'app; versioni vecchie = più versioni dello stesso crate o driver caricabili a parte. Test di compatibilità per versione |
| ✅ Oracle nativo | P3 | Crate `oracle` con Instant Client scaricato al primo uso (Oracle Free Use Terms) |
| ✅ **MongoDB** | P3 | Sintassi mongosh, griglia dei campi + albero del documento |
| ✅ **Redis** | P2 | Il NoSQL più richiesto: chiavi per tipo (string, hash, list, set, zset, JSON) come tabelle virtuali, comandi nell'editor, modifica dei valori; stesso schema di MongoDB (griglia + albero) |
| ✅ libSQL/Turso; Cloudflare D1 valutato, non fatto | P3 | Varianti di SQLite |
| ✅ 🆕 Elasticsearch/OpenSearch, Cassandra/ScyllaDB | P3 | Query DSL e CQL; stesso schema di MongoDB (griglia + albero). Elasticsearch/OpenSearch provato su 8.15 e 2.17 (non su 7.x); Cassandra 5.0 e ScyllaDB 2025.1 |

---

### Gruppo H — Import, export e trasferimento dati

**Area:** `platypus-transfer`, `apps/desktop/src/transfer/`, `commands/transfer.rs`
**Dipende da:** 0.5

**Stato: completato** (P1, P2 e P3, con i profili di trasferimento salvati, la bulk copy di
SQL Server e l'appender di DuckDB).

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

**Caricamento bulk, file SQL e dump (P2).** `Connection::bulk_load` (flag `bulk_load`) carica
righe di testo: PostgreSQL con `COPY … FROM STDIN` in CSV (non CockroachDB), MySQL e MariaDB
con `LOAD DATA LOCAL INFILE` dove il server ha `local_infile` (letto alla connessione; un
avviso fa fallire il caricamento). L'import e il trasferimento lo usano a blocchi di 50.000
righe, tranne l'upsert; gli altri database restano con INSERT a blocchi. In
manual commit il caricamento sta sotto il savepoint dell'istruzione. P3: SQL Server con la bulk copy di
TDS (`INSERT BULK` con `KEEP_NULLS, CHECK_CONSTRAINTS, FIRE_TRIGGERS`, i valori dell'identity
tenuti): il driver converte il testo nel tipo di ogni colonna, letto con `SELECT TOP 0`, prima
di iniziare. Una tabella con `money`, `xml`, `text`, `image` o tipi CLR risponde `Unsupported`:
l'import lo sa da un caricamento vuoto prima delle righe e scrive con `OPENJSON` (o INSERT sotto
il livello di compatibilità 130). Un valore che la conversione non legge (`01/02/2024`) rimanda
il blocco a `OPENJSON`, dove decide SQL Server. Il `tiberius`
vendorizzato ha `bulk_columns`/`bulk_insert_columns` e le correzioni ai metadati di `date`,
`nvarchar`, `time`, `datetimeoffset` e `smalldatetime`. DuckDB con l'appender
(`appender_with_columns`, nella transazione della sessione o in una propria, così una riga
fallita non lascia niente, e un nome senza schema segue il `search_path`); i `TIMESTAMPTZ` si
convertono in UTC nel driver, perché il cast dell'appender perde l'offset. Conformance: `bulk_loads_rows` su tutti, e in `value_fidelity`
ogni tipo di SQL Server confrontato con un `INSERT` dello stesso testo. Su un server locale (Docker,
stesso host) la bulk copy va come `OPENJSON`: un milione di righe in circa 9 s in tutti e due i
casi; il guadagno atteso è sul lavoro del server (niente JSON da leggere), da misurare su un
server remoto. Il `tokio-postgres`
vendorizzato ha una patch per un `COPY` rifiutato dal server. "Esegui file SQL…"
(`platypus-transfer::script`) legge `.sql`, `.gz` e `.zst` a pezzi di 4 MB e divide le
istruzioni col dialetto (`GO` in T-SQL, `DELIMITER` di MySQL anche tra un pezzo e l'altro);
nella transazione unica, dove il DDL committa da solo (MySQL, Oracle) il resto continua in una
nuova e l'errore dice quali istruzioni restano. "Dump in un file SQL…" (`platypus-transfer::dump`)
scrive tipi e domini, sequenze, tabelle in ordine di chiavi esterne (aggiunte dopo le righe
dove il database lo permette), righe (in PostgreSQL di ogni tabella e partizione con `ONLY`),
numerazione e posizione delle sequenze, viste, routine (tra `DELIMITER` in MySQL, con `/` in
Oracle) e trigger; scrive su `<file>.part` e lo rinomina alla fine. Conformance su PostgreSQL,
MySQL, MariaDB, SQL Server, SQLite e DuckDB; E2E `sql-file-dump-sqlite`.

**Profili di trasferimento (P3).** Export di una query o di tabelle, import di un file, copia di una
tabella e migrazione di uno schema si salvano con le scelte del loro dialog ("Save as profile…",
e "Save profile" sostituisce quello aperto) in `transfer-profiles.json` nella cartella dati
(`apps/desktop/src-tauri/src/profiles.rs`, comandi e run in `commands/profiles.rs`). Il file tiene
le opzioni come le manda il dialog e a ogni salvataggio controlla che si rileggano. Le chiavi
segrete (chiave del bucket e URL firmato dell upload) non entrano nel file: stanno nello store
delle password sotto un id derivato da quello del profilo, tornano al momento del run, si
rimuovono col profilo e seguono le password quando si attiva o si toglie la master password. Una
migrazione di uno schema intero si salva come "tutte le tabelle" (`tables: null`) e legge le tabelle
al run; una selezione tiene l elenco. Il dialog **Transfer profiles…** (palette, e menu della
connessione per i suoi) elenca i profili e li esegue, apre, programma, rinomina ed elimina; un
profilo usato da un task non si elimina. Aprendo un profilo il dialog avvisa di tabelle e colonne
che non ci sono più. Il task dello scheduler `Profile` esegue un profilo; un export va nella
cartella del task, se ce l ha. E2E `profiles-export-sqlite`, `profiles-transfer-sqlite` e
`profiles-schedule-sqlite`.

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
| ✅ **Caricamento bulk nativo** per import e trasferimento: `COPY` in PostgreSQL, `LOAD DATA LOCAL` in MySQL; INSERT a blocchi come ripiego | P2 | D |
| ✅ 🆕 Bulk copy (TDS) in SQL Server e appender in DuckDB al posto di `OPENJSON` e INSERT a blocchi | P3 | D |
| ✅ Profili di import, export e trasferimento salvati e riutilizzabili (anche dal task scheduler del gruppo J) | P3 | D |
| ✅ **Esegui file SQL** dal navigator (`.sql` o `.sql.gz`), con avanzamento, annullamento e una sola transazione dove il database annulla il DDL; **dump SQL** di uno schema o di un database in un solo file (DDL e dati), che si riapre con lo stesso comando | P2 | D |

---

### Gruppo I — Strumenti di schema

**Stato: completato**. Editor tabella grafico (P1): il crate `platypus-schema` legge il design di
una tabella (`load`: dettagli del driver più le azioni delle FK, gli `auto_increment` e i default
letterali di MySQL, l'`AUTOINCREMENT` di SQLite) e genera lo script da un design all'altro
(`script`): le parti cambiate si eliminano e si ricreano, le colonne si rinominano e si
modificano sul posto (`CHANGE COLUMN` in MySQL, `sp_rename` e il vincolo di default per nome
in SQL Server). SQLite e DuckDB, che oltre a rinominare e aggiungere colonne non modificano
quasi nulla, ricreano la tabella copiando le righe (SQLite con `foreign_keys` spento; i suoi
trigger vanno ricreati a mano). `apply` esegue tutto in una transazione dove il DDL si annulla
(non MySQL). Il tab `table-editor` mostra l'anteprima dello script prima di salvare. Limiti: i
commenti di colonna non si scrivono in SQLite; le colonne generate e gli indici su espressioni
restano come sono solo se non si toccano.

Diagramma ER, linter (P2), diagrammi personalizzati e confronto schemi (P3): `platypus-schema` legge
tutte le tabelle di uno schema (`read_tables`, `load_designs`, quattro alla volta, senza le
partizioni). Il tab `erd` dispone le tabelle a colonne per profondità delle FK (quelle senza FK in
una griglia sotto), le tabelle si trascinano e le posizioni, le tabelle scelte, le note e lo zoom
restano con il tab; export SVG e PNG con `exportChart` del gruppo M; oltre 100 tabelle chiede
prima quali mostrare. Il linter (`lint`) segnala tabelle senza PK, FK senza indice (un indice
che inizia con le sue colonne, in qualunque ordine), indici duplicati e contenuti in un altro,
tipi diversi tra FK e colonna referenziata (stesso schema), con il `CREATE INDEX`/`DROP INDEX`
da aprire nell'editor. Il confronto (`compare`) abbina tabelle e parti per nome: le tabelle nuove
si creano e quelle in più si eliminano nell'ordine delle FK, le altre passano per `script`;
solo tra database dello stesso tipo, lo script si apre nell'editor della destinazione. Limiti:
una colonna rinominata diventa eliminata e aggiunta; viste, routine e sequenze non si
confrontano; le tabelle in più si eliminano dopo le modifiche alle altre (cambiare la PK di una
tabella ancora referenziata da una che sparisce fallisce); il linter non vede il WHERE degli
indici parziali né le colonne INCLUDE, e lo dice nel messaggio; i diagrammi vivono nel tab (chiuso il tab, il diagramma si perde).

P3 del 29–30 ottobre 2026. **Diagramma ER**: pulsante *Full screen* (il tab copre la finestra, Esc
esce); *Refresh* fissa prima le posizioni di tutte le tabelle e mette quelle nuove sotto
(`placeTables` in `erd/layout.ts`). **Modalità modifica** (*Edit*, non sulle connessioni in sola
lettura): tabelle nuove, modificate (dialog `DesignDialog` con il `DesignForm` estratto dal
table editor) ed eliminate, FK trascinando il cerchietto di una colonna su un'altra tabella (verso
la sua PK di una colonna); le modifiche restano con il tab (`edits`) e *Generate DDL* apre nell'editor
lo script di `platypus_schema::edited_script` (il design voluto passato a `compare`). Limiti: dopo
l'esecuzione le modifiche restano finché non si scartano; le tabelle esistenti non si rinominano;
eliminare una tabella non tocca le FK di tabelle non modificate che la puntano. **Form per oggetti**
(`platypus_schema::create_object`, `ObjectFormDialog`, sottomenu *Create* dello schema e *Create
trigger…* della tabella): viste (anche materializzate in PostgreSQL e Oracle), sequenze, funzioni
(macro in DuckDB) e trigger (in PostgreSQL la funzione `<trigger>_fn` più il trigger) per
PostgreSQL, MySQL/MariaDB, SQLite, SQL Server, Oracle e DuckDB, con anteprima e Create che esegue
gli statement uno a uno. Limiti: le sequenze si offrono su ogni connessione MySQL (solo MariaDB le
ha, il dialog lo dice); `CREATE OR REPLACE TRIGGER` di PostgreSQL vuole la 14. **Confronto dati**
(`platypus_transfer::compare`, *Compare data with…* della tabella): righe abbinate per PK (o chiave
unica di colonne NOT NULL) tra database dello stesso tipo, confrontate come testo, con filtro
WHERE comune; INSERT (identità con override), UPDATE delle sole colonne cambiate, DELETE delle righe
in più solo se richiesto. Limiti: 500.000 righe per lato in memoria, 200 differenze mostrate (lo
script le copre tutte), colonne calcolate escluse. **Changelog Flyway e Liquibase** dal confronto
schemi (scelta *Output* in `CompareDialog`): file `V<data>__<descrizione>.sql` senza BEGIN/COMMIT, o
Liquibase "formatted SQL" con un changeset per statement (`runInTransaction:false` per i PRAGMA di
SQLite). **Timeline dello schema** (tab *Schema history*, `schema-history.db` in `platypus-store`):
istantanee a mano o pianificate (task `snapshot` dello scheduler), una nuova solo se lo schema è
cambiato; tra due istantanee, o tra una e lo schema di adesso, le modifiche per tabella
(`platypus_schema::summary`) e lo script. Limiti: solo ciò che legge `load_designs` (non viste,
routine, sequenze); parti abbinate per nome; nessuna pulizia automatica. **Stato delle migrazioni**
(*Migration status…* dello schema, `platypus_schema::migrations`): Flyway (checksum CRC32 come
Flyway, `R__` ripetibili, fuori ordine, baseline), Liquibase (XML, YAML a blocchi, formatted SQL;
senza MD5), Prisma (SHA-256), Alembic (grafo delle revisioni) e Rails, contro le tabelle di
storico; stati applicata, pendente, divergente, fallita, mancante. Gli scenari E2E di queste P3
(`erd-*`, `schema-*`) sono scritti ma non ancora eseguiti (runner Windows).

**Area:** `apps/desktop/src/erd/` (tab `erd`), `apps/desktop/src/schema/` (linter e confronto), `apps/desktop/src/table-editor/`, nuovo crate `platypus-schema` (modello di schema, diff, DDL)
**Dipende da:** 0.3, 0.5; per il DDL completo serve il gruppo C

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Diagramma ER in sola lettura dalle FK, layout automatico, export PNG/SVG | P2 | D |
| ✅ 🆕 Diagramma ER a schermo intero e pulsante **Aggiorna** che rilegge lo schema (tabelle nuove, colonne e FK cambiate) tenendo le posizioni già scelte | P3 | N |
| ✅ Diagrammi personalizzati: si scelgono le tabelle, con note | P3 | D |
| ✅ ERD in modifica: si disegna lo schema e si genera il DDL | P3 | D+ |
| ✅ **Editor tabella grafico**: crea e modifica colonne, indici, vincoli e FK, con anteprima dell'ALTER | P1 | D |
| ✅ Creazione di viste, trigger, funzioni e sequenze da form | P3 | D |
| ✅ **Confronto schemi** con script di migrazione | P3 | D+ |
| ✅ **Confronto dati** tra due tabelle, con script di sincronizzazione | P3 | D+ |
| ✅ Changelog Liquibase/Flyway generati dal confronto | P3 | D+ |
| ✅ Timeline delle modifiche di schema (snapshot periodici e diff) | P3 | N |
| ✅ **Linter dello schema**: tabelle senza PK, FK senza indice, indici duplicati o contenuti in altri, tipi diversi tra FK e colonna referenziata | P2 | N |
| ✅ 🆕 Stato delle migrazioni di un progetto (Flyway, Liquibase, Prisma, Alembic, Rails): applicate, pendenti, divergenti | P3 | N |

---

### Gruppo J — Amministrazione del server

**Stato: P2 completati, P3 in parte** (fatti utenti e ruoli, dashboard e parametri del server;
il debugger PL/pgSQL è scritto ma mai provato su un server con `pldebugger`, vedi "Correzioni
aperte"). Dal 30 ottobre 2026: **parametri del server** (`server_parameters`, capacità omonima:
`pg_settings`, variabili di MySQL con la loro origine, `information_schema.SYSTEM_VARIABLES` di
MariaDB, `sys.configurations` di SQL Server con un elenco dei default, `system.settings` e
`system.server_settings` di ClickHouse, 31 PRAGMA di SQLite, `duckdb_settings()`), con ricerca,
"solo cambiati" e l'istruzione per cambiarli aperta nell'editor (mai eseguita da sola);
**dashboard** (`server_metrics`: connessioni, transazioni e statement al secondo calcolati tra
due letture, cache hit, dimensioni dei database, replica, uptime; ogni misura che il server
rifiuta mostra "—" con il motivo; PostgreSQL, MySQL, MariaDB, SQL Server, ClickHouse);
**utenti, ruoli e permessi** (`principals` e `principal_privileges`, capacità `security`; lo SQL
lo scrive `platypus_sql::security` per PostgreSQL, MySQL/MariaDB, SQL Server e ClickHouse): crea,
modifica, elimina (con `REASSIGN OWNED`/`DROP OWNED` in PostgreSQL), GRANT/REVOKE con grant
option e appartenenza ai ruoli, anteprima con la password mascherata, che non arriva mai nel
query log. Limiti: in MySQL serve `SELECT ON mysql.*` per leggere i permessi; SQL Server solo il
database corrente e niente `DENY`; si vedono i permessi diretti, non quelli ereditati dai ruoli;
in MySQL le transazioni al secondo contano solo i COMMIT/ROLLBACK espliciti. **Debugger
PL/pgSQL** (`Connection::debugger`, capacità `debugger`, `platypus-driver-pg/src/debug/`, tab
`debugger`): chiamata diretta di una funzione o procedura su due sessioni proprie
(`pldbg_oid_debug`, `pldbg_attach_to_port`), passi dentro e sopra, continua, breakpoint nella
colonna del sorgente, stack, variabili modificabili, risultato e messaggi alla fine, Stop che
non lascia la sessione bloccata; profilo `pldebug` di `docker/compose.yml` con l'immagine che
installa `pldebugger` (non costruibile nel container cloud: apt bloccato). Limiti: niente
breakpoint globali né funzioni trigger; serve `plugin_debugger` in `shared_preload_libraries`.
Dal 29 ottobre 2026: query più costose e statistiche di tabelle e
indici (`top_queries` e `object_statistics`, capacità `server_statistics`; PostgreSQL da
`pg_stat_statements` e `pg_stat_user_tables/indexes`, MySQL e MariaDB da `performance_schema`
e `information_schema`, SQL Server da `dm_exec_query_stats`, `dm_db_partition_stats` e
`dm_db_index_usage_stats`); manutenzione (`maintain`, capacità `maintenance` con le operazioni
del database) dal tab e dal menu del navigator; backup e restore con gli strumenti del
database (`commands/backup.rs`: `pg_dump`, `pg_restore`, `psql`, `mysqldump`, `mysql`,
avanzamento per tabella o per byte, clausole `DEFINER` tolte da un restore MySQL); task
pianificati (`scheduler/` e `commands/schedules.rs`: script, export e backup con ora, fuso
orario IANA, cartella, copie tenute e nuovi tentativi; `schedules.json` nella cartella dei
dati; girano ad app aperta, un orario perso si recupera all'avvio; il backup pianificato
degli altri driver, MongoDB escluso, è il dump SQL dello schema del gruppo H). `docker/compose.yml`
precarica `pg_stat_statements` e accende `performance_schema` in MariaDB. Prima: session manager e
lock manager. Due capacità opzionali del trait
`Connection` (`platypus-core/src/admin.rs`): `server_sessions` (`server_sessions`,
`server_locks`, `end_server_session` con `Terminate`) e `cancel_server_queries`
(`end_server_session` con `Cancel`, che SQL Server non ha). Driver: PostgreSQL
(`pg_stat_activity`, `pg_blocking_pids`, `pg_locks`; non CockroachDB né YugabyteDB), MySQL
(`PROCESSLIST`, `performance_schema.data_lock_waits`, `data_locks` e `metadata_locks`),
MariaDB (`INNODB_LOCK_WAITS`, `INNODB_LOCKS`), SQL Server (`dm_exec_sessions`,
`dm_exec_requests`, `dm_tran_locks`, `KILL`). Tab `admin` (*Server activity…* dal menu della
connessione, `apps/desktop/src/admin/`) sulla sessione del navigator, in autocommit: elenco
con refresh ogni 2–30 s o a mano, filtro, sessioni bloccate, catene di attesa, lock; annulla e
chiudi con conferma, rifiutati sulle connessioni in sola lettura e scritti nel query manager.
Limiti: Oracle non ancora; senza i diritti del server (`PROCESS`, `VIEW SERVER STATE`, …) si
vedono solo le proprie sessioni, e i lock di MySQL danno l'errore del server; MariaDB mostra
solo i lock coinvolti in un'attesa.

**Area:** `apps/desktop/src/admin/` (tab `admin`), `commands/admin.rs`, query di sistema nei driver
**Dipende da:** 0.3, 0.6

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ **Session manager**: sessioni attive, query in corso, kill e cancel | P2 | D |
| ✅ **Lock manager**: blocchi e catene di attesa | P2 | D |
| ✅ Backup e restore con `pg_dump`/`pg_restore` e `mysqldump`, con avanzamento | P2 | D |
| ✅ Manutenzione: VACUUM, ANALYZE, REINDEX, OPTIMIZE e CHECK TABLE dal menu | P2 | D+ |
| ✅ Utenti, ruoli e permessi (GRANT/REVOKE da form) | P3 | D |
| ✅ Dashboard del server: connessioni, TPS, cache hit, dimensioni, replica | P3 | D+ |
| ✅ **Task scheduler**: script, export e backup pianificati. Ogni piano ha ora e fuso orario, cartella, copie da tenere e un nuovo tentativo se fallisce; si vedono l'ultima esecuzione, il suo esito e la prossima, con "Esegui ora". Il backup è il dump SQL del gruppo H, quindi vale per tutti i driver senza `pg_dump` | P2 | D+ |
| Debugger PL/pgSQL (scritto, da provare con il profilo `pldebug`) | P3 | D+ |
| ✅ 🆕 **Query più costose del server**: `pg_stat_statements`, `performance_schema`, Query Store di SQL Server, con apertura nell'editor e nel tab `explain` | P2 | N |
| ✅ 🆕 Statistiche di tabelle e indici: indici mai usati, bloat, tabelle con molte scansioni complete, ultimo VACUUM/ANALYZE | P2 | N |
| ✅ 🆕 Parametri del server (`pg_settings`, `SHOW VARIABLES`) con ricerca e differenze dai valori di default | P3 | N |

---

### Gruppo K — Shell dell'applicazione ed ergonomia — ✅ completato

**Stato: completato** (import ed export dei temi come file JSON in Impostazioni →
Temi, con anteprima, elenco di ciò che il file salta e un tema importato chiaro, scuro o che
segue il sistema) (rotella sui tab, command palette con ricerca fuzzy su azioni,
connessioni, tabelle e query recenti, riapertura del tab chiuso, pagina Impostazioni con
sezioni, scorciatoie personalizzabili, barra degli strumenti configurabile).
Fatta anche la **nuova grafica**: temi Platypus Dark e Light, font Inter e JetBrains Mono
inclusi, icone Lucide, navigator, tab, toolbar e dialog ridisegnati
([spec](superpowers/specs/2026-10-24-grafica-e-progetti-design.md), parte A). Fatti anche i
**progetti come spazi di lavoro** (parte B): barra dei progetti a sinistra, connessioni in
più progetti, tab, cronologia, palette e titolo della finestra per progetto, `Ctrl+1`…
per cambiare progetto, `connections.toml` versione 2. Poi la grafica **Cornice**: la cornice
della finestra (barra superiore, barra dei progetti, navigator) prende la tinta del tipo della
connessione della scheda attiva (DEV, TEST, PROD) attorno a un foglio arrotondato con il lavoro;
barra superiore con progetto, connessione e tipo, ricerca che apre la palette, tema e
impostazioni; schermata di benvenuto con apertura di un file di dati e import delle connessioni.
La barra superiore fa anche da barra del titolo (pulsanti della finestra, trascinamento; con
l'impostazione "Title bar" torna quella di sistema, sempre su macOS) e le conferme sono dialog
dell'app (`ui/ask.tsx`) al posto di `window.confirm`. I pulsanti delle barre (editor SQL, modifica
della griglia, explain, oggetti, designer di tabella) mostrano solo l'icona, con il tooltip; Run e
le azioni dei dialog tengono la parola. Poi i **P2**: menu della scheda (fissa, apri accanto, chiudi
le altre o quelle a destra), schede raggruppate per connessione (Impostazioni › Tabs), editor
diviso (`Ctrl+Shift+D`), tabelle e colonne trascinate dal navigator nell'editor (nome, o `SELECT`
in un editor vuoto o con Shift), interfaccia in italiano e inglese (`i18n.ts`, `t()` con il
testo inglese come chiave, controllo `scripts/i18n-check.mjs` nella build; la documentazione
delle funzioni SQL nel completamento resta in inglese), accessibilità (ruoli, nome e focus dei
dialog in `ui/modalA11y.ts`, navigator, schede e menu da tastiera, contrasto dei commenti SQL),
log delle attività e degli errori, notifica di sistema a fine query lunga
(`tauri-plugin-notification`), `Ctrl+=` / `Ctrl+-` / `Ctrl+0` e font della griglia. Poi le **rifiniture**: icone dell'app col logo tondo per le misure piccole,
"New connection…" nel menu di una connessione e in quello della lista (scope `navigator.list`),
ricerca e quattro colonne nella scelta del database, ricerca nelle Impostazioni (anche le
scorciatoie, per nome dell'azione), navigator ridimensionabile (`shell/SidebarResizer.tsx`,
larghezza in `layout.sidebar_width`), menu e dialog con un leggero effetto vetro. Dal ramo del
gruppo Q: navigator nascondibile (`Ctrl+B` e pulsante della barra superiore, `shell/sidebar.ts`,
`layout.sidebar_collapsed`), Impostazioni con una scheda per sezione (la ricerca mostra le
corrispondenze di tutte le sezioni, raggruppate), un solo stile per i dialog (parti spaziate dal
gap della colonna, avvisi, azioni a destra con la principale per ultima) e i token del vetro
(`--glass-*`) usati anche dai tooltip dell'editor. Dal 28 ottobre 2026, P3: **database di esempio**
nella schermata di benvenuto (`samples/wildlife.sql` creato nella cartella dei dati al primo
clic, con la connessione e quattro script salvati in `samples/scripts/`), **modalità portable**
(una cartella `data` accanto all'eseguibile, come in VS Code: tutto lì dentro, anche i dati del
web view; l'aggiornamento chiede di sostituire l'eseguibile), **modalità Vim** nell'editor
(`@replit/codemirror-vim`, `:w` salva) e **altre cinque lingue** (spagnolo, tedesco, francese,
portoghese del Brasile, cinese; dizionari caricati quando servono, numeri e plurali secondo la
lingua, nomi dei tasti come le tastiere locali). Restano pannelli trascinabili, mini-finestra
dalla system tray e tab staccato in una finestra.

**Area:** `App.tsx`, `apps/desktop/src/shell/` (tab bar, palette, impostazioni), `styles.css`, `theme.ts`
**Dipende da:** 0.2, 0.3, 0.7


**P3 finali.** Pannelli ridimensionabili (`shell/PanelResizer.tsx`): l'altezza dell'editor
(`layout.editor_height`, la stessa per tutti i tab), la larghezza dei pannelli laterali di un
risultato (`layout.side_panel_width`) e la divisione dell'editor diviso (`layout.split_share`,
in millesimi), trascinando o con le frecce, doppio clic per il default. Finestre accanto a
quella principale (`src-tauri/src/windows.rs`, `shell/WindowApp.tsx`): "Apri in una nuova
finestra" sposta un tab in una finestra `tab-<n>` con le sue sessioni (non con una transazione
aperta o modifiche della griglia); "Sposta nella finestra principale" lo riporta, chiudere la
finestra lo lascia a `Ctrl+Shift+T`, e un'uscita senza chiuderla (dalla tray) lo ritrova
all'avvio. La finestra `quick` è la query rapida, sempre in primo piano, dall'icona nella tray
(`tray-icon`, menu tradotto dal frontend; su Linux solo con AppIndicator) o dalla palette. Le
finestre si aprono da comandi asincroni (WebView2 si blocca altrimenti), il blocco dell'app le
raggiunge e l'attività in una tiene aperte le altre. E2E `windows-sqlite` (lo scenario raggiunge
le altre finestre con `connect(port, …, pick)`).
| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ **Rotella del mouse sui tab = scroll orizzontale** | P1 | N |
| ✅ **Command palette** (Ctrl+K o Ctrl+Shift+P): azioni, oggetti, connessioni e query recenti, ricerca fuzzy | P1 | N |
| ✅ Riapri l'ultimo tab chiuso (Ctrl+Shift+T) | P1 | N |
| ✅ Tab colorati per connessione, raggruppabili e fissabili; menu "chiudi altri" | P2 | N |
| ✅ Editor diviso, con due tab affiancati | P2 | N |
| ✅ Trascinare una tabella dal navigator nell'editor inserisce il nome o un `SELECT` | P2 | N |
| ✅ Pagina Impostazioni, scorciatoie personalizzabili, barra degli strumenti configurabile | P1 | D |
| ✅ Lingua dell'interfaccia: italiano e inglese, con l'infrastruttura i18n | P2 | D |
| ✅ Accessibilità: navigazione da tastiera completa, screen reader, contrasto | P2 | D |
| ✅ Pannelli trascinabili e layout salvato | P3 | D |
| ✅ Modalità portable, con impostazioni accanto all'eseguibile | P3 | N |
| ✅ Mini-finestra di query rapida dalla system tray | P3 | N |
| ✅ Modalità Vim nell'editor | P3 | N |
| ✅ Log degli errori e attività in background visibili, per la diagnosi | P2 | D |
| ✅ Notifica di sistema alla fine di una query lunga, quando la finestra non è in primo piano | P2 | D |
| ✅ Zoom dell'interfaccia e dimensione del font di editor e griglia (Ctrl+= / Ctrl+-) | P2 | D |
| ✅ 🆕 Tab staccato in una finestra separata (secondo monitor) | P3 | D |
| ✅ **Import ed export dei temi** come file JSON (colori dell'app e dell'editor, chiaro e scuro), con anteprima prima di applicarli; primo passo verso i mod | P2 | N |
| ✅ 🆕 **Database di esempio** nella schermata di benvenuto: un clic apre un SQLite o DuckDB incluso, con dati realistici, tabelle collegate da FK e qualche script salvato, per provare l'app senza un server | P3 | N |
| ✅ 🆕 Altre lingue dell'interfaccia (spagnolo, tedesco, francese, portoghese, cinese), con le stringhe di `i18n.ts` aperte ai contributi | P3 | D |

---

### Gruppo L — Sicurezza operativa — ✅ completato

**Stato: completato** (conferma o blocco di UPDATE/DELETE senza WHERE e TRUNCATE con le
righe stimate, conferma delle modifiche sulle connessioni Prod anche dal data editor, timer
della transazione aperta con avviso). P2: l'indicatore della transazione aperta conta le
modifiche e, cliccato, le elenca con Commit e Rollback (`guards/PendingDialog.tsx`; dopo ogni
statement che non è una query il tab legge lo stato della transazione, e la lista si svuota
quando si chiude). Il lucchetto della barra rende il tab di sola lettura (`readOnly` del
modello, non salvato): il tab vede la connessione come `read_only` e ne applica tutti i
controlli. `safety.auto_limit` (0, spento, di default) aggiunge un limite alle query
dell'editor senza `LIMIT`/`TOP`/`OFFSET`/`FETCH` propri (`platypus_sql::add_limit`: `TOP (n)`
in SQL Server, che rinuncia alle UNION; niente con le clausole di lock); il risultato tiene lo
statement com'è scritto e la barra di stato dice "LIMIT automatico".

**P3.** Restrizioni per connessione (`SavedConnection.restrictions`): `no_export` (niente export,
dump, copie in altri database, "Query with SQL", salvataggio di una cella, snapshot, e agenti
spenti) e `no_data_editing` (righe solo da SQL: niente data editor, import, dati generati); il
backend le controlla in `AppState::exportable`/`editable`, la UI nasconde le azioni. Registro
delle transazioni del tab (`guards/txLog.ts`): gli statement per transazione, con come è finita
(un fallimento o un rollback del database è "terminata", non "confermata"). Mascheramento per
la condivisione dello schermo (`guards/mask.ts`, Ctrl+Alt+M): colonne per nome e tipo in
Impostazioni › Safety, a puntini nella griglia (con le decorazioni, non in `DataGrid`), nei
pannelli, nel profilo della colonna e nei documenti MongoDB; copie, export e grafici restano
in chiaro. Snapshot prima di UPDATE/DELETE dall'editor (`platypus_sql::snapshot::touched_rows`,
`platypus_transfer::snapshot`): le righe toccate, lette paginate sulla sessione, fino a 10.000,
diventano gli statement che le rimettono (INSERT per un DELETE, UPDATE per chiave primaria), in
`snapshots/` nella cartella dei dati (gli ultimi 200); niente snapshot per join, LIMIT, tabelle
senza chiave o UPDATE della chiave. Blocco dopo N minuti di inattività (`safety.lock_minutes`,
spento di default) con la master password: chiude le sessioni Prod e rende inerte il resto della
finestra. Conformance degli snapshot su PostgreSQL, MySQL, MariaDB, SQL Server, SQLite e DuckDB;
E2E `restrictions-sqlite`, `transaction-log-sqlite`, `mask-sqlite`, `snapshot-sqlite`,
`app-lock-sqlite`.

**Area:** `platypus-sql/src/analysis.rs` (analisi dello statement prima dell'esecuzione), `apps/desktop/src/guards/`
**Dipende da:** 0.5; il gruppo F per i tipi di connessione

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Blocco o conferma per UPDATE/DELETE senza WHERE, con il numero di righe stimato | P1 | N |
| ✅ Conferma per DDL e DML sulle connessioni Prod, con il riepilogo degli statement | P1 | D |
| ✅ Timer della transazione aperta: avviso dopo N minuti, indicatore sempre visibile | P1 | N |
| ✅ **Pending transactions**: elenco degli statement non ancora committati, per tab | P2 | D |
| ✅ **Transaction log** della sessione | P3 | D |
| ✅ `LIMIT` automatico sulle SELECT senza limite, configurabile | P2 | N |
| ✅ Mascheramento dei dati sensibili per screen share (regole su nome e tipo di colonna) | P3 | N |
| ✅ Restrizioni di sicurezza per connessione: niente export, niente modifica | P3 | D+ |
| ✅ 🆕 Sola lettura temporanea per un tab o una sessione, senza cambiare la connessione salvata | P2 | N |
| ✅ 🆕 **Snapshot prima di UPDATE/DELETE manuali**: le righe toccate salvate in locale prima dello statement, con ripristino in un clic | P3 | N |
| ✅ 🆕 Blocco dell'app dopo N minuti di inattività (con la master password), disconnessione delle sessioni Prod | P3 | N |

---

### Gruppo M — Analisi dei dati

**Stato: completato** (grafici dai risultati con export PNG e SVG e configurazione salvata
nel tab, statistiche della colonna al passaggio del mouse, SQL sui risultati con un DuckDB in
memoria, diff con l'esecuzione precedente). Il calcolo è nel crate `platypus-local`, sulle
righe Arrow del lavoro della query (`jobs.rs`); il grafico è un modulo SVG proprio
(`analysis/Chart.tsx`, `analysis/chartScale.ts`), con i colori del tema. Limiti: grafici e
statistiche sulle righe caricate; al massimo 2.000 punti per serie (fasce di X con minimo e
massimo) e 50 categorie (le altre in "Altro"); SQL sui risultati fino al limite di memoria di
512 MB (oltre, tabella parziale con avviso), su un database DuckDB in memoria con nome del
driver (`memory_database`), raggiunto da una connessione temporanea mai salvata; con più
risultati la tabella `result` copia di nuovo il risultato scelto oltre a `rN`; diff con una
copia per tab, abbinata per chiave solo quando la griglia può modificare il risultato, al
massimo 10.000 righe mostrate; MongoDB senza SQL sui risultati né diff. Notebook
(`apps/desktop/src/notebook/`, tab `notebook`): celle Markdown (parser proprio in `html.ts`,
reso come elementi React e, nell'export, come HTML con tutto il testo escapato; link non
cliccabili nell'app) e SQL, nominate `c1`, `c2`… nell'ordine; una cella "sui risultati" gira sul
DuckDB in memoria di "Query with SQL" (`open_result_db` / `reload_result_db` con le tabelle
`cN` dei job delle altre celle, caricati per intero con `loadAll`), una per tab, rifatta quando
cambiano i job. Parametri condivisi (`analyze_sql` + `substitute`), controlli di Sicurezza e
connessioni read-only come nell'editor; sessione del tab in autocommit. File `.pnb` (JSON,
`format: "platypus-notebook"`), export HTML con al massimo 500 righe per risultato, il grafico
SVG mostrato e il piano.

P3 del 30 ottobre 2026. **Pivot** (*Pivot…* nella barra di "SQL sui risultati", nel menu della
griglia e nella palette): righe, colonna, valore e aggregato (count, count distinct, sum, avg,
min, max), totali e ordinamento; `analysis/pivot.ts` scrive il `PIVOT` di DuckDB, eseguito e
modificabile nel campo SQL. Limiti: al massimo 200 colonne; etichette di colonna (e chiavi di
riga con i totali) come testo, quindi ordinate come testo; un solo totale di riga e di colonna.
**Query tra database** (*Query across databases…* della tabella o della palette,
`commands/analysis/across.rs`): tabelle di connessioni diverse, con filtro WHERE opzionale,
copiate in un DuckDB in memoria (connessione temporanea come "SQL sui risultati") su cui gira un
normale tab; *Reload sources* le rilegge. Limiti: copie, non federazione; 512 MB in tutto (una
tabella oltre il limite è tagliata e segnata); niente MongoDB e Redis. **Risultati salvati**
(*Save result…* della griglia, pannello *Saved results* sotto Scripts, `src-tauri/src/saved_results.rs`):
il risultato intero (fino al limite di memoria, altrimenti segnato parziale) in
`saved-results/<id>.parquet` con un indice JSON; si apre senza connessione in un DuckDB in
memoria, con griglia, grafici e statistiche. Limiti: intervalli salvati come testo; non per
progetto; serve il driver DuckDB. **Dashboard** (*New dashboard* / *Dashboards…* della
connessione, tab `dashboard`, file in `dashboards/<progetto>/<nome>.json`): widget da script
salvati (tabella, grafico o valore singolo, larghezza 1–3, altezza 1–2), eseguiti in parallelo
ognuno sulla sua sessione, solo se lo script è in sola lettura e senza parametri; Refresh per
widget e di tutti, aggiornamento automatico (1–60 min) mentre il tab è attivo. Limiti: una
sessione per widget; si mostra il risultato dell'ultimo statement; niente trascinamento.
**Profilo della tabella** (*Profile data…* di tabelle e viste, tab `table-profile`,
`platypus-local` `table_profile.rs` e `patterns.rs`): le prime 10k/100k/1M righe o tutta la
tabella (con il limite di memoria), per colonna NULL, stringhe vuote, distinti, min/max,
media, deviazione e quartili, lunghezze, istogramma, valori frequenti, formati del testo
(email, URL, UUID, date, numeri come testo, telefono, spazi, maiuscole) con gli esempi che non
li rispettano e outlier con la regola IQR; un clic apre i dati filtrati. Limiti: niente
campionamento casuale né annullamento; outlier solo per colonne numeriche. Gli scenari E2E di
queste P3 (`analysis-pivot`, `-across`, `-saved-results`, `-dashboard`, `-table-profile`) sono
scritti ma non ancora eseguiti (runner Windows).

**Area:** `apps/desktop/src/analysis/`, crate `platypus-local` (DuckDB in-process)
**Dipende da:** 0.4; DuckDB del gruppo G

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ Grafici dai risultati (barre, linee, dispersione, torta), salvabili | P2 | D |
| ✅ Statistiche della colonna al passaggio del mouse: % NULL, distinti, min/max, mini istogramma | P2 | N |
| ✅ **SQL sui risultati**: interrogare il result set corrente con DuckDB | P2 | N |
| ✅ Join tra database diversi tramite DuckDB (tabella PG con tabella MySQL) | P3 | D+ |
| ✅ Diff tra due result set (stessa query prima e dopo) | P2 | N |
| ✅ Snapshot dei risultati salvati e consultabili offline | P3 | N |
| ✅ Pivot dei risultati | P3 | N |
| ✅ **Notebook**: celle SQL e Markdown, grafici e piano per cella, parametri comuni, export HTML. Una cella legge i risultati già caricati delle altre dal DuckDB dell'SQL sui risultati (`c1`, `c2`, …), senza rieseguirle sul server | P2 | N |
| ✅ Dashboard personalizzate dalle query salvate | P3 | D+ |
| ✅ 🆕 Profilo di una tabella intera: per colonna distribuzione, distinti, pattern (email, date, numeri come testo), outlier | P3 | N |

---

### Gruppo N — Accesso per agenti esterni (MCP)

**Stato: P1 e P2 completate, P3 quasi** (resta il mascheramento delle colonne sensibili del
gruppo Q nei tool su script e cronologia, che aspetta quelle colonne). Il crate `platypus-mcp` ha la libreria degli strumenti (`Agent`) e
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
output JSON documentato nel README. Gli agenti avviano l'app stessa con `--mcp` (gruppo P):
gli installer non hanno un secondo eseguibile. P2 in parte: tool `sample_rows` ed `explain`
(albero del piano o JSON nel limite di byte, stesso controllo di `run_query`, `analyze` in sola
lettura solo dove la transazione rifiuta le scritture); registro di ogni chiamata ai tool in
`agent-calls.db` (sessione, agente, connessione, argomenti, durata, esito), con le pagine
Setup, Attività e Sicurezza in Impostazioni → Agents: Setup elenca Claude Desktop, Claude
Code, Codex, Cursor, Windsurf, Antigravity e Gemini CLI con il loro file e lo stato, e
Installa scrive la voce dopo conferma e copia di backup (JSON e TOML con commenti
conservati); Attività filtra per sessione, esito e testo ed esporta CSV; `propose_sql` salva
la proposta e l'app aperta (o al prossimo avvio) la apre in un tab dell'editor. Ponte con
l'app aperta (`apps/desktop/src-tauri/src/agent/`): server hyper su `127.0.0.1` (porta 47337,
impostazione `agents.bridge_port`; `agents.bridge` lo spegne) con token persistente
(`agent-token`); mentre gira porta e token stanno in `agent-bridge.json`, letto a ogni
chiamata dal server `--mcp` e dalla CLI (`platypus_mcp::HttpLink`, trait `AppLink`). Rotte
`/v1/sessions`, `/v1/confirm`, `/v1/hello` e `/mcp` (rmcp streamable HTTP, feature `http` di
`platypus-mcp`, gli stessi tool). Tool `list_open_sessions`; il server saluta l'app ogni 30 s e
la barra in alto mostra gli agenti collegati (dimenticati dopo 90 s). Conferma delle scritture:
con `agents.confirm_writes` (Impostazioni → Safety, attiva di default) ogni modifica su una
connessione read-write (`access::changes`) aspetta la risposta nell'app, con statement, righe
stimate e piano da `Connection::explain`; Rifiuta (con un motivo che l'agente legge), Consenti
una volta, Consenti per questa sessione (per sessione del server e connessione); 5 minuti
senza risposta o app chiusa = rifiutata. Una query che scrive tramite una funzione passa
dalla transazione di sola lettura dove il database la rifiuta, e al rifiuto chiede. Gli E2E danno a ogni scenario una porta libera e
`confirm_writes = false` se lo scenario non li imposta.

P3: lo schema come risorse MCP (`platypus-mcp/src/resource.rs`): una risorsa per connessione
(`platypus://<connessione>`, i suoi schemi) e i template `platypus://<c>/<schema>` (oggetti, fino
a 2000) e `platypus://<c>/<schema>/<tabella>` (come `describe_table`), nomi percent-encoded, ogni
lettura nel registro come `read_resource`. Tool di sola lettura `list_scripts`, `read_script`
(256 KB) e `search_history` (50, fino a 500): la lettura degli script è passata in
`platypus-store/src/scripts.rs`, usata anche dall'app; gli script legati a una connessione senza
accesso per gli agenti e la sua cronologia non si vedono, gli errori passano dal `Redactor`. Il
mascheramento delle colonne sensibili del gruppo Q non c'è ancora: i punti dove andrà sono
segnati nel codice. `get_user_context` chiede all'app aperta (`/v1/context`, coda in comune con
le conferme delle scritture) la scheda attiva: un dialog mostra cosa verrebbe condiviso (testo,
selezione, risultato, dimensioni) con Rifiuta, Condividi una volta, Condividi per questa
sessione (ricordato nella pagina, per sessione del server); il server taglia testo e selezione a
64 KB e il risultato a 100 righe e 32 KB, e rifiuta il contenuto di una scheda su una
connessione senza accesso per gli agenti (dopo il consenso, perché l'app rivela la connessione
solo allora). Plugin pronti: `plugins/platypus-db` (server MCP, `PLATYPUS_DB` per il percorso
dell'app, e una skill con le istruzioni d'uso) nel marketplace `.claude-plugin/marketplace.json`
della repository, e `plugins/codex` con la voce di `config.toml` e le stesse istruzioni come
`AGENTS.md`. E2E `agent-context` (non provato: gli E2E girano solo su Windows).

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
| ✅ `explain` e campione di righe (`sample_rows`) come tool dedicati, con i limiti di `run_query` | P2 | D+ |
| ✅ **Ponte con l'app aperta** (MCP su HTTP locale con token): l'agente vede le sessioni aperte e l'indicatore "agente collegato" compare nella barra | P2 | N |
| ✅ Conferma nell'app per le scritture: dialog con lo statement, le righe stimate e il piano EXPLAIN (riusa le guardie del gruppo L), con "consenti per questa sessione". Il server `--mcp` non ha finestra: serve il ponte con l'app aperta, o una finestra di conferma propria | P2 | N |
| ✅ **Configurazione degli agenti in un clic**: la pagina "Agents" scrive la voce del server nei file di Claude Desktop, Cursor, Windsurf e Codex (o lancia `claude mcp add`) e mostra in quali client è già presente | P2 | N |
| ✅ **Registro delle chiamate degli agenti**: ogni chiamata a un tool (non solo le query), con agente, connessione, argomenti, durata ed esito, raggruppata per sessione, con filtro ed export CSV | P2 | N |
| ✅ Tool `propose_sql`: apre la query proposta dall'agente in un tab dell'editor, da rivedere ed eseguire a mano | P2 | N |
| ✅ Contesto dell'utente su consenso: testo dell'editor, selezione, risultato corrente della griglia | P3 | N |
| ✅ Schema come risorse MCP (`platypus://<connessione>/<schema>/<tabella>`) per i client che le supportano | P3 | D+ |
| ✅ Plugin pronti: plugin per Claude Code e configurazione per Codex con le istruzioni d'uso (quando usare quale tool, limiti) | P3 | N |
| ✅ **Tab Agenti come pannello di integrazione**: sotto-schede Setup, Attività e Sicurezza; in Setup l'elenco dei client (Claude Desktop, Claude Code, Codex, Cursor, Windsurf, Antigravity, …) con il percorso del loro file di configurazione e un pulsante **Installa** che scrive la voce del server (con backup del file e conferma), accanto la configurazione da copiare a mano per il client scelto; Attività mostra le chiamate degli agenti, Sicurezza i permessi per connessione | P2 | N |
| 🆕 Tool di sola lettura su script salvati e cronologia del progetto, con le colonne sensibili del gruppo Q escluse o mascherate (fatti i tool `list_scripts`, `read_script`, `search_history`; manca il mascheramento, che aspetta le colonne sensibili del gruppo Q) | P3 | N |

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

**Stato: P1 in parte, P2 in parte, P3 in parte.** Fatti: un solo eseguibile anche per gli agenti
(`platypus-db --mcp`, il server MCP è nella libreria `platypus-mcp`; il binario `platypus-mcp`
resta per sviluppo e test), DuckDB scaricato al primo uso (crate `duckdb` in modalità
`loadable-extension`, libreria caricata da `platypus-duckdb-loader`, l'unico crate con codice
`unsafe`), log su file con rotazione, segnalazione dei crash su scelta dell'utente e pacchetto
di diagnostica (Impostazioni › Diagnostics). Dal 28 ottobre 2026: verifica su Linux (build,
avvio, memoria, scroll e controlli della finestra con un window manager; il doppio clic sulla
barra del titolo ora massimizza anche su Linux; numeri in BENCHMARKS.md, misurati senza GPU),
test di scala (`tests/scale.rs` nella conformance e lo scenario E2E `scale-sqlite`: 10.000
tabelle, un milione di righe, celle da 32 MB; il navigator ora disegna solo le righe in vista
e il driver SQL Server legge i valori `max` e i pacchetti in blocco, da 10,5 s a 0,8 s per 48
MB), benchmark con soglie (`scripts/bench.py`, job `bench` della CI) e pulizia di `target`
(`scripts/clean-target.mjs`). Dal 29 ottobre 2026: confronto delle dimensioni dei download
Linux con DBeaver CE e Beekeeper Studio (BENCHMARKS.md, "Side by side", e README); avvio e
memoria degli altri client non ancora misurati, perché nel container di test non si possono
eseguire. Sempre il 29 ottobre: README corto in inglese e in italiano (`README.it.md`), con il
dettaglio spostato in `docs/` (FEATURES, AGENTS, SHORTCUTS, DEVELOPMENT), e quattro GIF (un
milione di righe, editor, modifica dei dati, explain) registrate dalla build di release con gli
scenari E2E `gifs-*` (`e2e/gif.mjs`: fotogrammi dalla pagina, codificati con `gifenc`). Il sito
non aveva ancora le GIF. Dal 29 ottobre anche: le GIF nel sito (in cima e nelle schede di
Screenshots, con la PNG al loro posto per chi ha chiesto meno animazioni), DataGrip 2026.2.6 e
TablePlus 2.2.0 nel confronto delle dimensioni (BENCHMARKS.md, README e una tabella nella
sezione Performance del sito); il piano di pubblicazione è in RELEASING.md. Il sito (`site/`) è stato ridisegnato: solo scuro,
l'ornitorinco con i database intorno, una finestra dell'app disegnata in HTML dove una query si
scrive da sola, il tour con le GIF, una griglia di funzionalità, i loghi dei database, le
prestazioni e il confronto. Restano la verifica su un desktop Linux con GPU, il test reale
dell'auto-update, la firma Windows, avvio e memoria di DBeaver accanto ai nostri (li misura
`scripts/bench.py other`; dal container cloud i download degli altri client sono bloccati) e la
distribuzione;
la verifica e la firma su macOS sono P3 (servono un Mac e l'Apple Developer Program).
Dal 30 ottobre 2026: manifest per winget, Homebrew, AUR e Flathub in `packaging/`, riempiti
con versione e SHA-256 degli installer da `scripts/packaging.mjs` (test in
`scripts/packaging.test.mjs`); installata da Flatpak o AUR, l'app annuncia la nuova versione
senza installarla (marker `share/platypusdb/package-manager`). Immagine di anteprima social
1280×640 (`docs/brand/social-preview.png`, da `social-preview.html` con
`scripts/social-preview.mjs`) e testi della vetrina (descrizione, topic, Discussions, `good
first issue`) in RELEASING.md. La pubblicazione dei pacchetti e le impostazioni del
repository aspettano che diventi pubblico.

| Attività | Prio |
|---|---|
| Verifica su Linux: build, avvio, memoria, scroll; correzioni di piattaforma (fatta senza GPU, vedi BENCHMARKS.md; resta un desktop con GPU) | P1 |
| Verifica su macOS: build, avvio, memoria, scroll; correzioni di piattaforma (serve un Mac) | P3 |
| ✅ Prima release su GitHub (v0.1.0, poi v0.2.0; v0.2.1 solo Windows, compilata in locale perché i minuti di Actions erano finiti) | P1 |
| Test reale dell'auto-update da una release pubblicata alla successiva | P1 |
| Firma del codice Windows (Authenticode), per l'avviso di SmartScreen: certificato **Certum Open Source Code Signing** (~30-60 €/anno, anche per singoli nell'UE, chiave su smart card o cloud SimplySign, firma con `bundle.windows.signCommand` di Tauri); alternative SignPath Foundation (gratis per l'open source, firma in CI) o Azure Trusted Signing (verificare se disponibile in Italia). La reputazione di SmartScreen cresce con i download anche con la firma. Piano in [RELEASING.md](RELEASING.md) | P2 |
| Firma e notarizzazione macOS (Apple Developer Program, 99 $/anno) | P3 |
| ✅ 🆕 **Piano di pubblicazione** di ogni versione, anche senza CI: build Windows in locale, release bozza con `latest.json`, macOS e Linux aggiunti dal workflow quando ci sono i minuti; passi in [RELEASING.md](RELEASING.md) | P1 |
| Benchmark affiancato con DBeaver, aggiornando `BENCHMARKS.md` | P2 |
| ✅ Benchmark in CI con soglie (avvio, memoria, decode) per bloccare le regressioni | P2 |
| ✅ Crash report opt-in e log su file con rotazione | P2 |
| ✅ **Un solo eseguibile anche per gli agenti**: l'app fa da server MCP con un'opzione (`platypus-db --mcp`) invece di un secondo binario `platypus-mcp` che ricontiene tutti i driver (circa 50-60 MB in meno nell'installer) | P2 |
| ✅ **DuckDB scaricato al primo uso** invece che incluso nell'installer (circa 35 MB): chi non apre file CSV, Parquet o DuckDB non lo scarica mai | P2 |
| ✅ Build di sviluppo più leggere: meno informazioni di debug per le dipendenze in `Cargo.toml` e pulizia periodica della cartella `target` | P3 |
| Distribuzione: winget, Homebrew cask, Flatpak, AUR (manifest pronti in `packaging/`, riempiti da `scripts/packaging.mjs`; resta la pubblicazione quando il repository è pubblico, passi in [RELEASING.md](RELEASING.md)) | P3 |
| Sito e documentazione utente | P3 |
| ✅ 🆕 Test di scala: schemi con 10.000 tabelle, tabelle da milioni di righe, celle da decine di MB (navigator, autocomplete, griglia, export) | P2 |
| ✅ 🆕 Pacchetto di diagnostica per le segnalazioni: log, versione, impostazioni senza segreti, in un file zip | P3 |
| 🆕 **Vetrina del repository**: descrizione e topic su GitHub (`sql-client`, `database`, `postgresql`, `rust`, `tauri`, `mcp`, …), immagine di anteprima social (1280×640), Discussions attive, issue `good first issue` per chi vuole contribuire (pronti l'immagine `docs/brand/social-preview.png` e i testi in [RELEASING.md](RELEASING.md), "Repository showcase"; restano le impostazioni su GitHub, da fare quando il repository è pubblico) | P2 |
| ✅ 🆕 **Video o GIF di 20-30 secondi** in cima al README: avvio, scroll di milioni di righe, explain con la heatmap | P2 |
| ✅ 🆕 Le stesse GIF nel sito (`site/`) | P2 |
| 🆕 **Confronto con DBeaver, DataGrip, TablePlus e Beekeeper Studio** nel README e nel sito (dimensione, avvio, memoria, licenza, MCP), con i numeri di `BENCHMARKS.md` aggiornati | P2 |
| 🆕 Lancio quando il repository diventa pubblico: Show HN, r/programming, r/rust, r/PostgreSQL, Product Hunt | P3 |

---

### Gruppo Q — Documentazione e catalogo dei dati

**Stato: completato.** P3: **colonne sensibili**, un segno a mano per colonna (*Sensitive* nelle
Columns del tab oggetto, tabella `sensitive_columns` di `catalog.db`) sopra le regole su nome e
tipo delle impostazioni Safety (`guards/maskRules.ts`, `guards/mask.ts`: `useSensitive`, con la
tabella del risultato da `SensitiveScope` dove è nota, data viewer e profilo tabella); le usano
il mascheramento, le intestazioni della griglia e gli export (`ExportOptions.masked`/`omitted`
in `platypus-transfer`: valori scritti come `••••••`, NULL lasciati NULL, o colonne tolte);
gli agenti del gruppo N le useranno col mascheramento delle colonne sensibili di N. **Dove si usa
una colonna**: la sezione *Used by* del tab di una tabella (`column_usage` in
`commands/catalog.rs`): il DDL di viste, routine e trigger dello schema letto in blocco con
`Connection::objects_ddl`, più gli script salvati del progetto sulla stessa connessione o su
nessuna; il confronto dei nomi è in `platypus-sql::usage` (parole e identificatori quotati fuori
da commenti e stringhe, il corpo `$$` delle routine PostgreSQL riletto come SQL), e conta solo
un testo che nomina anche la tabella. P2: **Note e tag** in `catalog.db`, un SQLite a parte nella
cartella dati (`CatalogStore` in `platypus-store`, migrazioni con `PRAGMA user_version`), per
connessione salvata, database e `schema.oggetto.colonna`: riquadro Notes e colonne Note e Tags nel
tab oggetto, segno e tooltip nel navigator con il filtro `#tag`, segno sulle intestazioni di
colonna del data viewer, note e tag nel riquadro informativo dell'autocomplete; seguono le
rinomine fatte dall'app e si cancellano con la connessione. **Commenti del database** modificabili
dal tab oggetto, con l'anteprima dello SQL in `SqlActionDialog`: lo SQL lo scrive
`platypus-schema` (un ramo per dialetto in `Ddl::table_comment` e `Ddl::comments`) e
`Connection::object_comment` legge quello della tabella; SQLite, libSQL e MongoDB non hanno
commenti e il campo resta in sola lettura. **Documentazione dello schema** dal menu del navigator
(*Export documentation…*, `DocsDialog`) in Markdown o HTML, con il crate puro `platypus-docs`:
indice, colonne, chiavi, indici, commenti, note e tag, e un diagramma ER Mermaid per schema;
l'HTML è un file solo con Mermaid 11.4.1 incorporato e funziona offline. Fatta anche la **colonna
di descrizione delle foreign key**, scelta per tabella puntata (dal menu dell'intestazione della
griglia o da Row label nel tab oggetto) e salvata in `catalog.db`; *Automatic* è la regola di
prima. Limiti: le note si perdono se un oggetto si rinomina fuori dall'app (ricompaiono se torna
con lo stesso nome); il diagramma di uno schema con più di 100 tabelle si omette, con un avviso
nel documento; il commento si modifica sulle tabelle, mentre quello di viste e viste materializzate
si vede in sola lettura (ogni database lo scrive in modo diverso, MySQL non lo ha).

Sapere cosa contiene un database senza chiederlo a chi l'ha scritto: note e tag sugli oggetti,
commenti del database, documentazione esportabile, colonne sensibili segnate una volta e
rispettate da tutta l'app.

**Area:** `apps/desktop/src/catalog/`, `commands/catalog.rs`; note e tag nello store del progetto
(`platypus-store`, con una migrazione del formato)
**Dipende da:** il tab oggetto del gruppo C, 0.5, 0.6

| Funzionalità | Prio | Origine |
|---|---|---|
| ✅ **Note e tag locali** su tabelle e colonne (non scritti nel database), mostrati nel navigator, nell'intestazione della griglia e nell'autocomplete | P2 | N |
| ✅ Modifica dei commenti del database (`COMMENT ON`, `COMMENT` di MySQL, proprietà estese di SQL Server) dal tab oggetto | P2 | D |
| ✅ **Documentazione dello schema** esportata in HTML o Markdown: tabelle, colonne, chiavi, commenti, note e diagramma ER | P2 | N |
| ✅ **Colonne sensibili** (PII, segreti): regole su nome e tipo più scelte a mano, usate dal mascheramento del gruppo L, dagli agenti del gruppo N e dagli export | P3 | N |
| ✅ Dove si usa una colonna: viste, funzioni, trigger e script salvati che la nominano | P3 | N |

---

### Gruppo R — Revisione grafica

**Stato: in attesa.** Si prende solo quando tutti gli altri gruppi sono `✅ completato`: rivede
l'aspetto dell'app finita, non quello di funzionalità ancora in movimento.

Una passata unica su tutta l'interfaccia perché sembri un prodotto solo, fatto con cura: oggi ogni
gruppo ha aggiunto le sue schermate con lo stile del momento. Si parte dall'inventario di tutte le
schermate (screenshot di ognuna, in tema chiaro e scuro, in italiano e in inglese) e si correggono
le differenze, senza cambiare il comportamento. Restano le scelte di fondo già prese: colori
attuali, rail dei progetti in stile Discord, tab in alto, interfaccia minimale che non sembri
generata.

**Area:** `apps/desktop/src/styles/`, i componenti di tutte le aree (solo CSS, markup e testi),
`apps/desktop/src/assets/`; nessuna modifica a backend, driver o formati dei file
**Dipende da:** tutti gli altri gruppi completati

| Funzionalità | Prio | Origine |
|---|---|---|
| 🆕 **Inventario visivo**: una scheda per schermata e dialog con screenshot (chiaro/scuro, it/en) e i problemi trovati | P1 | N |
| 🆕 **Tutto a portata di mano e comprensibile**: ogni pagina studiata perché l'utente capisca da sola cosa può fare lì; le azioni principali visibili sulla pagina stessa (pulsanti, intestazioni, stati vuoti con l'azione utile), non solo nei menu contestuali o nelle scorciatoie; dalla pagina di un oggetto si arriva a tutto ciò che lo riguarda (dati, struttura, editor grafico, DDL, diagramma). Esempio: l'editor grafico della tabella oggi si apre solo dal menu del navigator o con F4 | P1 | N |
| 🆕 **Design token unici**: spaziature, raggi, ombre, dimensioni dei font e dei controlli da una scala sola; via i valori scritti a mano nei singoli componenti | P1 | N |
| 🆕 Dialog, menu, tooltip, notifiche e pannelli con la stessa struttura (titolo, corpo, pulsanti, focus, Esc/Invio) | P1 | N |
| 🆕 Stati vuoti, di caricamento e di errore uguali in tutta l'app, con testo e azione utile | P1 | N |
| 🆕 **Finestra di connessione riordinata**: oggi i controlli sono messi senza un ordine (Tipo e Colore accanto al Nome, "Connetti con" isolato, Sola lettura e Accesso degli agenti in fondo al tab Generale). Raggrupparli in sezioni con titolo: *Connessione* (Host/URL, host, porta, database), *Autenticazione* (utente, provenienza della password, password), *Aspetto e sicurezza* (tipo di ambiente, colore, sola lettura, accesso degli agenti); allineamento su una griglia sola | P1 | N |
| 🆕 Griglia, editor e navigator allineati: altezze di riga, icone, colori di selezione e di modifica | P1 | N |
| 🆕 Contrasto e accessibilità di tutti i temi (WCAG AA), focus visibile, uso da sola tastiera | P1 | N |
| 🆕 Testi rivisti in italiano e inglese: tono, lunghezza, etichette tagliate nelle finestre piccole | P2 | N |
| 🆕 Set di icone coerente (una sola famiglia, stesse dimensioni e tratto) e icone dei driver | P2 | N |
| 🆕 Nel navigator l'icona del driver di ogni connessione al posto del pallino colorato; il colore della connessione e lo stato (connessa, errore) restano come piccolo segno sull'icona | P2 | N |
| 🆕 Impostazioni come pagine con intestazione (icona, titolo, sottotitolo, descrizione) e sotto-schede, sul modello del tab Agenti del gruppo N: liste di schede con azione a destra, pannello di dettaglio a fianco | P2 | N |
| 🆕 Posizione degli elementi personalizzabile e set di icone a scelta | P3 | N |
| 🆕 Micro-animazioni misurate (apertura pannelli, transizioni dei tab), disattivabili | P3 | N |

---

### Gruppo S — Collaudo finale

**Stato: in attesa.** Si prende dopo il gruppo R, sull'app finita e ridisegnata: è il collaudo
prima della v1.0, fatto con calma e con metodo, non una passata veloce.

Verificare che tutto funzioni su tutti i database e i sistemi, come lo usa una persona vera, e
lasciare ogni controllo come test automatico che resti nel tempo. Si lavora come nella campagna
di test di ottobre 2026: un tester per area con l'app compilata, uno scenario E2E per ogni bug,
poi le correzioni e un secondo giro con occhi nuovi; un agente o una persona alla volta.

**Non in una sessione cloud.** Il collaudo finale (e in generale i test finali prima di un
rilascio) è meglio non farlo in una sessione di un agente in cloud: lì gli E2E non girano (solo
Windows, WebView2), non c'è GPU né macOS, i download dei driver e degli altri client sono
bloccati dalla rete del container, e il disco è un'allowance fissa che i database di test
riempiono. Va fatto su macchine vere: Windows, poi macOS e un desktop Linux.

**Area:** `apps/desktop/e2e/`, `crates/platypus-conformance/`, `docs/BENCHMARKS.md`; le correzioni
nei file delle aree toccate, in commit piccoli e separati
**Dipende da:** gruppo R completato

| Funzionalità | Prio | Origine |
|---|---|---|
| 🆕 **Inventario delle funzionalità** con, per ognuna, lo scenario E2E o il test di conformance che la copre; i buchi diventano test | P1 | N |
| 🆕 **Giro per aree** con un tester ciascuna (connessioni, navigator, editor, griglia e modifica, analisi, import/export, impostazioni e tab, sicurezza e MCP, schema, amministrazione) sui casi normali, i casi limite e gli errori | P1 | N |
| 🆕 **Tutti i database** con tutte le funzioni che li riguardano: PostgreSQL e varianti, MySQL, MariaDB, SQL Server, Oracle, ClickHouse, SQLite, DuckDB, libSQL, MongoDB, Trino, ODBC/ADBC | P1 | N |
| 🆕 **Percorsi completi da utente** (primo avvio, import da DBeaver, giornata di lavoro, analisi, sviluppo, agenti) con riavvio dell'app nel mezzo, in italiano e in inglese, anche da sola tastiera | P1 | N |
| 🆕 **Installer e aggiornamento** da ogni versione pubblicata, con i dati dell'utente (connessioni, password, script, impostazioni) che restano | P1 | N |
| 🆕 **macOS e Linux**: gli scenari E2E anche fuori da Windows, o un collaudo a mano con una lista scritta | P2 | N |
| 🆕 **Prestazioni e memoria**: tabelle da milioni di righe, schemi con migliaia di oggetti, una sessione di ore senza che la memoria cresca, database remoto lento; i numeri in BENCHMARKS.md | P2 | N |
| 🆕 **Revisione di sicurezza** del codice: credenziali, MCP, sola lettura, SQL generato, file letti e scritti | P2 | N |
| 🆕 Secondo giro con tester nuovi che non hanno visto i rapporti del primo, poi la verifica completa (fmt, clippy, test, E2E, conformance) tutta verde prima della v1.0 | P1 | N |

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
| **5** | **R** | Revisione grafica di tutta l'app, a funzionalità finite | v1.0 | In attesa |
| **6** | **S** | Collaudo finale di tutta l'app, dopo la revisione grafica | v1.0 | In attesa |

Per ogni gruppo, la Definition of Done segue quella dell'MVP:
- test di conformance o E2E per ciò che è stato aggiunto;
- clippy e fmt puliti;
- nessuna regressione sui target di [BENCHMARKS.md](BENCHMARKS.md);
- codice rivisto per semplicità e performance: refactor fatti, non rimandati;
- funzionalità documentata nel README o nella documentazione utente.
