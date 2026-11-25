<!-- source: 25c359215e93 -->
[← Guida utente](README.md)

# Risoluzione dei problemi e FAQ

Comincia con **Prova la connessione** nel modulo della connessione: riporta l'errore con il
messaggio del server, e indica la versione del server dove serve.

## Errori di connessione

### L'host non è raggiungibile

Controlla prima l'host e la porta (il modulo dice "Manca l'host." o "La porta deve essere un numero
da 1 a 65535."). Poi guarda cosa sta tra te e il server: un tunnel SSH, un proxy, un port forward, o
un comando attorno alla connessione che avvia una VPN (pagina **Rete**). Ognuno aggiunge un proprio
messaggio, qui sotto.

### TLS

Nella pagina **Rete**, **Modalità SSL** decide cosa viene controllato (vedi
[Connettersi](connect.md#tls)).

- Un server che rifiuta una connessione non cifrata richiede `require` o superiore; un server con un
  certificato di cui il tuo computer non si fida richiede `verify-ca` o `verify-full` con il
  **Certificato CA (PEM)**, oppure `require` per cifrare senza controllare.
- "invalid certificate" e "invalid root certificates" indicano un file PEM che non si riesce a
  leggere: controlla che sia il certificato stesso, non una chiave.
- "a client certificate needs both the certificate and its key": compila insieme **Certificato
  client (PEM)** e **Chiave client (PEM)**. "invalid client certificate or key" vuol dire che non
  corrispondono o non sono PEM.
- Redis, Elasticsearch e Cassandra prendono TLS da una modalità SSL `require` o superiore.

### SSH

Gli errori cominciano con `SSH <user>@<host>:<port>:`.

- `no answer within 10 s` o `cannot reach <host>:<port>`: il server SSH (o il proxy davanti) non è
  raggiungibile da qui.
- `cannot read the key <path>`: il **File della chiave privata** manca, non si può leggere, o
  richiede la sua passphrase.
- `no SSH agent is running`: avvia il tuo agente SSH, oppure usa un file di chiave o una password.
- Una password o una chiave rifiutata è la risposta del server: controlla **Utente SSH** e il
  metodo.
- Un server la cui chiave è cambiata dalla prima connessione viene rifiutato. Le chiavi sono quelle
  di `~/.ssh/known_hosts`, condiviso con OpenSSH: se il cambiamento è previsto, togli la vecchia
  riga da lì.

### Proxy

Gli errori nominano il proxy, per esempio `SOCKS5 proxy host:1080: …` o `HTTP proxy host:3128: …`.

- `no answer within 10 s`: il proxy non risponde.
- `the proxy asks for a user and password, or refused them`: scrivili nell'URL
  (`http://user:password@host:3128`).
- `cannot reach <host>:<port>: …`: il proxy ha risposto ma non ha potuto raggiungere il database.
- Un URL che non viene capito è segnalato come `proxy "<url>": …`. Un port forward va da solo:
  togli il tunnel SSH o il proxy.

### Portachiavi

- `keychain error: …`, o "the system has no keychain to save the password in": su Linux deve essere
  in esecuzione un Secret Service come GNOME Keyring o KWallet. Senza, imposta una password
  principale (Impostazioni › **Password principale**) o scegli **Chiedi ogni volta** per la
  password.
- "No password is saved for this connection: it will be saved in the keychain." è solo un avviso.
- Le password importate da un altro strumento senza le sue (DBeaver con una propria password
  principale, per esempio) vengono chieste al primo utilizzo.

### Password principale

- "PlatypusDB è bloccato" o "Inserisci la password principale per usarle": digitala e fai clic su
  **Sblocca**. Il blocco per inattività è in Impostazioni › **Sicurezza**; le connessioni di
  produzione si riconnettono quando servono.
- Non si può recuperare. Se la perdi, le password salvate sono perse: reimposta le password in ogni
  connessione.
- Per tornare al portachiavi, Impostazioni › **Password principale** › **Disattiva…** (chiede quella
  attuale).

## Un download di driver fallisce

DuckDB, Snowflake, BigQuery, Databricks, Trino, gli altri driver ADBC, Oracle e IBM Db2 scaricano la
loro libreria la prima volta che si usano. Il popup dice "Download del driver `<name>`…" con un
**Annulla**, e un errore mostra un messaggio con **Riprova**.

- `the download failed (check the internet connection)` o `cannot reach the driver registry`: nessun
  percorso verso il registro; controlla la rete, il proxy o il firewall, poi **Riprova**.
- `the downloaded driver is corrupted (its SHA-256 does not match)`: il file è stato alterato o
  tagliato durante il trasferimento; riprova, e segnalalo se continua.
- `no <name> driver for this platform`, o "PlatypusDB downloads DuckDB on Windows, macOS and Linux,
  on x64 and ARM processors; not on this system.": non c'è una build per il tuo sistema.
- `the driver's license (…) does not allow downloading it here`: l'app non scarica i driver la cui
  licenza lo vieta.
- IBM Db2 si raggiunge tramite ODBC solo su Windows, per ora; registrare il suo driver chiede il
  permesso di amministratore (Controllo account utente). Rispondi di sì, altrimenti il driver non
  viene registrato.
- **Annulla** lascia il modulo utilizzabile; l'offerta ritorna a **Prova la connessione** o alla
  prima apertura.

Impostazioni › **Driver** elenca ciò che è stato scaricato, rimuove un driver e mostra i driver ODBC
del sistema. Una rimozione che Windows ha rifiutato si completa al prossimo avvio.

## Un agente non vede una connessione

Scorri questo elenco.

1. **L'accesso è disattivato.** L'agente risponde "`<name>` is not available to agents". In
   Impostazioni › **Agenti** › **Sicurezza**, imposta la connessione su **Sola lettura** o
   **Lettura e scrittura**. Una connessione arrivata da un progetto condiviso parte con
   **Disattivato** sul tuo computer.
2. **Una password principale.** L'agente risponde "the saved password of `<name>` is locked by
   PlatypusDB's master password, which agents cannot enter". Dai a quella connessione un comando
   per la password o `.pgpass`, oppure disattiva la password principale.
3. **La password viene chiesta ogni volta.** L'agente risponde "`<name>` asks for its password at
   each connection, and agents cannot type it". Salvala nel portachiavi, oppure usa `.pgpass` o un
   comando per la password.
4. **Scritture rifiutate.** Le connessioni di produzione e di sola lettura non vanno mai oltre la
   sola lettura; sulle altre la modifica aspetta la tua risposta nell'app ([Agenti](agents.md)).
5. **Il client non è stato riavviato** dopo **Installa**, oppure la voce manca: Impostazioni ›
   **Agenti** › **Configurazione** dice se PlatypusDB è nel file del client.
6. **Una copia portabile o un altro profilo.** L'agente legge le connessioni del profilo da cui è
   stato avviato; un client configurato con l'app installata non vede la cartella `data` di una
   copia portabile.

## La copia portabile

- È un eseguibile con una cartella `data` vuota accanto. Se la cartella non c'è all'avvio, l'app usa
  invece le cartelle di sistema, quindi una cartella `data` rinominata sembra aver fatto perdere le
  connessioni: rinominala di nuovo.
- Tutto è in `data`, log compresi; la cartella si sposta con l'eseguibile. Le password salvate
  restano nel portachiavi di sistema del computer, a meno che una password principale non le tenga
  anche nella cartella: su un altro computer, imposta prima la password principale oppure inserisci
  di nuovo le password.
- Non si aggiorna da sola: sostituisci il suo eseguibile con quello nuovo. La schermata di benvenuto
  dice `portable` accanto alla versione.

## Dove sono i dati e i log

L'app tiene connessioni e impostazioni (`connections.toml`, `settings.toml`, e `secrets.vault` con
una password principale) nella sua cartella di configurazione, e cronologia, script, driver scaricati
e log nella sua cartella dei dati, entrambe chiamate `dev.platypusdb.desktop`:

| Sistema | Cartella di configurazione | Cartella dei dati |
|---|---|---|
| Windows | `%APPDATA%\dev.platypusdb.desktop` | `%APPDATA%\dev.platypusdb.desktop` |
| macOS | `~/Library/Application Support/dev.platypusdb.desktop` | la stessa |
| Linux | `~/.config/dev.platypusdb.desktop` | `~/.local/share/dev.platypusdb.desktop` |

Una copia portabile tiene tutto nella sua cartella `data`, e la variabile d'ambiente
`PLATYPUS_DATA_DIR` mette tutto nella cartella che indica.

![Impostazioni, la pagina Diagnostica](../../../../screenshots/guide/diagnostics.png)

Il log è `logs/platypus.log` nella cartella dei dati: una riga per ogni avvio, errore e crash, con
rotazione a 1 MB e due file più vecchi (`platypus.1.log`, `platypus.2.log`). Impostazioni ›
**Diagnostica** mostra la cartella (**Copia**) e **Salva pacchetto diagnostico…**, uno zip con la
versione, il sistema, le impostazioni senza password e i log, da allegare a una segnalazione di bug.
Non viene inviato nulla da nessuna parte; leggi i log prima di condividerli, perché un errore può
nominare un server o una tabella. Dopo un crash, al prossimo avvio viene offerto lo stesso
pacchetto.

## Aggiornamenti

- All'avvio l'app cerca una release più recente e mostra "PlatypusDB `<version>` is available" con
  **Più tardi** e **Installa e riavvia**.
- Una copia portabile lo dice soltanto: sostituisci il suo eseguibile. Un'installazione da un gestore
  di pacchetti (Flatpak, l'AUR) lo dice e lascia l'aggiornamento a lui.
- I driver scaricati si aggiornano a parte: Impostazioni › **Driver** › **Cerca aggiornamenti**, o le
  impostazioni **Cerca aggiornamenti dei driver all'avvio** (attiva, al massimo una volta al giorno)
  e **Installa da solo gli aggiornamenti dei driver** (disattivata). I driver integrati si aggiornano
  con l'app.

## FAQ

**Posso usare PlatypusDB offline?** Sì, con un database locale. I download (driver, aggiornamenti)
richiedono la rete.

**Dove segnalo un bug?** Sul [tracker delle issue](https://github.com/platypus-labs/platypus-db/issues),
con il pacchetto diagnostico se puoi.

**Dove sono le scorciatoie?** [SHORTCUTS.md](../../../SHORTCUTS.md), e Impostazioni › Scorciatoie da
tastiera per cambiarle.
