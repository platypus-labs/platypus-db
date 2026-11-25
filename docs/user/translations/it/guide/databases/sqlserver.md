<!-- source: 3830a0c639a7 -->
[← Database](README.md)

# SQL Server e Azure SQL

Il driver parla TDS da sé (un `tiberius` modificato), quindi non c'è nulla da installare o
scaricare. È scritto per SQL Server 2017 (14.0) e successivi, e per Azure SQL (non provato sul
servizio: vedi la fine di questa pagina).

## Connessione

| Incolli | Letto come |
|---|---|
| `sqlserver://sa:pass@host:1433/master?encrypt=true&trustServerCertificate=true` (o `mssql://`) | la forma URL; porta predefinita 1433 |
| `jdbc:sqlserver://host:1433;databaseName=shop;encrypt=true` | un URL JDBC (driver Microsoft, jTDS o il vecchio `jdbc:microsoft:`) |
| `Server=tcp:db.example.com,1433;Database=shop;User Id=sa;Password=…;Encrypt=True` | una stringa di connessione ADO.NET o ODBC |

In un URL, `database` o `databaseName` indica il database, e `encrypt` e `trustServerCertificate`
vengono letti come li leggono le stringhe JDBC e ADO.NET (a meno che non ci sia anche `sslmode`).

Le **istanze con nome** (`host\SQLEXPRESS`) non vengono cercate tramite SQL Server Browser: indica la
porta dell'istanza. Una stringa incollata con un'istanza e senza porta lo dice.

**TLS**: `disable` spegne la cifratura. `prefer` (il predefinito) cifra e non controlla il
certificato (i server spesso ne hanno uno autofirmato), e ripiega sul traffico in chiaro se il server
non ha la cifratura. `require` cifra senza controllare. `verify-ca` e `verify-full` controllano
entrambi la catena **e** il nome dell'host, perché la libreria TLS controlla sempre il nome; punta
`sslrootcert` al file della tua CA, altrimenti si usano i certificati attendibili del sistema.

**Accesso**:

- *Login SQL Server*: utente e password.
- *Accesso di Windows* (`Integrated Security=SSPI` in una stringa incollata): solo quando PlatypusDB
  gira su Windows; altrove è un errore.
- *Microsoft Entra ID*: un token creato dallo strumento `az`, che deve essere installato e con
  l'accesso effettuato. È così che si raggiunge l'accesso Entra di Azure SQL.

**Azure SQL**: il gateway può mandare il client al nodo del database stesso; l'app segue un solo
reindirizzamento di questo tipo, tranne con un tunnel SSH, che porta a un solo indirizzo.

## Privilegi

La navigazione richiede ciò che il login può vedere. Le schede di amministrazione leggono:

- **Sessioni** e **Lock**: `sys.dm_exec_sessions`, `dm_exec_requests`, `dm_exec_connections` e
  `dm_tran_locks`. Per vedere cosa fanno gli altri login, il login ha bisogno di `VIEW SERVER STATE`.
- **Istruzioni più costose**: `sys.dm_exec_query_stats`, la plan cache.
- **Tabelle e indici**: `sys.tables`, `dm_db_partition_stats`, `dm_db_index_usage_stats` e
  `sys.stats`.
- **Parametri**: `sys.configurations`; *Apri nell'editor* scrive un'istruzione `sp_configure`.
- **Dashboard**: contatori delle prestazioni, dimensioni da `sys.databases` e `sys.master_files`, e
  gruppi di disponibilità dove ce ne sono; un valore che il login non può leggere mostra "—".
- **Utenti e ruoli**: i login, i ruoli e i permessi del server.
- **Manutenzione**: `UPDATE STATISTICS`, `ALTER INDEX ALL … REBUILD` e `DBCC CHECKTABLE`.

## Cosa funziona

- Modifica dei dati nella griglia, per chiave primaria (o una chiave unica su colonne non nulle, o
  una chiave virtuale).
- Commit e rollback manuali; annullare un'istruzione in esecuzione (un attention TDS) mantiene la
  sessione, la sua transazione e le sue tabelle temporanee.
- Più result set da un solo batch, e messaggi `PRINT`.
- Più database per connessione: una connessione che indica il suo database mostra solo quello;
  *Mostra tutti i database* elenca gli altri.
- Explain mostra il piano **stimato** (`SHOWPLAN`). Explain analyze è rifiutato.
- *Chiudi sessione* è `KILL`. Annullare l'istruzione di un'altra sessione non è possibile su SQL
  Server, quindi la scheda offre solo di chiudere la sessione.
- Le importazioni usano la bulk copy di SQL Server (inserimenti con `OPENJSON` per le tabelle con
  colonne `money`, `xml` o legacy).
- Strumenti dello schema (editor delle tabelle, diagramma ER, confronto), anche per colonne
  calcolate e con collation, indici ricchi e tabelle temporali. Gli indici XML e spaziali restano
  fuori da un confronto.
- La modalità terminale esegue `sqlcmd`, se è nel PATH.

## Agenti

Su SQL Server il solo controllo delle istruzioni blocca le scritture degli agenti in sola lettura
(nessuna sessione in sola lettura lato server); la transazione di un agente viene annullata.
`server_activity` e `top_queries` funzionano. Vedi [Agenti e la CLI](../../../../AGENTS.md).

## Limiti noti

- Precedente al 2017: l'app si collega e avvisa che alcune funzioni potrebbero non funzionare.
- Niente Explain analyze e niente ricerca delle istanze con nome.

## Provato su

La conformance suite gira su SQL Server 2017 e 2022 (container), e il driver ODBC generico è provato
sullo stesso container 2022. Azure SQL non è provato: il suo reindirizzamento, l'accesso Entra e le
regole di cifratura sono nel codice ma non sono mai stati eseguiti sul servizio.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
