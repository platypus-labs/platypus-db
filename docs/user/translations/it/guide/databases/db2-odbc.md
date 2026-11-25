<!-- source: 1206da228de0 -->
[← Database](README.md)

# IBM Db2 e ODBC (Windows)

Db2, e qualsiasi database con un driver ODBC o un'origine dati (DSN), si raggiungono con il driver
manager ODBC del sistema. Per ora funziona **solo su Windows**: su macOS e Linux una connessione Db2
o ODBC dice che i driver ODBC si raggiungono solo su Windows.

## Primo uso: il driver ODBC di IBM (Db2)

Quando scegli **IBM Db2** per una nuova connessione e su questo computer non c'è nessun driver ODBC
di IBM registrato, sopra il modulo si apre un popup: *IBM Db2 needs IBM Data Server Driver for ODBC
and CLI*. Mostra il pacchetto (versione 12.1.4), la sua dimensione, le condizioni di IBM e da dove
arriva, e controlla il download con uno SHA-256. **Scarica e installa** lo scarica nella cartella dei
dati dell'app e lo registra con `db2cli install -setup`; per farlo **Windows chiede il permesso di
amministratore (User Account Control)**. **Annulla** lascia il modulo utilizzabile, e l'offerta
ritorna a *Prova la connessione* e alla prima apertura.

Un driver già presente nel sistema va bene: l'app ne cerca uno che si chiami `IBM DB2 ODBC DRIVER…`
o `IBM DATA SERVER…`, sia che l'abbia registrato lei o un client IBM completo, e prende il più
recente in base al nome. L'opzione `driver` della connessione sostituisce questa scelta.
*Impostazioni › Driver* elenca e rimuove il driver scaricato, e mostra i driver ODBC e le origini
dati del sistema.

## Connessione

**IBM Db2**: host, porta (50000 per impostazione predefinita), **database** (host e database sono
entrambi obbligatori), utente e password. La stringa di connessione la costruisce l'app (`DATABASE`,
`HOSTNAME`, `PORT`, `PROTOCOL=TCPIP`, `UID`, `PWD`). TLS: le modalità `disable` e `prefer` si
collegano senza; `require` e quelle sopra aggiungono `Security=SSL`, e `sslrootcert` diventa
`SSLServerCertificate`. I server Db2 ascoltano il TLS su una porta diversa da quella in chiaro,
quindi imposta la porta. Con un tunnel SSH si usa l'estremità locale del tunnel.

**Piattaforma** sceglie il tipo di server Db2:

- *Db2 for Linux, UNIX and Windows (LUW)*, la scelta predefinita, anche per i vecchi server 8.x.
- *Db2 for z/OS*: lo stesso driver IBM, porta 446 per impostazione predefinita, e il **database** è
  il nome della location. Il driver di IBM raggiunge z/OS solo con una licenza Db2 Connect su questo
  computer.
- *Db2 for IBM i* (AS/400): il driver di IBM, *IBM i Access ODBC Driver* (IBM i Access Client
  Solutions, che si installa da IBM, non dall'app; va bene anche il vecchio driver iSeries Access).
  L'host è il nome del sistema, il database è facoltativo e imposta la libreria predefinita, non c'è
  la porta e il TLS aggiunge `SSL=1`. Non passa da un tunnel SSH, perché il driver apre più porte del
  server.

**Altro driver ODBC** (la scelta generica): indica una di queste cose

- un'**origine dati (DSN)** che il sistema conosce, scritta a mano o presa dai suggerimenti;
- il nome di un **driver** (suggerito dai driver ODBC del sistema) con una **stringa di connessione**
  che contiene il resto, per esempio `Server=host;Database=sales`;
- una **stringa di connessione** da sola, usata così com'è (può indicare lei stessa `DRIVER=`).

I campi utente e password vengono aggiunti come `UID` e `PWD`, a meno che la stringa non li abbia già.
Una stringa ADO.NET o ODBC incollata con `Driver={…}` viene letta nel modulo corrispondente quando il
driver è SQL Server, PostgreSQL, MySQL/MariaDB o ClickHouse; qualsiasi altro driver, o un `DSN=`,
diventa una connessione ODBC generica con la stringa così com'è.

## Privilegi

Quelli che ha il login del database. Il navigator usa le chiamate di catalogo di ODBC (`SQLTables`,
`SQLColumns`, `SQLPrimaryKeys`, `SQLForeignKeys`). Non ci sono schede di amministrazione.

## Cosa funziona

- Query e risultati, con i tipi che riporta il driver.
- Commit e rollback manuali.
- Annulla: la chiamata di annullamento di ODBC non è raggiungibile, quindi un'istruzione che non è
  ancora partita viene abbandonata insieme alla sua sessione (l'app ne apre una nuova); una che sta
  restituendo righe si ferma quando il suo flusso viene chiuso.
- Il navigator, dalle chiamate di catalogo. L'SQL per Db2 è scritto con le regole di quoting di Db2
  (i nomi senza virgolette diventano maiuscoli, le righe si limitano con `FETCH FIRST`); il driver
  generico scrive SQL standard.

## Cosa non funziona

- Niente explain, niente DDL degli oggetti, niente schede di amministrazione, niente caricamento in
  blocco (le importazioni usano `INSERT`).
- **I valori di testo e binari più lunghi di 64 KiB sono un errore**, invece di un buffer enorme.
- La **modifica dei dati nella griglia** non è provata su questi database. La griglia non la blocca,
  ma senza savepoint un salvataggio fallito in modalità commit manuale annulla l'intera transazione
  della sessione e lo dice.
- La modalità terminale non è disponibile.

## Agenti

Gli agenti in sola lettura si affidano solo al controllo delle istruzioni. Vedi
[Agenti e la CLI](../../../../AGENTS.md).

## Limiti noti

- Solo Windows.
- Db2 non ha un container nell'ambiente di test, quindi la stringa di connessione Db2 e il
  riconoscimento del driver sono verificati solo da unit test.

## Provato su

Il driver ODBC generico è provato contro SQL Server 2022 con il driver `SQL Server` di Windows:
tipi, risultati lunghi, errori con il loro SQLSTATE, metadati, commit e rollback, annullamento di
un'istruzione in attesa, letterali e tipi propri di SQL Server, e importazioni. IBM Db2 non è provato
contro un server.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
