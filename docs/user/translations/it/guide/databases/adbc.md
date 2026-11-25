<!-- source: 6fd0f54f48e3 -->
[← Database](README.md)

# Snowflake, BigQuery, Databricks, Trino e altri driver ADBC

Questi database si raggiungono con i driver **ADBC** (Arrow Database Connectivity), librerie che
l'app carica quando ti connetti. Snowflake, BigQuery, Databricks e Trino hanno una tessera ciascuno;
qualsiasi altro database con un driver ADBC si connette con **Altro driver ADBC**.

## Primo utilizzo: il download del driver

I quattro database citati hanno bisogno del loro driver su questo computer. Quando ne scegli uno per
una nuova connessione e il driver non è installato, sopra il modulo si apre un popup: *Snowflake
richiede un driver* (e così via). Dice che è un driver ADBC, scaricato una volta per tutte le
connessioni di quel database, e mostra il pacchetto, la sua versione, la sua licenza e da dove viene:
il [registro dei driver dbc](https://docs.columnar.tech/dbc/) (Columnar). **Scarica** lo recupera,
ne controlla lo SHA-256 (un pacchetto senza viene rifiutato) e lo tiene con i dati dell'app;
**Annulla** lascia il modulo utilizzabile, e l'offerta torna a *Prova la connessione* e alla prima
apertura.

L'offerta legge solo ciò che c'è su questo computer, quindi compare anche offline; il download ha
bisogno della rete. *Impostazioni › Driver* elenca i driver installati e li rimuove, cerca versioni
più nuove una volta al giorno (o con *Cerca aggiornamenti*), installa un'altra versione o torna a una
precedente con *Versioni…*, e può installare gli aggiornamenti da solo. Dopo una modifica, le
connessioni salvate che usano il driver vengono provate (connessione, versione, schemi, `SELECT 1`),
con *Go back* se una fallisce.
La scelta dell'altro driver non scarica nulla: gli indichi un file di libreria.

## Connessione

Ogni database ha i suoi campi:

| Database | Campi |
|---|---|
| Snowflake | **Account** (per esempio `xy12345.eu-west-1`), utente, password, warehouse, ruolo, database, schema |
| BigQuery | **Progetto**, dataset e un file della chiave dell'account di servizio (JSON); vuoto significa le Application Default Credentials di Google. Nessuna password |
| Databricks | **Nome host del server**, **Percorso HTTP**, catalogo, schema; il campo password è il **token di accesso** |
| Trino | **Host**, porta (8080 per impostazione predefinita), **utente**, catalogo, schema, password (facoltativa) e TLS |
| Altro driver ADBC | il file della **libreria del driver**, un simbolo **entrypoint** facoltativo, un URI, utente e password; ogni altra opzione che aggiungi viene passata al driver |

Le scelte del **TLS di Trino** sono *Automatico* (HTTPS sulla porta 443, altrimenti HTTP),
*Disattivato* (HTTP), *Attivo, verifica il certificato* e *Attivo, non verificato*.

**Altro driver ADBC**: la libreria è il file del driver che hai scaricato o compilato. L'entrypoint
serve solo per una libreria che non esporta `AdbcDriverInit` (per la libreria ADBC di DuckDB è
`duckdb_adbc_init`). L'URI e le opzioni sono quelli che documenta quel driver. Un file mancante, o
che non è un driver, viene segnalato quando ti connetti.

## Privilegi

Quello che l'account o il token hanno sul servizio. Il navigator legge cataloghi, schemi, tabelle e
colonne con la chiamata dei metadati di ADBC.

## Cosa funziona

- Query, risultati, il navigator e l'autocompletamento. Il risultato arriva in streaming come batch
  Arrow.
- Annulla: l'app chiede al driver di annullare; i driver scritti in Go annullano solo le proprie
  chiamate, quindi un'istruzione che non si ferma viene abbandonata insieme alla sua sessione, e
  l'app ne apre una nuova.
- DDL degli oggetti solo su **Trino** (`SHOW CREATE`). Snowflake, BigQuery, Databricks e gli altri
  driver non mostrano alcun DDL.
- Le chiavi esterne che puntano a una tabella non vengono elencate (servirebbe leggere le chiavi di
  ogni tabella).
- Esportazioni, e importazioni di file (le importazioni tipizzate sono testate su Trino).

## Cosa non funziona

- **Nessun commit manuale**: ogni istruzione fa commit da sola, e disattivare l'auto-commit dice che
  il commit manuale con un driver ADBC non è supportato.
- **Niente explain**, e niente schede di amministrazione.
- **Nessuna modifica dei dati nella griglia**: un salvataggio controlla ogni modifica in una
  transazione, e queste connessioni non ne hanno. I risultati restano in sola lettura; usa SQL per le
  modifiche.
- La modalità terminale non è disponibile.

## Agenti

`run_query`, `sample_rows` e gli altri strumenti funzionano, nei limiti dei controlli. Senza una
sessione in sola lettura lato server, gli agenti in sola lettura si affidano al solo controllo delle
istruzioni. Le istruzioni che agiscono al di fuori di esso vengono chieste prima di girare: `query()`
di Trino e le funzioni `SYSTEM$` di Snowflake. Vedi [Agenti e la CLI](../../../../AGENTS.md).

## Limiti noti

- Un accesso a un warehouse cloud diverso dai campi qui sopra (coppia di chiavi, single sign-on) non è
  nel modulo; il driver generico accetta le opzioni proprie del driver.
- Snowflake, BigQuery e Databricks sono nel codice e condividono lo strato dei driver con Trino, ma
  non hanno test sui servizi reali.

## Testato su

La conformance suite esegue **Trino 484** con il suo driver scaricato (tipi, risultati lunghi,
annullamento, metadati, errori, istruzioni che finiscono con un terminatore, importazioni tipizzate, e
righe e UUID) e il **driver ADBC generico** sul driver ADBC di Apache SQLite e su quello di DuckDB.
Snowflake, BigQuery e Databricks non sono testati.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
