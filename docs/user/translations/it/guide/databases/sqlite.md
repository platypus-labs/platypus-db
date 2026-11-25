<!-- source: eeb36912ad18 -->
[← Database](README.md)

# SQLite

SQLite gira dentro l'app (`rusqlite` con SQLite incluso): nessun server, host, utente o password,
nulla da scaricare.

## Connessione

Scegli **SQLite** e indica il percorso del file, oppure incolla un URL:

| Incolli | Significato |
|---|---|
| `sqlite:///home/ana/app.db` | il file `/home/ana/app.db` |
| `sqlite:///C:/data/app.db` | un file su un'unità di Windows |
| `sqlite:///:memory:` | un database in memoria |
| `file:app.db?mode=ro` nel campo del percorso | un URI SQLite, passato a SQLite così com'è |

Il file viene creato, come database vuoto, se non esiste ma esiste la sua cartella. Una cartella
mancante è un errore (più probabilmente un refuso o un'unità non montata), e lo è anche un percorso
relativo che non esiste: indica il percorso completo. Un file che non è un database SQLite viene
rifiutato quando ti colleghi.

Una connessione in sola lettura (e un agente in sola lettura) apre il file in sola lettura, così è
SQLite stesso a rifiutare una scrittura. Le impostazioni SSL, SSH e proxy non valgono per un database
incorporato.

## Privilegi

Quelli del sistema operativo: l'app deve poter leggere il file, e scrivere il file e la sua cartella
(per il journal) per cambiare qualcosa. Un file occupato viene atteso prima di dare un errore.

## Cosa funziona

- Modifica dei dati nella griglia, per chiave primaria (o una chiave unica su colonne non nulle, o
  una chiave virtuale). Una colonna `INTEGER PRIMARY KEY` conta come chiave anche se SQLite la
  riporta come nullable.
- Commit e rollback manuali; annullamento di un'istruzione in esecuzione (`sqlite3_interrupt`).
- Explain mostra solo il piano di query di SQLite: non c'è Explain analyze.
- Scheda Parametri: i pragma che contano, con i valori predefiniti documentati da SQLite.
- Niente schede sessioni, lock, dashboard, utenti o istruzioni.
- L'editor delle tabelle, il diagramma ER e il confronto. SQLite modifica poco sul posto, quindi
  cambiare una tabella la ricrea e ne copia le righe; il confronto e la ricostruzione mantengono gli
  indici su espressioni e parziali, le chiavi differite e il contatore di `AUTOINCREMENT`.
- Ricerca nei metadati, DDL degli oggetti, ed esportazioni, importazioni e copie come per qualsiasi
  database SQL.
- La modalità terminale esegue `sqlite3`, se è nel PATH.

## Agenti

Gli agenti in sola lettura e le connessioni in sola lettura aprono il file in sola lettura, ed
eseguono le query con `query_only`. `estimate_change` su SQLite non ha una stima delle righe da dare.
Vedi [Agenti e la CLI](../../../../AGENTS.md).

## Limiti noti

- Un altro processo può tenere occupato il file: l'app aspetta fino a 5 secondi per un file occupato,
  poi lo segnala SQLite.
- Un'istruzione di un agente che chiama `load_extension(…)` viene chiesta prima, come una modifica.

## Provato su

La conformance suite gira sul SQLite incluso nell'app, comprese transazioni, piani, modifica delle
tabelle, la ricostruzione che mantiene `AUTOINCREMENT` e i file aperti in sola lettura.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
