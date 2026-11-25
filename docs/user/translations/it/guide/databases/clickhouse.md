<!-- source: b5b424fa73d7 -->
[← Database](README.md)

# ClickHouse

Il driver parla con l'**interfaccia HTTP** di ClickHouse e legge i risultati nel formato Arrow
stream. Fa parte dell'app; non si scarica nulla. Sfoglia, interroga e progetta tabelle, ma non
modifica le righe nella griglia.

## Connessione

| Incolli | Lo legge come |
|---|---|
| `clickhouse://default@ch.example.com/logs` | HTTP sulla porta 8123, database `logs` |
| `clickhouse://ch.example.com/logs?sslmode=require` | HTTPS; senza porta nell'URL, 8443 |
| `jdbc:clickhouse://host:8123/db` | un URL JDBC |
| `https://host:8443` in un modulo ClickHouse | l'URL HTTP(S) del server |

Usa la porta **HTTP**, non quella nativa (9000). L'utente è `default` se vuoto; la password viene
inviata nell'header `X-ClickHouse-Key`. Il campo Database esegue `USE <database>` dopo la
connessione.

Il **TLS** si sceglie con la modalità, perché HTTP non può negoziarlo: `disable` e `prefer` (il
predefinito) si connettono in **HTTP in chiaro**; `require` e le modalità `verify` usano HTTPS. Con
HTTPS la porta predefinita è 8443 quando l'URL non ne indica una; se compili il modulo, scrivi la
porta tu. `require` non controlla il certificato, `verify-ca` controlla la catena, `verify-full`
anche il nome dell'host; `sslrootcert` indica il file della tua CA, altrimenti si usano i certificati
attendibili del sistema.

Una connessione è una sola sessione HTTP: `SET`, `USE` e le tabelle temporanee durano tra
un'istruzione e l'altra.

## Privilegi

Lo sfoglio legge `system.databases`, `system.tables`, `system.columns` e
`system.data_skipping_indices`; l'utente vede quello che ClickHouse gli lascia vedere lì. Le schede
leggono:

- **Parametri**: `system.settings` e `system.server_settings`.
- **Dashboard**: `system.metrics`, `system.events`, `system.parts` (dimensioni dei database) e
  `system.replicas`; ciò che l'utente non può leggere mostra "—".
- **Utenti e ruoli**: `system.users`, `system.roles`, `system.grants` e `system.role_grants`.
  Vengono elencati anche gli utenti definiti nei file di configurazione del server; il server rifiuta
  di cambiarli.

## Cosa funziona

- Query, risultati, il navigator, l'autocompletamento, il DDL degli oggetti, la ricerca nei metadati e
  il designer delle tabelle (compresa la clausola engine e gli skipping index).
- Explain mostra il piano di `EXPLAIN indexes = 1`; non c'è Explain analyze.
- Annulla ferma l'istruzione sul server con `KILL QUERY`.
- Le schede Parametri, Dashboard e Utenti e ruoli. Niente schede di sessioni, lock, istruzioni né
  tabelle e indici.
- Esportazioni, e importazioni in ClickHouse come istruzioni `INSERT`.

## Cosa non funziona

- **Nessuna modifica dei dati nella griglia**: dice "ClickHouse tables are not edited in place". Il
  salvataggio controllerebbe il numero di righe di ogni modifica dentro una transazione, e ClickHouse
  non ha né transazioni né conteggi di righe per gli update. Cambia le righe con SQL.
- **Nessuna transazione**: la barra del commit manuale è nascosta; ogni istruzione ha effetto da sola.
- Niente chiavi esterne né vincoli di unicità (la chiave primaria si limita a ordinare i dati).

## Agenti

Un agente in sola lettura, e una connessione in sola lettura, inviano ogni istruzione tranne `SET`
con `readonly = 1`, così il server rifiuta le scritture e le table function che raggiungono altri
server o i suoi file (`url`, `s3`, `remote`, `file` e simili). L'istruzione di un agente viene anche
controllata rispetto a un elenco di table function che leggono solo dati locali. Vedi
[Agenti e la CLI](../../AGENTS.md). La modalità terminale non è disponibile: `clickhouse-client` usa
la porta nativa.

## Limiti noti

- HTTP in chiaro a meno che la modalità non sia `require` o più forte: il predefinito `prefer` non
  cifra.
- Si usa solo l'interfaccia HTTP; un server che espone solo la porta nativa non si può raggiungere.

## Testato su

La conformance suite gira su ClickHouse 26.3. Le versioni più vecchie di ClickHouse non sono testate,
e l'app non ha una versione minima per esso.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
