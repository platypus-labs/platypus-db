<!-- source: fd0bf4386fb3 -->
[← Database](README.md)

# Oracle

Il driver usa la libreria client di Oracle (ODPI-C, tramite il crate `oracle`), caricata quando ti
colleghi. La libreria è Oracle Instant Client, che l'app scarica al primo uso.

## Primo uso: Instant Client

Quando scegli **Oracle** per una nuova connessione e il client non è su questo computer, sopra il
modulo si apre un popup: *Oracle needs Oracle Instant Client*. Mostra il pacchetto (Basic Light
23.26.4), la sua dimensione, la sua licenza (Oracle Free Use Terms and Conditions) e da dove arriva,
e controlla il download con uno SHA-256. **Scarica** lo mette nella cartella dei dati dell'app;
**Annulla** lascia il modulo utilizzabile, e l'offerta ritorna a *Prova la connessione* e alla prima
apertura. *Impostazioni › Driver* elenca il client e lo rimuove.

- L'app lo scarica su Windows (x64) e Linux (x64 e ARM64). Su macOS non scarica nulla: installa
  Instant Client da Oracle e metti la sua cartella nel percorso delle librerie.
- Se l'app non ne ha scaricato uno, usa un Instant Client o un client completo che il sistema riesce
  a caricare.
- Windows richiede il Microsoft Visual C++ Redistributable, e Linux la libreria `libaio`
  (`libaio1`, `libaio1t64` o `libaio` a seconda della distribuzione); quando il client non si
  carica, l'errore indica il pezzo mancante.

## Connessione

Non esiste un URL `oracle://`; incolla invece un URL JDBC, come
`jdbc:oracle:thin:@host:1521/service` (si leggono le forme thin e OCI), oppure compila il modulo:

- **Host** e **porta** (predefinita 1521), e **Nome del servizio** nel campo Database
  (per esempio `FREEPDB1`). Vengono scritti come stringa Easy Connect, `host:port/service`.
- **Senza host**, il campo Database è un nome di servizio di rete da `tnsnames.ora`, o un intero
  descrittore di connessione, scritto così com'è.
- **Utente e password.** Senza utente, accede l'account del sistema operativo o un wallet Oracle.
  L'utente si può scrivere alla maniera di SQL*Plus, `sys as sysdba`, oppure puoi scegliere il
  **Ruolo** (`SYSDBA`, `SYSOPER`, `SYSBACKUP`, `SYSDG`, `SYSKM`, `SYSASM`).
- **TLS**: le modalità `disable` e `prefer` usano TCP in chiaro; `require` e quelle sopra usano
  `tcps://` con i certificati attendibili del sistema. `require` salta il controllo del nome del
  server; `verify-ca` e `verify-full` lo controllano. Vale il timeout di connessione. Con un tunnel
  SSH si usa l'estremità locale del tunnel.

## Privilegi

La navigazione legge le viste `ALL_*`, quindi il login vede gli oggetti su cui ha diritti. Annulla
invia un break; se l'istruzione è ancora in esecuzione dopo un paio di secondi, l'app la annulla da
una seconda sessione con `ALTER SYSTEM CANCEL SQL`, che richiede quel privilegio, e se non basta
abbandona la sessione (la sua transazione viene annullata e si apre una nuova connessione).

## Cosa funziona

- Modifica dei dati nella griglia, per chiave primaria (o una chiave unica su colonne non nulle, o
  una chiave virtuale).
- Commit e rollback manuali.
- Blocchi PL/SQL: in uno script un blocco finisce con una riga `/`. Il testo di `DBMS_OUTPUT` torna
  come messaggi.
- Navigator (tabelle, viste, viste materializzate, sequenze, sinonimi, funzioni, procedure, trigger,
  tipi e package), DDL degli oggetti e strumenti dello schema.
- Explain mostra il piano di `EXPLAIN PLAN` e `DBMS_XPLAN`, solo stimato: non c'è Explain analyze.
- Errori di compilazione, numeri e orari al nanosecondo, JSON, vettori, cursori, LOB lunghi e colonne
  di tipo file vengono riletti come li tiene il database.
- Niente schede sessioni, lock, dashboard, istruzioni, utenti o parametri.
- La modalità terminale non è disponibile.

## Agenti

Gli agenti in sola lettura si affidano solo al controllo delle istruzioni: qui Oracle non ha una
sessione in sola lettura lato server. Un'istruzione che chiama un package Oracle, come `UTL_HTTP`,
viene chiesta prima dell'esecuzione, come una modifica. Vedi [Agenti e CLI](../../AGENTS.md).

## Limiti noti

- L'app non confronta la versione del server con un minimo; il client che scarica è il 23.26.4.
- Per un server precedente alla 12 il navigator legge il dizionario in un altro modo (niente
  `ORACLE_MAINTAINED`, niente `FETCH FIRST`). Nei test gira solo Oracle Free 23.26, quindi quel
  percorso non è mai stato provato. Neppure Autonomous Database e gli altri servizi cloud di Oracle
  sono provati.
- Instant Client non viene scaricato su macOS.

## Provato su

La conformance suite gira su Oracle Free 23.26 (container `gvenzl/oracle-free`), con Instant Client
scaricato o quello del sistema: tipi, risultati lunghi, errori che indicano lo sbaglio, PL/SQL,
navigator, commit e rollback, salvataggio di molte righe modificate, annullamento (anche di una
pausa PL/SQL), piani, DDL che si riesegue, connessione come `SYSDBA` e confronto degli schemi. Le
altre versioni non sono provate.

Vedi anche: [Connessione](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
