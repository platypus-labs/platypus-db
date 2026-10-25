# Driver ADBC e ODBC generici

**Data:** 25 ottobre 2026
**Stato:** design approvato in chat, da rivedere per iscritto
**Mockup:** `.superpowers/brainstorm/` (locale, non versionato): `driver-popup.html` (approvato)
**Gruppo:** G — Nuovi driver, riga roadmap "ADBC e ODBC generici (Snowflake, Oracle, Db2,
Databricks, BigQuery, Trino)" (P2). Branch `group-g-adbc-odbc`.

## Obiettivo

Collegarsi a Snowflake, BigQuery, Databricks, Trino, Oracle e Db2, e a qualsiasi database
che abbia un driver ADBC o ODBC, senza scrivere un driver nativo per ognuno. L'installer
resta leggero: i driver si scaricano quando servono, dall'app, dopo una domanda.

**Successo:**
- Scegliendo Snowflake (o un altro dei sei) e premendo Test o connettendosi, se il driver
  manca compare un popup: con un click il driver viene scaricato, verificato e usato.
- Query, griglia, export, cronologia, navigator e autocompletamento funzionano su questi
  database come sugli altri.
- Trino passa la conformance attraverso il ponte ADBC; l'ODBC generico la passa su
  PostgreSQL via psqlODBC.
- Un driver qualsiasi (file ADBC, driver o DSN ODBC) si usa con le voci generiche.

**Fuori ambito (prima versione):** explain, modifica dei dati nella griglia, editor di
tabelle per i nuovi driver (capability spente, accendibili per profilo dopo); tunnel SSH per
i nuovi driver (i driver aprono le loro connessioni: la capability non c'è); driver ADBC con
licenza non libera (`LicenseRef-PBL`, es. Redshift e SQL Server di Columnar).

## Fasi

1. **Fase 1 — ADBC:** modello (`DriverKind`, `options`, file v6), crate `platypus-drivers`,
   ponte ADBC, profili Snowflake, BigQuery, Databricks, Trino, ADBC generico, popup del
   driver, Settings › Drivers. Unita in `main` da sola.
2. **Fase 2 — ODBC:** ponte ODBC, profili Oracle e Db2, ODBC generico, installer dei vendor.

## Modello (`platypus-core`, `platypus-store`)

`DriverKind` riceve `Snowflake`, `BigQuery`, `Databricks`, `Trino`, `Oracle`, `Db2`, `Adbc`,
`Odbc`. Nuovi metodi:
- `bridge() -> Option<Bridge>`: `Adbc` per Snowflake, BigQuery, Databricks, Trino, Adbc;
  `Odbc` per Oracle, Db2, Odbc; `None` per i driver nativi.
- `default_port()`: Trino 8080, Oracle 1521, Db2 50000; gli altri `None`.
- `is_embedded()` invariato (falso per tutti i nuovi).

`ConnectionConfig` e `SavedConnection` guadagnano `options: BTreeMap<String, String>`
(`#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`): i campi propri di un
profilo. Password e token restano nel `SecretStore` (keychain o vault) come oggi.

`connections.toml`: `FILE_VERSION` 5 → 6 (un'app vecchia non deve aprire connessioni di
driver che non conosce né perdere `options`). I file v5 si leggono senza cambiamenti.

**Campi per profilo** (chiavi di `options`; `host`, `port`, `database`, `user`, password come
oggi):

| Driver | `host` | `database` | `options` |
|---|---|---|---|
| Snowflake | account (`xy12345.eu-west-1`) | database | `warehouse`, `role`, `schema` |
| BigQuery | — | — | `project`, `dataset`, `credentials_file` (vuoto: Application Default Credentials) |
| Databricks | workspace host | catalog | `http_path`, `schema`; token = password |
| Trino | host | catalog | `schema` |
| Oracle | host | service name | — |
| Db2 | host | database | — |
| Adbc | — | — | `driver_file`, `uri`, più chiavi libere passate tali e quali al driver |
| Odbc | — | — | `dsn` oppure `connection_string`; `dialect` |

Il dialog di connessione mostra i campi dal descrittore del profilo nel frontend
(`apps/desktop/src/drivers/profiles.ts`), con etichette e segnaposto.

## Driver su disco (`platypus-drivers`, nuovo crate)

**ADBC** — registro `dbc` di Columnar, `https://dbc-cdn.columnar.tech/index.yaml`:
- richieste con user agent `platypus-db/<versione>` (chiesto dal registro);
- `index.yaml` in cache in `<dati>/drivers/index.yaml`, aggiornato se più vecchio di 24 ore,
  usato dalla cache se la rete manca;
- per un driver (`snowflake`, `bigquery`, `databricks`, `trino`): la versione più recente con
  un pacchetto per la piattaforma (`windows_amd64`, `macos_arm64`, …), la sua licenza;
  rifiutati i driver con licenza `LicenseRef-*`;
- download del `.tar.gz` con avanzamento e annullamento, verifica dello sha256 dell'indice
  quando c'è (mismatch = errore e file cancellato), estrazione in
  `<dati>/drivers/adbc/<nome>/<versione>/`, poi un `driver.toml` locale con nome, versione,
  licenza, file della libreria; scrittura atomica (cartella temporanea e rename);
- elenco, aggiornamento (nuova versione disponibile) e rimozione dei driver scaricati;
- limite di dimensione sui file estratti (come per l'import `.dbp`).

**ODBC (fase 2)**:
- elenco dei driver installati (`SQLDrivers`) e dei DSN (`SQLDataSources`);
- per Oracle e Db2 gli URL degli zip pubblici dei vendor (Instant Client Basic + ODBC;
  IBM clidriver), download, estrazione in `<dati>/drivers/odbc/<nome>/`, registrazione con
  `odbc_install.exe` / `db2cli install -setup` avviata con elevazione (UAC); se fallisce o
  viene rifiutata, errore con il link alla pagina del vendor. Licenza del vendor mostrata
  con link prima del download.

## Ponte ADBC (`platypus-driver-adbc`, fase 1)

- `adbc_core` con il driver manager; carica la libreria dal `driver.toml`.
- Un thread per connessione: le chiamate ADBC sono bloccanti; il `Connection` asincrono gli
  manda comandi su un canale.
- Traduzione profilo → opzioni ADBC in una funzione pura per driver
  (`fn options(config) -> Vec<(String, OptionValue)>`), con test. Le chiavi esatte di ogni
  driver si verificano sulla sua documentazione durante l'implementazione (es. Snowflake
  `adbc.snowflake.sql.account`, `adbc.snowflake.sql.warehouse`, `adbc.snowflake.sql.role`;
  BigQuery `adbc.bigquery.sql.project_id`, `adbc.bigquery.sql.dataset_id`).
- `execute`: `Statement::set_sql_query` + `execute`; il `RecordBatchReader` diventa lo stream
  di `RecordBatch`. Statement senza risultati: righe toccate se il driver le dà.
- `cancel_handle`: `Statement::cancel`; uno stream eliminato interrompe la lettura.
- Transazioni: opzione `adbc.connection.autocommit`; `commit`/`rollback` se il driver li
  supporta, altrimenti autocommit fisso e la UI non offre il commit manuale.
- Metadati: `get_objects` (cataloghi › schemi › tabelle › colonne; chiavi se il driver le
  dà) per `schemas`, `objects`, `table_details`, `catalog_columns`. Un profilo può usare query
  sue (`INFORMATION_SCHEMA` su Snowflake, BigQuery, Trino) dove `get_objects` è lento o povero.
- `server_info`: `get_info` (nome e versione del vendor e del driver).
- Errori ADBC → `platypus_core::Error` con lo SQLSTATE quando c'è.

## Ponte ODBC (`platypus-driver-odbc`, fase 2)

- `odbc-api` + `arrow-odbc`; un thread per connessione.
- Connection string dal profilo (`DRIVER={…};DBQ=host:1521/service;UID=…;PWD=…` per Oracle,
  `DRIVER={IBM DB2 ODBC DRIVER};DATABASE=…;HOSTNAME=…;PORT=…;PROTOCOL=TCPIP;UID=…;PWD=…`
  per Db2), il DSN, o quella scritta dall'utente.
- Risultati letti a blocchi in Arrow (`arrow-odbc`); cancellazione con `SQLCancel`.
- Autocommit e commit/rollback con l'API ODBC.
- Metadati: `SQLTables`, `SQLColumns`, `SQLPrimaryKeys`, `SQLForeignKeys`.

## Dialetti (`platypus-sql`)

| Driver | Identificatori | Limite righe |
|---|---|---|
| Snowflake, Trino | `"…"` | `LIMIT n` |
| Oracle, Db2 | `"…"` | `FETCH FIRST n ROWS ONLY` |
| BigQuery, Databricks | `` `…` `` | `LIMIT n` |
| Adbc, Odbc | scelto dall'utente: ANSI (default) o un dialetto esistente | come il dialetto |

Prima di aggiungere un dialetto: controllare che non esista già su `main` o sui branch
(regola di CLAUDE.md).

## Interfaccia (`apps/desktop`)

- **Selettore dei driver:** otto tessere nuove (loghi da `simple-icons` dove ci sono:
  Snowflake, Google BigQuery, Databricks, Trino, Oracle, IBM; per i generici un'icona Lucide).
- **Dialog di connessione:** campi del profilo; per i driver con ponte una riga di stato del
  driver ("installed vX · Update · Remove" oppure "not installed · Download").
- **Popup del driver** (mockup approvato): al primo Test o connessione senza driver.
  ADBC: nome, versione, dimensione, licenza, fonte (registro dbc, Columnar), verifica
  SHA-256, "Download and connect"; poi barra di avanzamento annullabile; errore con
  riprova. ODBC: avviso UAC, licenza del vendor, "Download and install", link alla pagina
  del vendor.
- **Settings › Drivers:** elenco dei driver scaricati con versione, aggiornamento,
  rimozione; per ODBC i driver di sistema rilevati.
- **Comandi Tauri:** `driver_status(driver)`, `install_driver(driver, channel)` (eventi di
  avanzamento), `cancel_driver_install`, `list_drivers`, `remove_driver(name)`.
- `platypus-mcp` e la CLI usano gli stessi driver già scaricati; non scaricano niente da soli
  (errore chiaro: "install the Snowflake driver from the app").

## Errori

- Rete assente e nessuna cache: "Cannot reach the driver registry" con il dettaglio.
- Piattaforma senza pacchetto: "No Snowflake driver for this platform".
- SHA-256 diverso: file cancellato, "The downloaded driver is corrupted"; nessun caricamento.
- Libreria che non si carica (dipendenze mancanti, architettura): errore del loader con il
  percorso.
- Driver rimosso mentre una connessione lo usa: la connessione resta finché non si chiude;
  la rimozione cancella i file al prossimo avvio se sono in uso (Windows blocca le DLL).

## Test

- **Unitari (`platypus-drivers`):** lettura di un `index.yaml` di prova, scelta della versione
  per piattaforma, rifiuto delle licenze `LicenseRef-*`, sha256 sbagliato rifiutato,
  estrazione con limite di dimensione, `driver.toml` scritto e riletto, rimozione. Rete
  finta con un server HTTP locale nei test.
- **Unitari (ponti):** traduzione profilo → opzioni ADBC / connection string ODBC per ogni
  profilo; conversione degli errori.
- **Store:** v5 → v6, `options` salvate e rilette.
- **Conformance:** profilo `trino` in `docker/compose.yml` (`trinodb/trino`, catalog `memory`)
  usato attraverso il ponte ADBC (driver scaricato nella cache dei test al primo giro, salto
  con `skipping` se la rete manca); ODBC generico con psqlODBC sul container PostgreSQL
  (salto se psqlODBC non è installato); fase 2: profili `oracle` (`gvenzl/oracle-free`) e
  `db2` (`icr.io/db2_community/db2`), saltati se il driver ODBC manca.
- **E2E:** Trino dal selettore, popup, download reale, connessione al container, query;
  Settings › Drivers mostra il driver e lo rimuove.
- Snowflake, BigQuery, Databricks: solo test del profilo, più una prova manuale con un
  account di prova se disponibile.

## Documentazione

README (database supportati e come arrivano i driver), EXTENDING.md (come si aggiunge un
profilo ADBC/ODBC, il crate `platypus-drivers`, file v6), ROADMAP (riga del gruppo G).
