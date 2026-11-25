<!-- source: a2b0bfe9784d -->
[← Database](README.md)

# PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB e gli altri

Un solo driver, basato su `tokio-postgres`, parla il protocollo PostgreSQL. Apre PostgreSQL 12 e
successivi, e i server e i servizi che parlano lo stesso protocollo. Non si scarica nulla: il driver
fa parte dell'app.

## Connettersi

Scegli **PostgreSQL** nella griglia dei database, oppure incolla un URL: il modulo si compila da
solo.

| Incolli | Letto come |
|---|---|
| `postgres://user:pass@host:5432/db?sslmode=require` (o `postgresql://`) | la forma URL; la porta predefinita è 5432 |
| `jdbc:postgresql://host/db?sslmode=require` | un URL JDBC, come lo conservano DBeaver e DataGrip |
| `host=db port=5432 dbname=shop user=app sslmode=require` | una stringa di connessione libpq |
| `Host=db;Database=shop;Username=app;Password=…` | una stringa di connessione Npgsql |

Parametri URL che l'app conosce: `sslmode`, `sslrootcert`, `sslcert`, `sslkey`, `application_name`,
`connect_timeout` e `pgbouncer=true`. Ogni altro parametro viene lasciato fuori, e il modulo dice
quale. Un elenco di host per il failover (`h1:5432,h2:5432`) tiene il primo host.

**TLS** segue il `sslmode` di libpq:

| Modalità | Cosa succede |
|---|---|
| `disable` | niente TLS |
| `prefer` (il predefinito) | TLS se il server lo offre, altrimenti in chiaro; il certificato non viene controllato |
| `require` | TLS, certificato non controllato |
| `verify-ca` | TLS, si controlla la catena del certificato, non il nome dell'host |
| `verify-full` | TLS, si controllano catena e nome dell'host |

La catena viene controllata con il file indicato in `sslrootcert` se c'è, altrimenti con i
certificati attendibili del sistema. Un certificato client (`sslcert` e `sslkey`) viene inviato in
ogni modalità che usa TLS.

**L'accesso** avviene con la password, oppure con un token che lo strumento a riga di comando di un
cloud crea a ogni connessione: AWS IAM (`aws`), Google Cloud IAM (`gcloud`) e Microsoft Entra ID
(`az`) sono nella scelta *Autenticazione* della connessione. Lo strumento deve essere installato e
con l'accesso effettuato sul tuo computer. Oppure con **Kerberos**, qui sotto.

**Kerberos (GSSAPI).** Scegli *Ticket Kerberos (GSSAPI)* sotto *Password from*. Il campo della
password sparisce: nessuna password viene chiesta, salvata o inviata. Se il server ne chiede una, o
ti fa entrare senza chiedere Kerberos, la connessione viene rifiutata e te lo dice.

- **Il nome del servizio** è `postgres`, a meno che tu non lo compili (il `krb_srvname` del server).
  Il ticket richiesto è `service/host`, nel tuo realm predefinito: l'host deve essere il nome nel
  principal del servizio, non un indirizzo IP né un alias, perché l'app non lo risolve. Attraverso un
  tunnel SSH o un port forward è comunque il nome del server.
- **L'utente** è il ruolo PostgreSQL, che il server ricava dal tuo principal (`include_realm` e
  `pg_ident.conf`). Se è vuoto, l'app usa il nome utente del tuo sistema.
- **Il ticket** su Linux viene da `kinit`, nella cache indicata da `KRB5CCNAME` (o in quella
  predefinita), o da un keytab: con `KRB5_CLIENT_KTNAME` impostata, Kerberos ottiene il ticket da
  solo. Queste variabili devono essere impostate dove parte l'app. Su Windows è il ticket della tua
  sessione Windows.
- **Senza un ticket** la prova dice cosa eseguire: `kinit`, le due variabili e `klist`, seguiti dal
  testo di Kerberos. Lo stesso vale per un ticket scaduto, un servizio sconosciuto e un KDC
  irraggiungibile.
- **`psql`, `pg_dump` e `pg_restore`** accedono allo stesso modo (`PGKRBSRVNAME`,
  `PGREQUIREAUTH=gss`, e niente `PGPASSWORD`). La loro copia di libpq deve essere stata compilata
  con GSSAPI.
- **Quali build lo hanno.** I pacchetti per Windows (SSPI) e Linux (GSSAPI, MIT Kerberos). Quelli
  per macOS no: nessun Mac l'ha ancora compilato, e il modulo lo dice. La feature Cargo `kerberos`
  di `platypus-driver-pg` lo attiva; senza, di Kerberos non si collega e non si esegue nulla.
- **La cifratura GSS** (`hostgssenc`, `gssencmode`) non si usa: per cifrare usa TLS, con righe
  `hostssl` o `host`. Un `krbsrvname` in un URL incollato viene lasciato fuori, come ogni altro
  parametro che l'app non conosce.

**Pooler.** Dietro PgBouncer, Supavisor o il pooler di Neon in transaction mode, scegli *Transaction
mode* nel modulo (o aggiungi `pgbouncer=true` all'URL). Le istruzioni girano allora senza nome,
perché la transazione successiva potrebbe essere su un'altra sessione del server. Il modulo lo
suggerisce per gli host che finiscono con `.pooler.supabase.com` e per gli host Neon con `-pooler`
nel nome. In questa modalità il debugger PL/pgSQL è spento, e le sessioni di sola lettura degli
agenti si affidano solo al controllo dell'istruzione.

Gli altri server hanno riquadri propri, che impostano soltanto dei valori predefiniti:

| Riquadro | Valori predefiniti che imposta |
|---|---|
| CockroachDB | porta 26257 |
| YugabyteDB | porta 5433, utente e database `yugabyte` |
| TimescaleDB | nessuno |
| Supabase | utente e database `postgres`, TLS `require` |
| Neon, AlloyDB | TLS `require` |

## Privilegi

Per esplorare basta ciò che il server mostra al login. Le schede di amministrazione leggono questi:

- **Sessioni** e **Lock**: `pg_stat_activity` e `pg_locks`. L'app elenca le sessioni client che il
  server restituisce; quanto delle sessioni e delle istruzioni degli altri utenti il server mostri
  al login è una regola del server, che l'app non controlla. *Annulla istruzione* e *Chiudi
  sessione* chiamano `pg_cancel_backend` e `pg_terminate_backend`; è il server a decidere se il
  login può farlo.
- **Istruzioni più costose**: l'estensione `pg_stat_statements`. Senza, la scheda dice come
  attivarla (aggiungerla a `shared_preload_libraries`, riavviare, `CREATE EXTENSION`).
- **Tabelle e indici**: `pg_stat_user_tables` e `pg_stat_user_indexes`.
- **Parametri** e **Dashboard**: `pg_settings`, `pg_stat_database`, `pg_stat_activity` e le viste
  della replica; gli standby sono elencati solo a un membro di `pg_monitor`. Un valore che il login
  non può leggere mostra "—" e il motivo.
- **Utenti e ruoli**: `pg_roles` e `pg_auth_members`; per modificarli servono i diritti che
  richiedono le istruzioni.
- **Manutenzione**: `VACUUM`, `ANALYZE` e `REINDEX` sulla tabella.
- **Backup…** e **Ripristina…** eseguono `pg_dump`, `pg_restore` e `psql`, che devono essere nel
  PATH.
- **Debug…** richiede l'estensione `pldebugger` sul server.

## Cosa funziona

- Modifica dei dati nella griglia, per chiave primaria (o una chiave univoca su colonne non nulle, o
  una chiave virtuale che imposti tu). Array e valori compositi si modificano un elemento o un campo
  per riga.
- Transazioni con commit manuale, e istruzioni annullate sul server.
- Explain ed Explain analyze; il piano viene letto dal JSON o dal testo di PostgreSQL.
- Più database per connessione, partizioni, geometrie PostGIS, array e tipi compositi.
- Le importazioni e le copie in PostgreSQL usano `COPY`.
- Tutte le schede di amministrazione qui sopra, il debugger PL/pgSQL e gli strumenti di schema
  (editor di tabelle, diagramma ER, confronto).

**CockroachDB e YugabyteDB** si aprono con lo stesso driver, ma l'app capisce di che server si
tratta, e gli strumenti del server che si basano sulle viste proprie di PostgreSQL sono spenti:
niente schede sessioni, lock, dashboard, istruzioni, tabelle e indici, parametri, utenti o
manutenzione, e niente debugger. CockroachDB ha un proprio formato di piano (Explain ed Explain
analyze funzionano) e le importazioni passano come istruzioni `INSERT` invece di `COPY`. Una tabella
di CockroachDB senza chiave si modifica con il suo `rowid` nascosto. TimescaleDB (testato), Supabase,
Neon e AlloyDB (non testati sui servizi) per l'app sono PostgreSQL, con tutto ciò che sta sopra; ciò
che un servizio ospitato limita per il tuo ruolo è una sua regola.

## Agenti

Gli agenti di sola lettura e le connessioni di sola lettura aprono la sessione con
`default_transaction_read_only` (PostgreSQL, CockroachDB e YugabyteDB), così è il server stesso a
rifiutare le scritture; dietro un pooler in transaction mode c'è solo il controllo dell'istruzione.
`server_activity` e `top_queries` funzionano su PostgreSQL vero e proprio, non su CockroachDB né su
YugabyteDB. Vedi [Agenti e la CLI](../../AGENTS.md).

## Limiti noti

- Più vecchio di 12: l'app si connette e avvisa che alcune funzioni potrebbero non funzionare.
- Il debugger è spento dietro un pooler in transaction mode.
- La modalità terminale richiede `psql` nel PATH.

## Testato su

La suite di conformance gira su PostgreSQL 12, 14, 16 e 17, TimescaleDB 2.30 su PostgreSQL 17,
CockroachDB 26.2 e YugabyteDB 2025.2, più PgBouncer in transaction mode, un certificato client su
TLS reciproco, e il debugger su un server con `pldebugger`. Supabase, Neon e AlloyDB non sono
testati: sono il driver PostgreSQL con dei valori predefiniti.

Kerberos è testato con un ticket vero da un KDC vero (MIT Kerberos, nel profilo `kerberos` di
`docker/compose.yml`) su PostgreSQL 17, da un client Linux con GSSAPI: un ticket da `kinit`, un
keytab, TLS, `psql`, nessun ticket, un ticket scaduto e un nome di servizio sbagliato. **L'accesso
tramite SSPI di Windows non è stato provato con un ticket vero:** un computer Windows fuori da un
dominio non può raggiungere il KDC di prova senza cambiare le impostazioni di sistema (`ksetup`). Su
Windows si testa solo ciò che succede senza un ticket. Neanche una riga `sspi` del server
(PostgreSQL su Windows) è stata provata.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
