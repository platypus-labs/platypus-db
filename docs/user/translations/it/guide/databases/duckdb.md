<!-- source: 728e6511c5f7 -->
[← Database](README.md)

# DuckDB

DuckDB gira dentro l'app, come SQLite: nessun server, host, utente o password. A differenza di
SQLite, l'app non porta con sé la libreria di DuckDB: **la scarica la prima volta** che si apre un
database DuckDB o un file di dati.

## Primo uso: la libreria di DuckDB

Quando scegli **DuckDB** per una nuova connessione e la libreria non è su questo computer, sopra il
modulo si apre un popup: *Scaricare DuckDB?* Mostra il pacchetto (DuckDB 2.0.0, dalla release di
DuckDB su GitHub, licenza MIT), la sua dimensione e la sua origine, e controlla il download con uno
SHA-256. **Scarica** la mette nella cartella dei dati dell'app, una volta per tutti i database e i
file DuckDB; **Annulla** lascia il modulo utilizzabile, e l'offerta ritorna a *Prova la connessione*
e alla prima apertura. *Impostazioni › Driver* elenca la libreria e la rimuove.

L'app la scarica per Windows (x64, ARM64), Linux (x64, ARM64) e macOS (una sola libreria per
entrambi i processori). L'app usa esattamente la versione con cui è stata compilata (2.0.x) e offre
quella se ne è installata un'altra. Offline, con la libreria mancante, la connessione non si può
aprire.

## Connessione

Scegli **DuckDB** e indica il percorso del file del database, oppure incolla un URL:

| Incolli | Significato |
|---|---|
| `duckdb:///home/ana/analytics.duckdb` | quel file, creato se manca (la sua cartella deve esistere) |
| `duckdb:///C:/data/analytics.duckdb` | un file su un'unità di Windows |
| `duckdb:///:memory:` | il database in memoria che le sessioni dell'app condividono |
| `:memory:<name>` come percorso | un database in memoria tutto suo, con un nome |

**File di dati come database.** Un percorso che finisce in `.csv`, `.tsv`, `.json`, `.ndjson`,
`.jsonl`, `.parquet`, `.xlsx`, `.xlsm`, `.xls` o `.ods` si apre come database in memoria su quel file
(puoi anche trascinare un file sulla finestra). CSV, TSV, JSON, NDJSON e Parquet diventano una vista
che porta il nome del file, che DuckDB rilegge a ogni query; ogni foglio di una cartella di lavoro
diventa una tabella, caricata una volta sola. Un CSV o TSV viene prima controllato (ogni riga deve
avere tanti valori quanti l'intestazione), la sua codifica del testo viene rilevata (oppure la indichi
tu), e le opzioni di importazione permettono di saltare le righe errate.

Le estensioni non vengono scaricate né caricate da sole; ci sono solo quelle integrate nella libreria
(Parquet, JSON).

## Privilegi

Quelli del sistema operativo: accesso in lettura al file, accesso in scrittura al file e alla sua
cartella per cambiare qualcosa. Un solo processo alla volta può aprire un file DuckDB in scrittura,
quindi finché l'app ha un file aperto un altro programma (per esempio la riga di comando `duckdb`)
non lo può usare; le sessioni dell'app su un file condividono un'unica istanza. Per questo la
modalità terminale non è offerta.

## Cosa funziona

- Modifica dei dati nella griglia, per chiave primaria (o una chiave unica su colonne non nulle, o
  una chiave virtuale). Un file di dati viene letto in memoria: una vista CSV, JSON o Parquet non è
  una tabella e non si può modificare, e le modifiche ai fogli di una cartella di lavoro non vengono
  riscritte nel file.
- Commit e rollback manuali; annullamento di un'istruzione (`duckdb_interrupt`).
- Explain mostra il piano (`EXPLAIN (FORMAT JSON)`); **Explain analyze** esegue l'istruzione in una
  transazione che poi annulla, ed è rifiutato se hai una transazione aperta.
- Scheda Parametri: le impostazioni di DuckDB. Niente schede sessioni, lock, dashboard, utenti o
  istruzioni.
- Ricerca nei metadati, DDL degli oggetti, strumenti dello schema. DuckDB modifica poco sul posto,
  quindi cambiare una tabella la ricrea e ne copia le righe.
- Le importazioni passano dall'appender di DuckDB, nella transazione della sessione.

## Agenti

Un agente in sola lettura, e una connessione in sola lettura, aprono il file del database in sola
lettura, quando nessun'altra sessione dell'app lo tiene già aperto; un database in memoria o un file
di dati non è mai in sola lettura, e il controllo delle istruzioni blocca lì le scritture. Le sessioni
DuckDB degli agenti non possono leggere né scrivere altri file né caricare estensioni, e chiudono il
file dopo ogni chiamata così che l'app lo possa aprire. `query_across_databases` gira in un DuckDB
tutto suo che l'app scarica come questo. Vedi [Agenti e la CLI](../../../../AGENTS.md).

## Limiti noti

- I file Excel si leggono con il lettore dell'app, non con l'estensione Excel di DuckDB.
- Una versione di DuckDB diversa dalla 2.0.x non viene caricata.

## Provato su

La conformance suite gira su un file DuckDB, con la libreria che corrisponde ai binding dell'app:
transazioni, piani, modifica delle tabelle, valori speciali, numeri a 128 bit e senza segno e orari
con fuso.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
