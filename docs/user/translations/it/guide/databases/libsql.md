<!-- source: 7b39ba524d58 -->
[← Database](README.md)

# libSQL e Turso

Il driver parla il protocollo **Hrana 3** su HTTP(S) con un server libSQL (`sqld`) o con Turso.
L'SQL è quello di SQLite, quindi anche il navigator, i tipi e i piani sono quelli di SQLite. Il
driver fa parte dell'app; non si scarica nulla.

## Connessione

| Incolli | Letto come |
|---|---|
| `libsql://db-acme.turso.io?authToken=…` | HTTPS verso quell'host, il token come password |
| `libsql://:token@localhost:8080?sslmode=disable` | HTTP in chiaro, il token al posto della password |
| `libsql://localhost:8080?tls=0&authToken=t` | `tls=0` è HTTP in chiaro, `tls=1` è HTTPS |
| `wss://…`, `ws://…` | lo stesso server su HTTP(S): `wss` è TLS, `ws` no |
| `https://host` o `http://host` in un modulo libSQL | l'indirizzo HTTP del server |

Senza porta si usa quella predefinita dello schema: 443, oppure 80 se il TLS è spento.

- **Token**: il campo password, oppure `authToken` nell'URL.
- **TLS**: un URL `libsql://` è HTTPS con certificato e nome dell'host controllati (`verify-full`),
  perché l'URL porta il token. Anche il riquadro Turso nel modulo parte da `verify-full`. Per un
  `sqld` locale senza TLS scegli `disable`, oppure usa `tls=0` nell'URL. `sslrootcert` indica il file
  della tua CA; altrimenti si usano i certificati attendibili del sistema.

## Privilegi

Quelli che permette il token: l'app invia le istruzioni che esegui, e le query sul catalogo SQLite
per il navigator.

## Cosa funziona

- Modifica dei dati nella griglia, per chiave primaria (o una chiave unica su colonne non nulle, o
  una chiave virtuale).
- Commit e rollback manuali. Un'istruzione gira su un flusso che il server tiene aperto tra una
  richiesta e l'altra (un `baton`), quindi una transazione e le tabelle temporanee durano. Se il
  server chiude il flusso fuori da una transazione, l'app ne apre un altro e continua; dentro una
  transazione, la connessione si dichiara persa e con lei la transazione.
- Explain mostra il piano di query di SQLite; non c'è Explain analyze.
- Ricerca nei metadati e DDL degli oggetti, e strumenti dello schema come per SQLite.
- Niente schede sessioni, lock, dashboard, utenti o parametri.

## Cosa non funziona

- **Annulla**: con HTTP non c'è modo di fermare un'istruzione in esecuzione, quindi Annulla risponde
  che annullare le istruzioni libSQL non è supportato.
- I risultati arrivano interi dal server, non riga per riga.
- La modalità terminale non è disponibile: i server libSQL non hanno un client a riga di comando con
  cui eseguire script.

## Agenti

Gli agenti in sola lettura e le connessioni in sola lettura si affidano solo al controllo delle
istruzioni; al server non viene detto di rifiutare le scritture. Vedi
[Agenti e la CLI](../../../../AGENTS.md).

## Limiti noti

- Un errore di rete dopo l'invio di un'istruzione può lasciare incerto se sia stata eseguita; l'app
  non la rimanda.

## Provato su

La conformance suite gira su `sqld` v0.24.33 (container `ghcr.io/tursodatabase/libsql-server`),
senza TLS. Il cloud di Turso non è provato.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
