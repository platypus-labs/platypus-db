<!-- source: a2b0bfe9784d -->
[← Datenbanken](README.md)

# PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB und der Rest

Ein Treiber, gebaut auf `tokio-postgres`, spricht das PostgreSQL-Protokoll. Er öffnet PostgreSQL 12 und
neuer sowie die Server und Dienste, die dasselbe Protokoll sprechen. Es wird nichts heruntergeladen: Der
Treiber ist Teil der App.

## Verbinden

Wähle **PostgreSQL** im Raster der Datenbanken oder füge eine URL ein: Das Formular füllt sich selbst
aus.

| Du fügst ein | Gelesen als |
|---|---|
| `postgres://user:pass@host:5432/db?sslmode=require` (oder `postgresql://`) | die URL-Form; der Standardport ist 5432 |
| `jdbc:postgresql://host/db?sslmode=require` | eine JDBC-URL, wie DBeaver und DataGrip sie aufbewahren |
| `host=db port=5432 dbname=shop user=app sslmode=require` | eine libpq-Verbindungszeichenfolge |
| `Host=db;Database=shop;Username=app;Password=…` | eine Npgsql-Verbindungszeichenfolge |

URL-Parameter, die die App kennt: `sslmode`, `sslrootcert`, `sslcert`, `sslkey`, `application_name`,
`connect_timeout` und `pgbouncer=true`. Jeder andere Parameter wird weggelassen, und das Formular sagt,
welcher. Eine Failover-Liste von Hosts (`h1:5432,h2:5432`) behält den ersten Host.

**TLS** folgt dem `sslmode` von libpq:

| Modus | Was passiert |
|---|---|
| `disable` | kein TLS |
| `prefer` (der Standard) | TLS, wenn der Server es anbietet, sonst unverschlüsselt; das Zertifikat wird nicht geprüft |
| `require` | TLS, Zertifikat nicht geprüft |
| `verify-ca` | TLS, die Zertifikatskette wird geprüft, der Hostname nicht |
| `verify-full` | TLS, Kette und Hostname werden geprüft |

Die Kette wird gegen die Datei in `sslrootcert` geprüft, falls es eine gibt, sonst gegen die
vertrauenswürdigen Zertifikate des Systems. Ein Client-Zertifikat (`sslcert` und `sslkey`) wird in
jedem Modus gesendet, der TLS nutzt.

**Die Anmeldung** läuft über ein Passwort oder über ein Token, das das Kommandozeilen-Tool einer Cloud
bei jeder Verbindung erzeugt: AWS IAM (`aws`), Google Cloud IAM (`gcloud`) und Microsoft Entra ID (`az`)
stehen in der Auswahl *Authentifizierung* der Verbindung. Das Tool muss auf deinem Rechner installiert
und angemeldet sein. Oder über **Kerberos**, siehe unten.

**Kerberos (GSSAPI).** Wähle *Kerberos-Ticket (GSSAPI)* unter *Password from*. Das Passwortfeld
verschwindet: Es wird kein Passwort abgefragt, gespeichert oder gesendet. Verlangt der Server eines
oder lässt er dich ein, ohne Kerberos zu verlangen, wird die Verbindung abgelehnt und sagt das.

- **Der Dienstname** ist `postgres`, wenn du nichts einträgst (das `krb_srvname` des Servers). Das
  angeforderte Ticket ist `service/host` in deinem Standard-Realm: Der Host muss der Name im Principal
  des Dienstes sein, keine IP-Adresse und kein Alias, da die App ihn nicht nachschlägt. Auch über einen
  SSH-Tunnel oder eine Portweiterleitung ist es der Name des Servers.
- **Der Benutzer** ist die PostgreSQL-Rolle, die der Server aus deinem Principal ableitet
  (`include_realm` und `pg_ident.conf`). Bleibt er leer, nimmt die App deinen Systembenutzernamen.
- **Das Ticket** kommt unter Linux von `kinit`, aus dem Cache, den `KRB5CCNAME` nennt (oder dem
  Standard-Cache), oder aus einer Keytab: Ist `KRB5_CLIENT_KTNAME` gesetzt, holt Kerberos das Ticket
  selbst. Diese Variablen müssen dort gesetzt sein, wo die App startet. Unter Windows ist es das Ticket
  deiner Windows-Sitzung.
- **Ohne Ticket** sagt der Test, was auszuführen ist: `kinit`, die beiden Variablen und `klist`, gefolgt
  vom eigenen Text von Kerberos. Dasselbe gilt für ein abgelaufenes Ticket, einen unbekannten Dienst und
  ein nicht erreichbares KDC.
- **`psql`, `pg_dump` und `pg_restore`** melden sich auf dieselbe Weise an (`PGKRBSRVNAME`,
  `PGREQUIREAUTH=gss` und kein `PGPASSWORD`). Ihre Kopie von libpq muss mit GSSAPI gebaut sein.
- **Welche Builds es haben.** Die Pakete für Windows (SSPI) und Linux (GSSAPI, MIT Kerberos). Die für
  macOS nicht: Noch kein Mac hat es gebaut, und das Formular sagt es. Das Cargo-Feature `kerberos` von
  `platypus-driver-pg` schaltet es ein; ohne es wird nichts von Kerberos gelinkt oder ausgeführt.
- **GSS-Verschlüsselung** (`hostgssenc`, `gssencmode`) wird nicht verwendet: Nimm TLS zur
  Verschlüsselung, mit `hostssl`- oder `host`-Zeilen. Ein `krbsrvname` in einer eingefügten URL wird
  weggelassen, wie jeder andere Parameter, den die App nicht kennt.

**Pooler.** Hinter PgBouncer, Supavisor oder dem Pooler von Neon im Transaktionsmodus wähle
*Transaktionsmodus (PgBouncer, Supavisor, Neon)* im Formular (oder hänge `pgbouncer=true` an die URL).
Anweisungen laufen dann unbenannt, weil die nächste Transaktion auf einer anderen Server-Sitzung liegen
kann. Das Formular schlägt es für Hosts vor, die auf `.pooler.supabase.com` enden, und für Neon-Hosts
mit `-pooler` im Namen. In diesem Modus ist der PL/pgSQL-Debugger aus, und die schreibgeschützten
Sitzungen der Agenten verlassen sich allein auf die Prüfung der Anweisung.

Die anderen Server haben eigene Kacheln, die nur Voreinstellungen setzen:

| Kachel | Voreinstellungen, die sie setzt |
|---|---|
| CockroachDB | Port 26257 |
| YugabyteDB | Port 5433, Benutzer und Datenbank `yugabyte` |
| TimescaleDB | keine |
| Supabase | Benutzer und Datenbank `postgres`, TLS `require` |
| Neon, AlloyDB | TLS `require` |

## Berechtigungen

Zum Durchsuchen reicht, was der Server der Anmeldung zeigt. Die Verwaltungs-Tabs lesen Folgendes:

- **Sitzungen** und **Sperren**: `pg_stat_activity` und `pg_locks`. Die App listet die Client-Sitzungen
  auf, die der Server liefert; wie viel von den Sitzungen und Anweisungen anderer Benutzer der Server
  der Anmeldung zeigt, ist die Regel des Servers, die die App nicht prüft. *Anweisung abbrechen* und
  *Sitzung beenden* rufen `pg_cancel_backend` und `pg_terminate_backend` auf; der Server entscheidet,
  ob die Anmeldung das darf.
- **Teuerste Anweisungen**: die Erweiterung `pg_stat_statements`. Ohne sie sagt der Tab, wie man sie
  einschaltet (zu `shared_preload_libraries` hinzufügen, neu starten, `CREATE EXTENSION`).
- **Tabellen und Indizes**: `pg_stat_user_tables` und `pg_stat_user_indexes`.
- **Parameter** und **Dashboard**: `pg_settings`, `pg_stat_database`, `pg_stat_activity` und die
  Replikations-Views; die Standbys werden nur einem Mitglied von `pg_monitor` aufgelistet. Eine Zahl,
  die die Anmeldung nicht lesen kann, zeigt „—“ und den Grund.
- **Benutzer und Rollen**: `pg_roles` und `pg_auth_members`; sie zu ändern braucht die Rechte, die die
  Anweisungen brauchen.
- **Wartung**: `VACUUM`, `ANALYZE` und `REINDEX` auf der Tabelle.
- **Sichern…** und **Wiederherstellen…** führen `pg_dump`, `pg_restore` und `psql` aus, die im PATH
  liegen müssen.
- **Debuggen…** braucht die Erweiterung `pldebugger` auf dem Server.

## Was funktioniert

- Datenbearbeitung im Raster, über den Primärschlüssel (oder einen eindeutigen Schlüssel auf Spalten
  ohne NULL oder einen virtuellen Schlüssel, den du festlegst). Arrays und zusammengesetzte Werte
  bearbeitest du pro Zeile ein Element oder Feld.
- Transaktionen mit manuellem Commit und auf dem Server abgebrochene Anweisungen.
- Explain und Explain analyze; der Plan wird aus dem JSON oder Text von PostgreSQL gelesen.
- Mehrere Datenbanken pro Verbindung, Partitionen, PostGIS-Geometrien, Arrays und zusammengesetzte
  Typen.
- Importe und Kopien nach PostgreSQL nutzen `COPY`.
- Jeder Verwaltungs-Tab oben, der PL/pgSQL-Debugger und die Schema-Tools (Tabelleneditor,
  ER-Diagramm, Vergleich).

**CockroachDB und YugabyteDB** öffnen sich mit demselben Treiber, aber die App erkennt, welcher Server
es ist, und die Server-Tools, die sich auf die eigenen Views von PostgreSQL stützen, sind aus: keine
Tabs für Sitzungen, Sperren, Dashboard, Anweisungen, Tabellen und Indizes, Parameter, Benutzer oder
Wartung, und kein Debugger. CockroachDB hat sein eigenes Planformat (Explain und Explain analyze
funktionieren), und Importe laufen als `INSERT`-Anweisungen statt als `COPY`. Eine CockroachDB-Tabelle
ohne Schlüssel wird über ihr verstecktes `rowid` bearbeitet. TimescaleDB (getestet), Supabase, Neon und
AlloyDB (nicht gegen die Dienste getestet) sind für die App PostgreSQL, mit allem Obigen; was ein
gehosteter Dienst für deine Rolle einschränkt, ist seine eigene Regel.

## Agenten

Schreibgeschützte Agenten und schreibgeschützte Verbindungen öffnen die Sitzung mit
`default_transaction_read_only` (PostgreSQL, CockroachDB und YugabyteDB), sodass der Server selbst
Schreibzugriffe ablehnt; hinter einem Pooler im Transaktionsmodus tut es nur die Prüfung der Anweisung.
`server_activity` und `top_queries` funktionieren bei PostgreSQL selbst, nicht bei CockroachDB oder
YugabyteDB. Siehe [Agenten und die CLI](../../AGENTS.md).

## Bekannte Grenzen

- Älter als 12: Die App verbindet sich und warnt, dass manche Funktionen vielleicht nicht gehen.
- Der Debugger ist hinter einem Pooler im Transaktionsmodus aus.
- Der Terminalmodus braucht `psql` im PATH.

## Getestet auf

Die Konformitätssuite läuft auf PostgreSQL 12, 14, 16 und 17, TimescaleDB 2.30 auf PostgreSQL 17,
CockroachDB 26.2 und YugabyteDB 2025.2, dazu PgBouncer im Transaktionsmodus, ein Client-Zertifikat über
gegenseitiges TLS und den Debugger auf einem Server mit `pldebugger`. Supabase, Neon und AlloyDB sind
nicht getestet: Sie sind der PostgreSQL-Treiber mit Voreinstellungen.

Kerberos ist mit einem echten Ticket von einem echten KDC getestet (MIT Kerberos, im Profil `kerberos`
von `docker/compose.yml`) auf PostgreSQL 17, von einem Linux-Client mit GSSAPI: ein Ticket von `kinit`,
eine Keytab, TLS, `psql`, kein Ticket, ein abgelaufenes Ticket und ein falscher Dienstname. **Die
Anmeldung über das SSPI von Windows wurde nicht mit einem echten Ticket ausprobiert:** Ein Windows-Rechner
außerhalb einer Domäne erreicht das Test-KDC nicht, ohne Systemeinstellungen zu ändern (`ksetup`). Unter
Windows ist nur getestet, was ohne Ticket passiert. Auch eine `sspi`-Zeile eines Servers (PostgreSQL unter
Windows) wurde nicht ausprobiert.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
