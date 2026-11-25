<!-- source: 284fda1cc34a -->
[← Guida utente](README.md)

# Connettersi a un database

![Scelta del database per una nuova connessione](../../screenshots/connections.png)

## Da un URL

1. Fai clic su **Nuova connessione** (oppure sul `+` del navigatore).
2. Incolla l'URL sotto i loghi: `postgres://`, `mysql://`, `sqlserver://`, `clickhouse://`,
   `libsql://`, `mongodb://`, `redis://`, `rediss://`, `elasticsearch://`, `opensearch://`, un
   URL JDBC o una stringa ADO.NET / ODBC. Il modulo si apre già compilato per quel database.
3. Scrivi la password nel campo **Password**, non nell'URL: il modulo te lo ricorda se l'URL ne
   contiene una.
4. Fai clic su **Prova la connessione**, poi su **Salva**.

Oppure scegli il database dai loghi (la ricerca in alto accetta `Enter` per il primo risultato).
La scelta imposta il driver; **Indietro** riporta ai loghi.

## Il modulo

Il modulo ha quattro pagine: **Generale**, **Aspetto e sicurezza**, **Rete** e **Avvio**. Un
puntino dopo il nome di una pagina indica che contiene delle impostazioni.

- **Generale**: host, porta, utente, database e da dove arriva la password (sotto).
- **Aspetto e sicurezza**: l'ambiente (Sviluppo, Test o Produzione), un colore, la casella
  **Sola lettura (rifiuta le modifiche ai dati)**, le caselle che bloccano gli export e la modifica
  dei dati, e l'accesso degli agenti. Vedi [Sicurezza](safety.md) e [Agenti](agents.md).
- **Rete**: tunnel SSH, modalità SSL e certificati, proxy, port forward e comandi prima e dopo la
  connessione.
- **Avvio**: un file di variabili, uno schema o un fuso orario predefinito, e istruzioni che
  vengono eseguite a ogni connessione.

## Password

In **Generale**, la password può arrivare da:

- **Salvata nel portachiavi**: il portachiavi del sistema. **Prova la connessione** la invia solo
  al server per cui è stata salvata: se cambi l'host, la porta o i server SSH intermedi, ti chiede
  di digitare la password per quella prova. Un altro database dello stesso server PostgreSQL o
  MySQL non è un altro server.
- **Chiedi ogni volta**: la digiti a ogni connessione. Gli agenti non possono aprire una
  connessione così.
- **File .pgpass** (solo PostgreSQL; host, porta, database e utente della connessione scelgono la
  riga, come fa libpq).
- **Comando di shell**: un comando che stampa la password.

Ogni campo può leggere una variabile d'ambiente scritta `${env:NAME}`. I gestori di password
(1Password, Bitwarden, HashiCorp Vault, AWS Secrets Manager) sono nella stessa lista per ogni
database il cui modulo chiede una password o un token; nessuno è limitato a un solo database.
Indica il riferimento del segreto: l'app costruisce da esso il comando di shell (`op read`,
`bw get password`, `vault kv get`, `aws secretsmanager get-secret-value`) e lo esegue a ogni
connessione. Il loro strumento a riga di comando deve essere installato e con l'accesso
effettuato. Niente di tutto questo è stato provato con un vero account 1Password, Bitwarden, Vault
o AWS: sono testati solo i comandi che l'app costruisce e il comando di shell stesso.

Alcuni database accedono senza alcuna password, e la stessa lista li offre: un token dallo
strumento a riga di comando di un cloud (AWS IAM e Google Cloud IAM per PostgreSQL e MySQL,
Microsoft Entra ID per questi due e per SQL Server), l'accesso di Windows per SQL Server, e un
**Ticket Kerberos (GSSAPI)** per PostgreSQL su Windows e Linux. Esegui prima `kinit`, oppure
imposta `KRB5CCNAME` o `KRB5_CLIENT_KTNAME`; su Windows il ticket è quello della tua sessione. Il
ticket è l'unica cosa che va al server, e una prova senza ticket dice cosa eseguire. La
[pagina di PostgreSQL](databases/postgresql.md) ha i dettagli, e cosa è testato e cosa no: un
vero ticket ha già funzionato da Linux, non ancora attraverso SSPI di Windows.

### Password principale

Una password principale conserva le password salvate in un file cifrato invece che nel
portachiavi.

1. Apri Impostazioni (`Ctrl+,`) › **Password principale**.
2. Fai clic su **Imposta una password principale…**, digitala due volte (almeno 8 caratteri) e
   conferma.
3. Per tornare al portachiavi, fai clic su **Disattiva…** nella stessa pagina. **Blocca ora**
   blocca subito.

Non si può recuperare: senza di essa le password salvate sono perse. Gli agenti non possono
inserirla (vedi [Risoluzione dei problemi](troubleshooting.md#un-agente-non-vede-una-connessione)).

## TLS

Nella pagina **Rete**, **Modalità SSL** accetta i valori di libpq: `disable`, `prefer`, `require`,
`verify-ca` e `verify-full`. `prefer` e `require` cifrano senza controllare il certificato;
`verify-ca` e `verify-full` lo controllano, quindi può servire un **Certificato CA (PEM)**. Per i
server che autenticano i client con un certificato, indica il **Certificato client (PEM)** e la
**Chiave client (PEM)** (PostgreSQL e MySQL).

## Tunnel SSH

![Un tunnel SSH nella pagina Rete](../../screenshots/guide/connect-network.png)

1. Nella pagina **Rete**, spunta **Connetti attraverso un tunnel SSH**.
2. Compila **Host SSH**, la porta e **Utente SSH**.
3. Autenticati con una password, un **File della chiave privata** (e la sua passphrase) o l'**Agente
   SSH**. Per un bastion, aggiungi un **Jump host** (**Prima attraverso un jump host**).
4. Fai clic su **Prova la connessione**.

La chiave del server viene controllata con `~/.ssh/known_hosts`: un server sconosciuto viene
memorizzato la prima volta, e una chiave cambiata da allora viene rifiutata. Un **Profilo di rete**
condivide un solo tunnel e le stesse impostazioni SSL tra più connessioni.

## Proxy e port forward

Sempre in **Rete**: un proxy SOCKS5 o HTTP (`socks5://user:password@host:1080`,
`http://host:3128`), oppure un port forward tramite `kubectl` o una sessione di AWS Systems
Manager (la CLI deve essere installata e con l'accesso effettuato). Un port forward va da solo:
togli il tunnel SSH o il proxy.

## Dopo

I dettagli di ogni database: [Database](databases/README.md). L'accesso al cloud, i pooler,
l'import da DBeaver, DataGrip e pgAdmin (comprese le connessioni Db2) e l'export delle connessioni
sono in [FEATURES.md](../FEATURES.md#connections). Se non si connette, vedi
[Risoluzione dei problemi](troubleshooting.md#errori-di-connessione).
