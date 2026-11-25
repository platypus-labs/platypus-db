<!-- source: 641268954234 -->
[← Database](README.md)

# Apache Cassandra e ScyllaDB

Un solo driver, basato sul crate `scylla` (con rustls), parla CQL con il protocollo nativo a entrambi.
Il server dice quale dei due è, e la connessione lo mostra. Il driver fa parte dell'app; non si
scarica nulla. L'editor accetta CQL.

## Connessione

| Incolli | Lo legge come |
|---|---|
| `cassandra://cassandra:s%40cret@node1:9043/shop` | in chiaro, porta 9043, keyspace `shop` |
| `scylla://node1` | lo stesso driver, per ScyllaDB |
| `cassandras://…` e `scyllas://…` | con TLS (verificato: `verify-full`) |

La porta predefinita è 9042. Il campo **Host** è un contact point, oppure più coppie `host:port`
separate da virgole. Il campo **Keyspace** (facoltativo) è il keyspace in cui parte la sessione; il
navigator li elenca tutti in ogni caso.

- **Un solo contact point, o un tunnel**: quel nodo da solo serve ogni istruzione. Un cluster
  annuncia gli indirizzi propri dei suoi nodi, che un container, un NAT o un tunnel nascondono, quindi
  il driver non li segue. Con più contact point il driver li riceve tutti e può usarne uno qualsiasi.
- **Accesso**: utente e password, per l'authenticator a password del server. Senza utente la
  connessione è anonima.
- **TLS**: CQL non può negoziarlo, quindi il TLS è attivo per gli URL `cassandras://` e `scyllas://`
  e per le modalità SSL da `require` in su; `prefer` è in chiaro. `require` non controlla il
  certificato; `verify-ca` e `verify-full` sì. Un file CA e un certificato client vanno nei campi TLS.
- **Le istruzioni non hanno un timeout** del driver: una gira finché non finisce o finché non la
  annulli.

## Privilegi

Il navigator legge `system_schema`, che Cassandra 3 e successivi e ScyllaDB condividono; l'utente
vede quello che il server gli lascia leggere lì. Non ci sono schede di amministrazione. Le scritture
richiedono i permessi che chiede CQL.

## Cosa funziona

- Script CQL, divisi dall'editor (le stringhe `$$ … $$`, i commenti `//` e un `BEGIN BATCH … APPLY
  BATCH` vengono capiti), con risultati tipizzati letti pagina per pagina mentre scorri, usando il
  paging state del server.
- Il navigator: i keyspace come schema, tabelle, viste materializzate, tipi definiti dall'utente,
  funzioni e aggregati, con le chiavi di partizione e di clustering di ogni tabella e i suoi indici; il
  DDL di ogni oggetto.
- **La griglia modifica le righe**, per chiave primaria completa (colonne di partizione e di
  clustering): una riga modificata è un `UPDATE`, una riga nuova un `INSERT` che richiede ogni colonna
  della chiave, una riga eliminata un `DELETE`. L'anteprima mostra le istruzioni, che girano una dopo
  l'altra; una che il server rifiuta ferma le restanti. *Script di annullamento* rimette i vecchi
  valori. Le colonne della chiave non si possono cambiare (elimina la riga e inseriscine un'altra), e i
  counter vengono rifiutati.
- Le lightweight transaction (`IF …`) vengono segnalate come tali con il loro esito.
- Annulla ferma un risultato in lettura. Gli errori mantengono il messaggio del server.
- `USE keyspace` cambia il keyspace della sessione, che il driver ricorda.

## Cosa non funziona

- **Nessuna transazione**: ogni istruzione ha effetto da sola, e non c'è il commit manuale.
- **Le scritture CQL sono upsert e non riportano un numero di righe**, quindi un'istruzione che non
  restituisce righe ne riporta zero modificate; un update di una riga che qualcuno ha eliminato nel
  frattempo la riscrive.
- Il visualizzatore dei dati non ha la barra di filtro e ordinamento, e non mostra il numero di righe.
- Niente explain, e niente strumenti SQL: niente editor delle tabelle, diagramma ER, confronto degli
  schemi, query builder, importazione SQL, snapshot prima delle modifiche, né modalità terminale
  (`cqlsh` accetta la password solo sulla sua riga di comando).

## Agenti

`run_query` e `sample_rows` (un `SELECT … LIMIT n` in CQL) funzionano; di un'istruzione che cambia
dati si chiede prima. Rifiutati su Cassandra: `explain` e `propose_edits`; `estimate_change` risponde
con una nota. Vedi [Agenti e la CLI](../../../../AGENTS.md).

## Limiti noti

- Cassandra precedente alla 4.0 e ScyllaDB precedente alla 5.4 vengono rifiutati alla connessione,
  con "older than … the oldest version supported", a differenza degli altri database, che si
  connettono con un avviso.
- Niente join, niente `OFFSET`, e le righe si trovano solo dalla loro chiave primaria, come previsto
  da CQL.

## Testato su

La conformance suite gira su Apache Cassandra 5 e ScyllaDB 2025.1, senza login: informazioni sul
server, script, ogni tipo come colonna tipizzata, paging, lightweight transaction, errori,
annullamento, il catalogo e il suo DDL (anche funzioni e aggregati), il salvataggio delle modifiche
della griglia e il loro annullamento, un login sbagliato o mancante che viene rifiutato, e un
indirizzo di tunnel. Cassandra 4.0 e ScyllaDB 5.4 sono le versioni più basse che il driver accetta e
non sono eseguite dalla suite.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
