<!-- source: 6f25d7181e09 -->
[← Database](README.md)

# MySQL e MariaDB

Un solo driver (il client `mysql_async`) apre MySQL 5.7 e successivi e MariaDB 10.6 e successivi.
Entrambi hanno un riquadro nella griglia dei database; il driver capisce a quale server si è
collegato e lo mostra nella connessione (`Connected: MariaDB 11.4`). Non si scarica nulla.

## Connettersi

| Incolli | Letto come |
|---|---|
| `mysql://user:pass@host:3306/db?sslMode=REQUIRED` (o `mariadb://`) | la forma URL; porta predefinita 3306 |
| `jdbc:mysql://host:3307/db`, `jdbc:mariadb://…` | un URL JDBC |
| `Server=db;Port=3306;Database=shop;Uid=app;Pwd=…` | una stringa di connessione Connector/NET, se ha anche `Driver=` o l'hai incollata in un modulo MySQL |

Parametri URL che l'app conosce: `sslmode` (scritto anche `ssl-mode`, `sslMode`, `ssl_mode`),
`sslrootcert`, `sslcert`, `sslkey`, `application_name` e `connect_timeout`.

**TLS**: `disable` lo spegne. `prefer` (il predefinito) usa TLS e ripiega sulla connessione in chiaro
quando il server non ce l'ha o offre solo versioni che il client rifiuta (succede con le immagini di
MySQL 5.7); non ripiega mai quando si accede con un token. `prefer` e `require` non controllano il
certificato. `verify-ca` controlla la catena con `sslrootcert` e non il nome dell'host;
`verify-full` controlla entrambi. I nomi di MySQL (`REQUIRED`, `VERIFY_IDENTITY`) sono capiti. Un
certificato client richiede sia `sslcert` sia `sslkey`.

**L'accesso** avviene con la password, oppure con un token AWS IAM, Google Cloud IAM o Microsoft
Entra ID creato dallo strumento `aws`, `gcloud` o `az`; il token passa su TLS come chiede il server.
Kerberos (`auth_gssapi` di MariaDB, il plugin Kerberos di MySQL) non è ancora supportato.

La connessione è sempre TCP: un socket Unix o una named pipe di Windows non si usano.

## Privilegi

Per esplorare basta ciò che il login può vedere. Le schede di amministrazione leggono:

- **Sessioni**: l'elenco dei processi, da `performance_schema.threads` su MySQL 8 e da
  `information_schema.PROCESSLIST` su MariaDB, MySQL 5.7 o un server con `performance_schema`
  spento. Quanto delle sessioni degli altri utenti il server mostri è una regola del server, che
  l'app non controlla.
- **Lock**: `performance_schema.data_lock_waits`, `data_locks` e `metadata_locks` su MySQL 8 (per
  vedere le attese delle altre sessioni serve `PROCESS`; senza, l'elenco non ha attese e lo dice),
  `INNODB_LOCK_WAITS` in `information_schema` su MariaDB e sui MySQL più vecchi.
- **Istruzioni più costose**: `performance_schema.events_statements_summary_by_digest`. Se
  `performance_schema` è spento, la scheda dice di avviare il server con questo attivo.
- **Tabelle e indici**: `information_schema`, più `performance_schema` per quanto viene letto ogni
  indice; senza, l'uso degli indici non si conosce.
- **Annulla istruzione** e **Chiudi sessione** eseguono `KILL QUERY` e `KILL CONNECTION`.
- **Parametri** e **Dashboard**: le variabili del server e i contatori di stato.
- **Utenti e ruoli**: gli account del server e i loro permessi.
- **Manutenzione**: `ANALYZE`, `OPTIMIZE` e `CHECK TABLE`.
- **Backup…** e **Ripristina…** eseguono `mysqldump` e `mysql`, che devono essere nel PATH.

## Cosa funziona

- Modifica dei dati nella griglia, per chiave primaria (o una chiave univoca su colonne non nulle, o
  una chiave virtuale).
- Commit e rollback manuali, e annullamento di un'istruzione sul server.
- Explain: da MySQL 8.0.16 si ottiene `EXPLAIN FORMAT=TREE`; MySQL più vecchio e MariaDB danno
  `EXPLAIN FORMAT=JSON`. **Explain analyze** è `EXPLAIN ANALYZE` da MySQL 8.0.18 e
  `ANALYZE FORMAT=JSON` su MariaDB; MySQL 5.7 e dalla 8.0.0 alla 8.0.17 lo rifiutano.
- Tutte le schede di amministrazione qui sopra, e gli strumenti di schema (editor di tabelle,
  diagramma ER, confronto).
- Le importazioni usano `LOAD DATA LOCAL` quando il server lo permette (`local_infile` attivo),
  altrimenti istruzioni `INSERT`.
- Gli errori nominano il server da cui vengono, per esempio `Unknown column 'x' in 'field list'
  (MySQL 5.7.44)`.

## Agenti

Gli agenti di sola lettura e le connessioni di sola lettura eseguono
`SET SESSION TRANSACTION READ ONLY`, e le loro query girano in una transazione di sola lettura.
`server_activity` e `top_queries` funzionano. Una `@variabile`, una tabella temporanea o un `SET`
fatti da una chiamata di un agente ci sono ancora nella successiva, in MySQL e MariaDB. Vedi
[Agenti e la CLI](../../../../AGENTS.md).

## Limiti noti

- Più vecchio di MySQL 5.7 o MariaDB 10.6: si connette, con un avviso che alcune funzioni potrebbero
  non funzionare.
- L'editor di tabelle salva con il DDL di MySQL che fa un commit per istruzione: uno script fallito
  può lasciarne applicata una parte.
- La modalità terminale richiede `mysql` nel PATH.

## Testato su

La suite di conformance gira su MySQL 5.7, 8.0 e 8.4 e su MariaDB 10.6 e 11.4, con un certificato
client su TLS reciproco su MySQL 8.4, e con sessioni chiuse dal server.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
