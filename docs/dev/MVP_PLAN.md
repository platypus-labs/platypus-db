# PlatypusDB — Piano MVP

> Client SQL desktop multi-database, alternativa leggera a DBeaver.
> Core in Rust, UI in Tauri 2. Obiettivo MVP: release `0.1.0` usabile ogni giorno su PostgreSQL, MySQL/MariaDB e SQLite.

---

## 1. Obiettivi e non-obiettivi

### Obiettivi MVP
1. Connettersi a **PostgreSQL**, **MySQL/MariaDB**, **SQLite**.
2. Gestire connessioni salvate (credenziali nel keychain del sistema operativo).
3. Esplorare lo schema (database → schema → tabelle/viste → colonne/indici) con caricamento lazy.
4. Scrivere ed eseguire SQL in un editor con evidenziazione sintattica, più tab, esecuzione dello statement sotto il cursore, cancellazione della query.
5. Visualizzare risultati enormi (1M+ righe) in una griglia virtualizzata fluida.
6. Aprire una tabella in modalità "data viewer" con ordinamento e filtro lato server.
7. Esportare i risultati in CSV, JSON, Parquet.
8. Storico delle query e controllo transazioni (autocommit / commit / rollback).
9. Autocomplete base (keyword + tabelle + colonne).

### Non-obiettivi (post-MVP)
- Oracle, SQL Server, DB2, Snowflake, ClickHouse, DuckDB (→ v0.2 tramite driver nativi, ADBC e ODBC)
- Tunnel SSH, proxy (→ v0.2, è il primo da fare dopo l'MVP)
- Modifica inline delle celle con generazione di UPDATE (→ v0.2)
- Diagrammi ER, confronto schemi, migrazione dati tra DB
- Editor visuali DDL (create/alter table)
- Sistema di plugin, sync cloud, collaborazione

### Target di performance (misurabili, verificati in CI/benchmark)
| Metrica | Target | DBeaver (riferimento indicativo) |
|---|---|---|
| Avvio a freddo fino a finestra interattiva | < 500 ms | 5–15 s |
| RAM a riposo (1 connessione aperta) | < 150 MB | 500 MB – 1 GB |
| Prime righe visibili dopo la risposta del server | < 100 ms | variabile |
| Scroll su 1M righe | 60 fps, nessun freeze | freeze / OOM |
| Dimensione installer | < 20 MB | ~120 MB |

---

## 2. Stack tecnologico

| Area | Scelta | Motivazione |
|---|---|---|
| Linguaggio core | Rust (stable, edition 2024) | prestazioni, niente GC, async maturo |
| Runtime async | `tokio` | standard de facto, richiesto dai driver |
| PostgreSQL | `tokio-postgres` (+ `postgres-native-tls` / `rustls`) | streaming con `query_raw`, accesso agli OID dei tipi, cancel token |
| MySQL/MariaDB | `mysql_async` | async nativo, streaming delle righe |
| SQLite | `rusqlite` (feature `bundled`) dentro `spawn_blocking` | maturo, `InterruptHandle` per la cancellazione |
| Formato dati interno | `arrow-rs` (`RecordBatch`) | colonnare e compatto; export e IPC verso la UI quasi gratis |
| Export | `arrow-csv`, `arrow-json`, `parquet` | stessa pipeline per tutti i formati |
| Parsing SQL | `sqlparser-rs` | split degli statement, analisi per l'autocomplete |
| Keychain | `keyring` | Windows Credential Manager / macOS Keychain / Secret Service |
| Storage locale | `rusqlite` (storico) + TOML (`serde`, `toml`) per la configurazione | semplice, ispezionabile a mano |
| Errori / log | `thiserror`, `anyhow` (solo nell'app), `tracing` | |
| Shell desktop | Tauri 2 | binario piccolo, webview di sistema, IPC binario |
| Frontend | TypeScript + React + Vite | ecosistema per griglia ed editor |
| Editor SQL | CodeMirror 6 (`@codemirror/lang-sql`) | leggero, estensibile, autocomplete personalizzabile |
| Griglia | Glide Data Grid (canvas) | milioni di righe, rendering su canvas, nessun DOM per cella |
| Arrow lato JS | `apache-arrow` | decodifica zero-copy dell'IPC |
| Stato UI | Zustand | minimale |

**Perché non `sqlx`:** è orientato ai tipi verificati a compile time. Un client SQL lavora con query arbitrarie e tipi scoperti a runtime: `tokio-postgres` e `mysql_async` espongono meglio i metadati delle colonne e lo streaming.

---

## 3. Architettura

```
platypus-db/
├── Cargo.toml                  # workspace
├── crates/
│   ├── platypus-core/          # trait Driver/Connection, tipi metadata, ResultStream, errori
│   ├── platypus-driver-pg/
│   ├── platypus-driver-mysql/
│   ├── platypus-driver-sqlite/
│   ├── platypus-sql/           # split statement per dialetto, completion engine
│   ├── platypus-store/         # config connessioni, keychain, storico (SQLite locale)
│   └── platypus-cli/           # CLI minimale: usata per test e benchmark del core
├── apps/
│   └── desktop/
│       ├── src-tauri/          # comandi Tauri, session manager, bridge IPC
│       └── src/                # React + TS (layout standard Tauri)
├── docker/
│   └── compose.yml             # postgres, mysql, mariadb per i test
└── docs/
```

**Regola fondamentale:** i crate `platypus-*` non dipendono da Tauri. La UI è sostituibile (in futuro gpui/egui) e la CLI copre tutto il core.

### 3.1 Astrazioni core (bozza)

```rust
pub trait Driver: Send + Sync {
    fn id(&self) -> DriverId;                       // "postgres", "mysql", "sqlite"
    fn dialect(&self) -> Dialect;
    async fn connect(&self, cfg: &ConnectionConfig, secret: Option<Secret>)
        -> Result<Box<dyn Connection>>;
}

pub trait Connection: Send + Sync {
    async fn execute(&self, sql: &str, opts: ExecOptions) -> Result<Execution>;
    fn cancel_handle(&self) -> CancelHandle;        // clonabile, usabile da un altro task
    async fn metadata(&self) -> &dyn MetadataProvider;
    async fn set_autocommit(&self, on: bool) -> Result<()>;
    async fn commit(&self) -> Result<()>;
    async fn rollback(&self) -> Result<()>;
    async fn ping(&self) -> Result<()>;
}

pub enum Execution {
    Rows(ResultStream),                             // SELECT & co.
    Affected { rows: u64, elapsed: Duration },      // DML/DDL
}

pub struct ResultStream {
    pub schema: SchemaRef,                          // Arrow schema + metadata sui tipi originali
    // Stream<Item = Result<RecordBatch>>, batch da ~1000 righe (configurabile)
}

pub trait MetadataProvider {
    async fn catalogs(&self) -> Result<Vec<Catalog>>;
    async fn schemas(&self, catalog: &str) -> Result<Vec<Schema>>;
    async fn objects(&self, schema: &SchemaRef) -> Result<Vec<DbObject>>;   // tabelle, viste, ...
    async fn columns(&self, obj: &ObjectRef) -> Result<Vec<Column>>;
    async fn indexes(&self, obj: &ObjectRef) -> Result<Vec<Index>>;
}
```

### 3.2 Flusso di esecuzione di una query

1. La UI invia `run_query(session_id, sql, cursor_pos)`.
2. `platypus-sql` individua lo statement da eseguire secondo il dialetto.
3. Il session manager (in `src-tauri`) crea un `QueryJob` con id, avvia lo stream su un task tokio e restituisce subito `job_id` + schema.
4. I batch vengono accumulati in un **result buffer** lato Rust (Vec<RecordBatch>) con limite di memoria configurabile (default 512 MB; superato il limite, il fetch si ferma e la UI lo segnala).
5. La UI chiede finestre di righe `fetch_rows(job_id, offset, len)`. La risposta è **Arrow IPC binario** tramite `tauri::ipc::Response` (niente JSON).
6. La griglia (canvas) decodifica con `apache-arrow` e disegna solo le celle visibili.
7. `cancel_query(job_id)` usa il `CancelHandle`:
   - PG: `CancelToken::cancel_query`
   - MySQL: `KILL QUERY <id>` su una connessione separata
   - SQLite: `InterruptHandle::interrupt()`

### 3.3 Mappatura dei tipi → Arrow
- Numerici, bool, testo, date/time, timestamp(tz), bytea/blob, UUID → tipi Arrow nativi.
- `numeric`/`decimal` → `Decimal128` se precisione ≤ 38, altrimenti `Utf8`.
- JSON/JSONB, array, enum, range, tipi geometrici, tipi sconosciuti → `Utf8` con il tipo originale salvato nei metadata del campo (la UI lo mostra nell'header).
- SQLite (tipizzazione dinamica): tipo dichiarato della colonna con fallback `Utf8`.

### 3.4 Persistenza locale
- `%APPDATA%/PlatypusDB/connections.toml` (o equivalente per OS): connessioni senza password.
- Password nel keychain dell'OS: `service = "platypusdb"`, `user = <connection-uuid>`.
- `history.db` (SQLite): `(id, connection_id, sql, started_at, duration_ms, rows, status, error)`.
- Script aperti / tab di sessione salvati in `workspace.json` e ripristinati all'avvio.

---

## 4. Funzionalità MVP nel dettaglio

### F1 — Gestione connessioni
- Dialog di creazione/modifica: driver, host, porta, database, utente, password (con "salva password"), opzioni SSL (disable / prefer / require / verify-full, path del certificato CA) per PG e MySQL, path del file per SQLite.
- Pulsante "Test connection" con latenza e versione del server.
- Cartelle/gruppi e colore per connessione (es. rosso = produzione).
- Flag "read-only": blocca gli statement non-SELECT a livello di app (con conferma).
- Import della stringa di connessione (`postgres://…`, `mysql://…`).

### F2 — Navigator
- Albero lazy: connessione → database → schema → {tabelle, viste, viste materializzate, funzioni (solo elenco)} → colonne, indici, chiavi.
- Cache dei metadati per connessione con "Refresh" per nodo.
- Ricerca/filtro nell'albero.
- Menu contestuale: "Apri dati", "Nuovo script", "Copia nome", "Genera SELECT".

### F3 — Editor SQL
- Tab multipli, ognuno legato a una connessione e a un database/schema attivo.
- Evidenziazione sintattica per dialetto, numeri di riga, cerca/sostituisci, commento con Ctrl+/.
- `Ctrl+Enter`: esegue lo statement sotto il cursore. `Alt+X`: esegue lo script (statement in sequenza, stop al primo errore, configurabile).
- Errori con posizione evidenziata nell'editor quando il driver la fornisce (PG `position`).
- Salvataggio/apertura di file `.sql`.

### F4 — Griglia dei risultati
- Virtualizzazione su canvas, colonne ridimensionabili, header con nome + tipo.
- Fetch progressivo: prime 1000 righe subito, poi altre su richiesta durante lo scroll; pulsante "Fetch all".
- Pannello dei valori per celle lunghe (JSON formattato, testo, hex per i binari).
- Selezione rettangolare + copia (TSV, utile da incollare in Excel).
- Barra di stato: righe, tempo di esecuzione, tempo di fetch, memoria del buffer.
- Più result set per l'esecuzione di uno script (tab dei risultati).

### F5 — Data viewer tabella
- Apertura dal navigator: `SELECT * FROM t LIMIT n`, generata con il quoting corretto per il dialetto.
- Click sull'header → `ORDER BY` lato server. Barra filtro → clausola `WHERE` libera.
- Conteggio righe su richiesta (`COUNT(*)` esplicito, mai automatico).

### F6 — Export
- CSV (separatore, quoting, header), JSON (array / NDJSON), Parquet.
- Export del result set corrente oppure riesecuzione in streaming verso file (senza tenere tutto in RAM).
- Progresso e annullamento.

### F7 — Transazioni e storico
- Toggle autocommit per tab; in modalità manuale pulsanti Commit/Rollback e indicatore "transazione aperta".
- Avviso alla chiusura del tab con una transazione aperta.
- Storico ricercabile per connessione, con doppio click per riaprire la query nell'editor.

### F8 — Autocomplete base
- Keyword del dialetto.
- Nomi di schema, tabelle e viste dalla cache dei metadati.
- Colonne dopo `alias.` risolvendo gli alias nella clausola `FROM` (con `sqlparser-rs`, con fallback tollerante per SQL incompleto).

---

## 5. Milestone

Stime per **1 sviluppatore full-time**. Part-time: moltiplicare per circa 2–2,5.

### M0 — Fondamenta (1 settimana)
- Workspace Cargo, crate vuoti, `rustfmt`/`clippy` con lint severi.
- Scaffold Tauri 2 + React + Vite; finestra vuota funzionante su Windows.
- `docker/compose.yml` con Postgres 16, MySQL 8, MariaDB 11 e dati di esempio (es. Pagila / Sakila).
- CI GitHub Actions: fmt, clippy, test, build Tauri su Windows/macOS/Linux.
- **Fatto quando:** la CI è verde e `cargo tauri dev` apre la finestra.

### M1 — Core + driver PostgreSQL (2 settimane)
- Trait di `platypus-core`, mappatura tipi PG → Arrow, streaming a batch, cancellazione.
- `platypus-cli`: `platypus query --url postgres://... "select ..."` stampa una tabella o esporta in CSV.
- Test di integrazione su tutti i tipi PG principali (con `testcontainers` o docker compose).
- Benchmark `criterion`: righe/s di conversione verso Arrow.
- **Fatto quando:** la CLI fa streaming di 10M righe con memoria costante e il cancel funziona.

### M2 — Shell UI, connessioni, griglia (2 settimane)
- `platypus-store`: config TOML + keychain.
- Comandi Tauri: `list_connections`, `save_connection`, `test_connection`, `open_session`, `run_query`, `fetch_rows`, `cancel_query`.
- Layout UI: sidebar, area tab, pannello dei risultati ridimensionabile.
- Glide Data Grid alimentata da Arrow IPC.
- **Fatto quando:** salvo una connessione PG, eseguo una query da una textarea e scorro 1M righe a 60 fps.

### M3 — Editor SQL (2 settimane)
- CodeMirror 6 con dialetti, tab multipli, workspace persistente.
- `platypus-sql`: split degli statement (stringhe, commenti, `$$` di PG, `DELIMITER` di MySQL), statement sotto il cursore.
- Esecuzione di script con più result set; evidenziazione degli errori.
- Storico delle query.
- **Fatto quando:** posso lavorare una giornata su PG usando solo PlatypusDB.

### M4 — Navigator + MySQL + SQLite (2 settimane)
- `MetadataProvider` per PG (`pg_catalog`), MySQL (`information_schema`), SQLite (`sqlite_master` + `pragma_*`).
- Driver MySQL/MariaDB e SQLite, con la stessa suite di test di integrazione parametrizzata per driver.
- Albero del navigator con lazy loading e cache.
- **Fatto quando:** i tre DB sono navigabili e interrogabili e la stessa suite di test passa su tutti e tre.

### M5 — Data viewer, export, transazioni (1,5 settimane)
- F5, F6, F7.
- **Fatto quando:** apro una tabella, filtro/ordino, esporto 5M righe in Parquet in streaming, uso commit/rollback.

### M6 — Autocomplete, rifinitura, release (1,5 settimane)
- F8.
- Tema chiaro/scuro, scorciatoie da tastiera, gestione errori di rete (riconnessione).
- Benchmark finali rispetto ai target della §1, confronto documentato con DBeaver.
- Installer firmati (MSI/NSIS su Windows, DMG su macOS, AppImage/deb su Linux), auto-update di Tauri.
- **Fatto quando:** tutti i criteri della §7 sono soddisfatti e la release `v0.1.0` è pubblicata.

**Totale stimato: circa 12 settimane full-time.**

---

## 6. Rischi e mitigazioni

| Rischio | Impatto | Mitigazione |
|---|---|---|
| WebKitGTK su Linux lento o con bug | griglia poco fluida su Linux | la griglia su canvas riduce il carico del DOM; test precoce in M2; piano B: renderer nativo per la sola griglia |
| Costo dell'IPC per finestre di righe grandi | lag durante lo scroll | Arrow IPC binario, finestre di righe con prefetch, benchmark in M2 prima di proseguire |
| Edge case nella mappatura dei tipi | crash o dati errati | fallback `Utf8` sempre disponibile, test per ogni tipo, fuzzing della conversione |
| Cancellazione MySQL (serve una seconda connessione) | query non annullabili | connessione "di controllo" lazy per sessione |
| SQL incompleto non parsabile per l'autocomplete | autocomplete inutile | tokenizer tollerante e euristiche sul contesto; il parser completo solo quando l'SQL è valido |
| Scope creep ("manca X di DBeaver") | MVP che non esce mai | elenco dei non-obiettivi in §1, backlog v0.2 separato |
| Firma del codice su Windows/macOS | avvisi di sicurezza all'installazione | gestire i certificati già in M4, non all'ultimo |

---

## 7. Criteri di accettazione MVP (Definition of Done)
- [ ] I target di performance della §1 sono misurati e rispettati su Windows e macOS.
- [ ] PG 13–17, MySQL 8, MariaDB 10.11+, SQLite 3: connessione, navigazione, query, export.
- [ ] Nessuna password in chiaro su disco o nei log.
- [ ] Le query lunghe sono annullabili e la UI non si blocca mai (nessuna operazione di I/O sul thread UI).
- [ ] La suite di test di integrazione è verde sui tre DB in CI.
- [ ] Installer per i tre sistemi operativi, con auto-update funzionante.
- [ ] README con screenshot, guida rapida e benchmark.

---

## 8. Backlog v0.2 (priorità indicativa)
1. Tunnel SSH (`russh`).
2. Modifica inline delle celle con generazione di UPDATE/INSERT/DELETE (solo tabelle con PK) e anteprima dell'SQL.
3. SQL Server (`tiberius`), DuckDB, ClickHouse.
4. ADBC + ODBC come driver generici (Snowflake, Oracle, ecc.).
5. Visualizzazione del DDL degli oggetti, explain plan grafico (PG `EXPLAIN (FORMAT JSON)`).
6. Formatter SQL.
7. Diagramma ER in sola lettura.
