<!-- source: 8b43e38d9f7d -->
[← Database](README.md)

# Redis

Il driver è basato sul crate `redis` (RESP3 quando il server lo parla, con rustls per il TLS). Apre
Redis 6 e successivi, e Redis Stack per le chiavi JSON. L'editor accetta **comandi Redis**, uno per
riga. Il driver fa parte dell'app; non si scarica nulla.

## Connessione

| Incolli | Lo legge come |
|---|---|
| `redis://:s%40cret@cache:6380/3` | TCP in chiaro, solo password, database 3 |
| `redis://app:pw@cache/0` | un utente ACL e la sua password |
| `rediss://app:pw@cache` | TLS (verificato: `verify-full`) |

La porta predefinita è 6379. Il percorso (o il campo Database) è il **numero del database**, `0` se
vuoto; qualsiasi altra cosa è un errore. Utente e password sono quelli dell'ACL: una password da sola
accede come utente predefinito.

Il **TLS** è attivo per gli URL `rediss://` e per le modalità SSL da `require` in su (`redis://`
significa niente TLS e `prefer` non lo attiva, perché Redis accetta il TLS su una porta tutta sua).
`require` non controlla il certificato; `verify-ca` e `verify-full` sì. Un file CA e un certificato
client vanno nei campi TLS. La sessione apre una connessione per ogni database che usa, perché un
`SELECT` su una connessione condivisa sposterebbe tutti gli altri che la usano.

## Privilegi

Le regole ACL dell'utente decidono cosa possono fare il navigator e i comandi. Il navigator legge
`INFO` e, per ogni tipo di chiave, esegue `SCAN … TYPE`; sfogliare un tipo esegue `BROWSE`, mai
`KEYS`. Non ci sono schede di amministrazione.

## Cosa funziona

- Comandi nell'editor, tra virgolette come le usa `redis-cli`; le righe con `#` e `--` sono commenti.
  Le risposte sono righe, con le risposte annidate come JSON nell'albero dei documenti (`F7`).
- `SELECT n` sposta la sessione su un altro database. `MULTI` mette in coda i comandi fino a `EXEC`,
  che li invia come una transazione e mostra ogni risposta.
- Il navigator elenca i database che contengono chiavi, ciascuno con una tabella per tipo di chiave:
  `string`, `hash`, `list`, `set`, `zset`, `stream`, e `json` con RedisJSON. Aprirne una esegue
  `BROWSE db0.hash`, facoltativamente con `MATCH` e `LIMIT`, leggendo le chiavi grandi un pezzo alla
  volta.
- **La griglia modifica le righe**: valori, TTL, nomi delle chiavi, campi hash, membri e punteggi; le
  righe nuove aggiungono chiavi ed elementi, quelle eliminate li rimuovono. L'anteprima mostra i
  comandi, che girano come un unico `MULTI`/`EXEC`; un comando che il server rifiuta non annulla gli
  altri. Uno *Script di annullamento* rimette i valori. Una chiave, un campo o un membro che esiste già
  ferma il salvataggio prima che giri qualsiasi cosa.
- `FLUSHDB` e `FLUSHALL` chiedono conferma prima, come un `UPDATE` senza `WHERE`; sulle connessioni di
  produzione i comandi che cambiano il server (`CONFIG SET`, `SHUTDOWN`) chiedono come fa il DDL. Le
  connessioni in sola lettura eseguono solo i comandi che leggono.
- Annulla ferma un comando bloccante (`BLPOP`, `XREAD BLOCK`).
- La modalità terminale esegue `redis-cli` se è nel PATH, con la password nel suo ambiente.
- Tutti i comandi e i tipi di chiave sono descritti in [FEATURES.md](../../FEATURES.md#redis).

## Cosa non funziona

- Le sottoscrizioni (`SUBSCRIBE` e simili) e `MONITOR` vengono rifiutate: prenderebbero il controllo
  della connessione.
- Niente commit manuale: `MULTI`/`EXEC` è l'unica transazione.
- Niente explain, niente DDL degli oggetti, niente strumenti SQL (editor delle tabelle, diagramma ER,
  confronto degli schemi, query builder, importazione SQL), niente snapshot prima delle modifiche.

## Agenti

`run_query` e `sample_rows` (`BROWSE … LIMIT n`) funzionano; di ogni comando che non sia una lettura
si chiede prima. I comandi che mostrano le credenziali del server (`ACL GETUSER`, `ACL LIST`, e un
`CONFIG GET` il cui pattern corrisponde a un'impostazione di password) sono rifiutati agli agenti in
sola lettura. Rifiutati su Redis: `explain`, `get_er_diagram`, `propose_edits` e
`query_across_databases`. Vedi [Agenti e la CLI](../../AGENTS.md).

## Limiti noti

- Versioni precedenti alla 6.0: si connette con un avviso che alcune funzioni potrebbero non
  funzionare.
- Il numero di chiavi di una tabella compare solo quando il database ha al massimo 10.000 chiavi.
- I valori che non sono UTF-8 compaiono tra virgolette con gli escape.

## Testato su

La conformance suite gira su Redis Stack 7.4 (con RedisJSON): metadati, `BROWSE` su ogni tipo, comandi
e i loro errori, salvataggio e annullamento delle modifiche della griglia, annullamento dei comandi
bloccanti, utenti ACL e selezione del database, e una connessione persa che si riapre. Redis 6 è la
versione più bassa che il driver accetta ma non è eseguita dalla suite.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
