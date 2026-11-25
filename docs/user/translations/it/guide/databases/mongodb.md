<!-- source: 81c6c1d2c8d1 -->
[← Database](README.md)

# MongoDB

Il driver è quello ufficiale di MongoDB per Rust (con rustls, quindi senza OpenSSL). Apre MongoDB
5.0 e successivi. L'editor accetta la **sintassi di mongosh**, non SQL. Il driver fa parte
dell'app; non si scarica nulla.

## Connessione

| Incolli | Lo legge come |
|---|---|
| `mongodb://user:pass@host:27017/shop?authSource=admin&replicaSet=rs0` | la forma URL; porta predefinita 27017 |
| `mongodb://a:27017,b:27018/?authSource=admin` | un elenco di seed, tenuto intero nel campo Host |
| `mongodb+srv://user:pass@cluster0.example.net/shop` | SRV: gli host vengono dal DNS |

Quello che non entra nei campi del modulo va nel **campo Database**, dopo un `?`, come opzioni della
stringa di connessione: `shop?authSource=admin&replicaSet=rs0`, con `srv=true` per `mongodb+srv`. Il
nome prima del `?` è il database in cui girano le istruzioni finché `use` non lo cambia (`test` se
è vuoto).

- **Accesso**: utente e password, autenticati su `admin` salvo diversa indicazione di `authSource`.
  Un URL incollato il cui percorso nomina un database lo usa come auth source, come dice la
  specifica della stringa di connessione. Le opzioni che contengono un segreto (una password in
  `authMechanismProperties`, per esempio) non vengono conservate quando incolli un URL; il modulo
  elenca quello che ha lasciato fuori.
- **TLS**: `tls=true` o `ssl=true` nell'URL significa `verify-full`; `tls=false` significa
  `disable`. MongoDB non ha un TLS opportunistico, quindi `prefer` (il predefinito) mantiene quello
  che la stringa di connessione implica: TLS attivo per `mongodb+srv`, disattivo negli altri casi.
  `require` attiva il TLS e accetta qualsiasi certificato; `verify-ca` e `verify-full` controllano
  entrambi la catena e il nome dell'host. `sslrootcert` (o `tlsCAFile`) indica il file della tua CA;
  un certificato client si può dare nel modulo, oppure con `tlsCertificateKeyFile` nel campo
  Database. Le opzioni TLS della stringa di connessione vengono mantenute in ogni modalità.
- **Con un tunnel SSH** si raggiunge solo il server che ci sta dietro (`directConnection=true`).

## Privilegi

Il navigator elenca i database e le collection che l'utente può elencare, e campiona i documenti per
mostrare i campi. Non si legge altro per l'amministrazione: non ci sono schede di amministrazione.
Le istruzioni che scrivi richiedono i ruoli che richiedono.

## Cosa funziona

- Istruzioni in sintassi mongosh, con i valori della shell (`ObjectId(...)`, `ISODate(...)`,
  `NumberLong(...)`, `/regex/i`) ed Extended JSON:
  - **Lettura**: `find` con `sort`, `limit`, `skip`, `projection`, `collation`, `hint`, `min`, `max`,
    `maxTimeMS`, `readConcern` e simili, `findOne`, `aggregate`, `countDocuments`,
    `estimatedDocumentCount`, `distinct`.
  - **Scrittura**: `insertOne`/`insertMany`, `updateOne`/`updateMany`, `replaceOne`,
    `deleteOne`/`deleteMany`, `bulkWrite`, `findOneAndUpdate`/`findOneAndReplace`/`findOneAndDelete` e
    `findAndModify`, con le loro opzioni (`arrayFilters`, `collation`, `hint`, `let`, `upsert`,
    `ordered`, `writeConcern`). Un'opzione che PlatypusDB non conosce viene rifiutata per nome, mai
    scartata.
  - **Indici e collection**: `createIndex`, `createIndexes`, `dropIndex`, `dropIndexes`, `hideIndex`,
    `unhideIndex`, `getIndexes`, `createCollection`, `createView`, `drop`, `renameCollection`,
    `convertToCapped`, `stats`, `validate`, `latencyStats`, `dataSize`, `storageSize`, `totalIndexSize`,
    `totalSize`, `isCapped`, `exists`.
  - **Piani**: `.explain()` dopo un'istruzione o `db.c.explain().find(…)` prima, con una verbosity,
    e l'**Explain** dell'editor. Un piano non applica mai una scrittura.
  - **Il server**: `db.runCommand`, `db.adminCommand`, `db.aggregate`, `db.serverStatus`, `db.currentOp`,
    `db.killOp`, il profiler, utenti e ruoli (`db.createUser` e simili), `rs.status`, `rs.conf`,
    `sh.status`, `show dbs`, `show collections`, `show users`, `show roles`, `show profile`, `show logs`
    e `use`.
- I risultati sono una griglia dei campi di primo livello, con il documento selezionato come albero
  accanto (`F7`). Il navigator elenca database, collection, campi campionati dai documenti e
  indici; l'editor completa collection, metodi e campi.
- Annulla ferma un'istruzione in corso; un risultato abbandonato libera la connessione.
- **Commit manuale** su un replica set o un cluster shardato (vedi [Transazioni](#transazioni)).
- **Modifica dei documenti nella griglia** (vedi [Modifica i documenti nella griglia](#modifica-i-documenti-nella-griglia)).
- La definizione di una collection come DDL.
- **Genera dati** riempie una collection con documenti finti.
- Lo stesso dettaglio è in [FEATURES.md](../../../../FEATURES.md#mongodb).

## Cosa non funziona

- **Solo alcuni risultati si modificano nella griglia**: i documenti di una sola collection (vedi
  [Modifica i documenti nella griglia](#modifica-i-documenti-nella-griglia)). Ogni altro risultato,
  come un'aggregazione, una vista o un `findOne`, resta in sola lettura. Cambia quei documenti con
  `updateOne`, `insertOne` e simili.
- **Nessuna transazione su un server standalone**: MongoDB le esegue solo su un replica set o un
  cluster shardato. Lì il commit manuale viene rifiutato, e lo dice.
- **Niente JavaScript**: l'editor esegue istruzioni, quindi `forEach`, `map`, `next`, `mapReduce`, i
  vecchi bulk builder (usa `bulkWrite`) e le variabili vengono rifiutati, e lo dicono. Anche `watch`
  è rifiutato: un change stream prenderebbe il controllo della connessione. Un `.comment()` su una
  lettura viene omesso: lo usa l'app stessa per trovare l'operazione da annullare.
- `bulkWrite` non è atomico, come in mongosh: quello che è girato prima di un errore resta, e
  l'errore dice fin dove è arrivato. Gli insert vicini viaggiano insieme, ogni altra scrittura per
  conto suo.
- Niente SQL: niente editor delle tabelle, diagramma ER, confronto degli schemi, query builder,
  importazione SQL, snapshot prima delle modifiche, né modalità terminale (`mongosh` accetta la
  password solo sulla sua riga di comando).

## Modifica i documenti nella griglia

La griglia modifica il risultato di un semplice `find` su una collection: il visualizzatore dei dati,
o un'istruzione che scrivi tu, come `db.orders.find({ status: 'paid' }).sort({ _id: 1 }).limit(20)`.
La collection si può nominare con `getCollection("my orders")` o `db.getSiblingDB("shop").orders`.
Dopo `use`, la collection è quella del database a cui sei passato. Una projection va bene se elenca
soltanto campi da includere e tiene `_id`. Viste, aggregazioni, `findOne`, `distinct`, conteggi,
una projection senza `_id` e una projection che esclude dei campi restano in sola lettura.

Un documento si trova dal suo `_id`. Un risultato senza `_id` è in sola lettura. **Anteprima
dell'SQL** mostra il mongosh che verrà eseguito, un'istruzione per documento:

- un documento modificato è `updateOne({ _id: … }, { $set: { … } })`. Una cella NULL scrive `null`.
  Quando un numero cambia, l'update diventa invece una pipeline, `[{ $set: { … } }]`: il numero è
  scritto con uno `$switch` sul tipo che il documento memorizza (vedi sotto), e ogni altro valore
  come `$literal`, così una stringa come `$total` o un documento con chiavi `$` viene salvato com'è;
- un documento nuovo è `insertOne({ … })`. Le celle lasciate NULL non vengono inviate. Senza `_id`,
  l'app crea un `ObjectId`;
- un documento eliminato è `deleteOne({ _id: … })`.

I valori sono scritti con il tipo BSON della loro colonna: `bool`, `string`, `objectId`, `date`, e
`document` e `array` dal JSON che mostra la griglia. Una data si scrive come `2024-01-02 10:30:00`,
con o senza fuso orario (UTC se manca). I numeri dentro un documento o un array mantengono il tipo
che dà loro il JSON: un numero intero è un int (un long oltre i 32 bit), un numero con una frazione
o un esponente un double.

Un numero mantiene il tipo che il documento memorizza, `int`, `long`, `double` o `decimal`, ogni
volta che quel tipo contiene esattamente il nuovo valore: il tipo di una colonna è solo ciò che i
suoi documenti hanno in comune (int e long fanno una colonna `long`, uno qualsiasi dei due con dei
double una colonna `double`), quindi non dice cosa contiene un singolo documento. Un int modificato in
`7` resta un int, un long un long, un double un double. Un valore che il tipo memorizzato non può
contenere prende il tipo della colonna, o il primo tipo numerico che lo contiene: `3.5` in un campo
int diventa un double, `3000000000` un long. Un long oltre 2^53 non viene mai scritto come double,
perché perderebbe cifre. Sul server non si converte nulla: l'app scrive il numero solo nei tipi che
sa contenerlo esattamente.

Queste operazioni vengono rifiutate prima che giri qualsiasi cosa, con il motivo:

- una modifica a `_id`: MongoDB non lo cambia mai. Elimina il documento e inseriscilo di nuovo;
- una colonna di tipi misti (`mixed`): il testo di un valore non distingue una stringa da un numero;
- i dati binari e gli altri tipi che la griglia non sa riscrivere esattamente (espressioni
  regolari, timestamp, JavaScript);
- un campo con un `.` nel nome o con un nome che inizia per `$`: `$set` lo leggerebbe come un
  percorso o un operatore;
- un valore che non rientra nel suo tipo, come `many` in una colonna `int` o un array in una
  colonna `document`.

Ogni istruzione deve cambiare esattamente un documento. Se una non ne trova nessuno, perché il
documento è stato eliminato o il suo `_id` è cambiato da quando la griglia l'ha letto, il salvataggio
si ferma e dice quale comando.

**Tutto o niente richiede un replica set.** Su un replica set o un cluster shardato, un salvataggio
gira in una transazione tutta sua e fa commit solo se ogni comando è riuscito. Se hai una transazione
aperta (commit manuale), il salvataggio gira al suo interno e aspetta il Commit. Se un comando
fallisce, MongoDB non ha savepoint: l'intera transazione viene annullata, insieme alle modifiche che
non avevi ancora confermato, e l'errore lo dice. Su un server standalone non ci sono transazioni: i
comandi girano uno dopo l'altro, e un errore ferma i restanti. I comandi prima dell'errore restano
salvati, e l'errore dice quanti.

Lo script di annullamento che segue un salvataggio rimette a posto i documenti: `$set` dei vecchi
valori per le modifiche, `deleteOne` per i documenti nuovi, `insertOne` dei campi caricati per quelli
eliminati. Le righe che iniziano con `//` dicono cosa non può ripristinare esattamente: i campi che
il risultato non mostrava, le colonne di tipi che la griglia non sa scrivere, i campi che erano NULL
(tornano come `null`, o vengono omessi) e i numeri il cui tipo la modifica può aver cambiato (tornano
come numeri del tipo che hanno allora, con lo stesso `$switch`).

## Transazioni

Il commit manuale funziona su un replica set o un cluster shardato (un `mongos`); basta un replica
set con un solo nodo. Su un server standalone viene rifiutato: il pulsante **Auto-commit** è
disabilitato e ne spiega il motivo. Una connessione di produzione a un replica set si apre in commit
manuale, con Commit e Rollback. La connessione tiene una sola sessione: la prima modifica dopo il
passaggio al commit manuale, o dopo un Commit o un Rollback, apre una transazione al suo interno (un
insert, update o delete, `createCollection`, `createIndex`, un salvataggio della griglia), e Commit o
Rollback la chiude. Le letture prima non ne aprono nessuna: vedono ciò che è confermato, come in
auto-commit, così sfogliare i dati non lascia mai una transazione aperta che il server chiuderebbe
allo scadere della sua durata. Quando una transazione è aperta, le tue letture girano al suo interno e
vedono le sue scritture; le altre sessioni le vedono solo dopo il Commit.

- **Nella transazione**, quando ce n'è una aperta (le letture) o la aprono (il resto): `find`,
  `findOne`, `aggregate`, `countDocuments`, `distinct`, gli insert,
  gli update, i replace e i delete, `bulkWrite`, i metodi `findOneAnd…`, `createCollection` e
  `createIndex`, e `db.runCommand` o `db.adminCommand` con un comando che legge o scrive documenti
  (`find`, `aggregate`, `insert`, `update`, `delete`, `findAndModify`, …). MongoDB accetta un indice
  in una transazione solo su una collection creata nella stessa transazione: su una già esistente, il
  server lo rifiuta e chiude la transazione.
- **Fuori da essa**, come in modalità auto-commit, senza toccarla: ciò che legge metadati o parla con
  il server. `show …`, `use`, `db.stats()`, `db.getCollectionNames()`, `getIndexes`, le statistiche
  della collection, `estimatedDocumentCount`, `.explain()`, gli altri comandi di `db.runCommand` e
  `db.adminCommand`, `rs.*`, `sh.*`, utenti e ruoli, il profiler, il navigator e il completamento.
  Anche le letture dei database `admin`, `config` e `local` e delle collection `system.*` girano
  fuori.
- **Rifiutati in commit manuale**: `drop`, `renameCollection`, `dropIndex`, `dropIndexes`,
  `hideIndex`, `convertToCapped`, `createView`, un `createCollection` time-series, un `aggregate`
  con `$out` o `$merge`, e le scritture su `admin`, `config`, `local` o `system.*`. MongoDB non li
  esegue in una transazione, e se girassero fuori si applicherebbero subito, cosa che il Rollback non
  potrebbe annullare. L'errore lo dice; non viene inviato nulla e la transazione aperta resta com'era.
  Passa all'auto-commit per eseguirli.
- Un risultato letto in una transazione continua a caricarsi mentre scorri, un batch alla volta, e
  nel frattempo possono girare altre istruzioni. Un risultato letto prima che la transazione si
  aprisse continua a caricarsi fuori da essa. Le sue colonne vengono dai suoi primi documenti. Dopo
  un Commit o un Rollback il resto non si può più leggere: esegui di nuovo l'istruzione.

**Quando il server chiude una transazione.** MongoDB interrompe una transazione che resta aperta più
di `transactionLifetimeLimitSeconds`, 60 secondi per impostazione predefinita. Ne interrompe una
anche per alcuni errori: una chiave duplicata, un conflitto di scrittura con un'altra sessione,
un'istruzione che rifiuta in una transazione, un limite di tempo (`maxTimeMS`) raggiunto. Altri
errori, come una fase di pipeline sconosciuta, la lasciano aperta. Dopo un errore PlatypusDB chiede
al server se la transazione esiste ancora. Quando non esiste più, l'errore dell'istruzione o del
Commit dice che il server ha chiuso la transazione e che le sue modifiche sono state scartate (con il
limite di tempo, quando è stato quello il motivo), la transazione risulta chiusa, e l'istruzione
successiva ne apre una nuova. Annullare un'istruzione in una transazione fa il rollback della
transazione: il server chiude una transazione la cui operazione viene terminata, e ciò che
l'istruzione ha fatto non è noto.

## Agenti

`run_query`, `sample_rows` (`find({}).limit(n)`) e gli altri funzionano con istruzioni mongosh; di
un'istruzione che non è una semplice query si chiede prima. Rifiutati su MongoDB: `explain`,
`get_er_diagram` e `query_across_databases`; `estimate_change` risponde con una nota.

`propose_edits` propone documenti da inserire, aggiornare o eliminare in una collection (non una
vista): la collection si apre nel suo visualizzatore dei dati con un `find` degli `_id` proposti e le
modifiche in sospeso, che salvi o scarti come se fossero tue. Un documento si indica solo con
`_id`; un `_id` di tipo `ObjectId` viene inviato come le sue 24 cifre esadecimali e letto come
`ObjectId` perché l'`_id` della collection lo è. I valori vengono controllati con le regole del
salvataggio della griglia (vedi sopra), prima che si apra qualsiasi cosa. Le colonne sensibili sono
mascherate solo per campo di primo livello: un campo sensibile dentro un documento non viene
distinto. Vedi [Agenti e la CLI](../../../../AGENTS.md).

## Limiti noti

- Versioni precedenti alla 5.0: si connette con un avviso che alcune funzioni potrebbero non
  funzionare.
- Il driver è un pool che si riconnette da solo, quindi una connessione persa non viene segnalata
  come per gli altri database.

## Testato su

La conformance suite gira su MongoDB 5.0 e 8.0: informazioni sul server, metadati, annullamento,
risultati abbandonati, errori con la loro posizione, documenti finti, e valori nei documenti
successivi che differiscono dal primo. Le transazioni sono testate su un replica set con un solo nodo
di MongoDB 8.0: commit, rollback, letture dentro e fuori, un risultato di più batch, metadati in
commit manuale, e una transazione che il server ha chiuso per il limite di tempo.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
