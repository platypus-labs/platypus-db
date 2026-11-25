<!-- source: 6f25d7181e09 -->
[← Datenbanken](README.md)

# MySQL und MariaDB

Ein Treiber (der Client `mysql_async`) öffnet MySQL 5.7 und neuer sowie MariaDB 10.6 und neuer. Beide
haben eine Kachel im Raster der Datenbanken; der Treiber erkennt, welchen Server er erreicht hat, und
zeigt ihn in der Verbindung (`Connected: MariaDB 11.4`). Es wird nichts heruntergeladen.

## Verbinden

| Du fügst ein | Gelesen als |
|---|---|
| `mysql://user:pass@host:3306/db?sslMode=REQUIRED` (oder `mariadb://`) | die URL-Form; Standardport 3306 |
| `jdbc:mysql://host:3307/db`, `jdbc:mariadb://…` | eine JDBC-URL |
| `Server=db;Port=3306;Database=shop;Uid=app;Pwd=…` | eine Connector/NET-Verbindungszeichenfolge, wenn sie auch `Driver=` enthält oder du sie in ein MySQL-Formular eingefügt hast |

URL-Parameter, die die App kennt: `sslmode` (auch als `ssl-mode`, `sslMode`, `ssl_mode` geschrieben),
`sslrootcert`, `sslcert`, `sslkey`, `application_name` und `connect_timeout`.

**TLS**: `disable` schaltet es aus. `prefer` (der Standard) nutzt TLS und fällt auf unverschlüsselt
zurück, wenn der Server keines hat oder nur Versionen anbietet, die der Client ablehnt (die Images von
MySQL 5.7 tun das); bei der Anmeldung mit einem Token fällt es nie zurück. `prefer` und `require`
prüfen das Zertifikat nicht. `verify-ca` prüft die Kette gegen `sslrootcert` und nicht den Hostnamen;
`verify-full` prüft beides. Die eigenen Namen von MySQL (`REQUIRED`, `VERIFY_IDENTITY`) werden
verstanden. Ein Client-Zertifikat braucht `sslcert` und `sslkey` zusammen.

**Die Anmeldung** läuft über ein Passwort oder über ein Token von AWS IAM, Google Cloud IAM oder
Microsoft Entra ID, das das Tool `aws`, `gcloud` oder `az` erzeugt; das Token geht über TLS, wie der
Server es verlangt. Kerberos (`auth_gssapi` von MariaDB, das Kerberos-Plugin von MySQL) wird noch nicht
unterstützt.

Die Verbindung ist immer TCP: Ein Unix-Socket oder eine benannte Pipe von Windows wird nicht genutzt.

## Berechtigungen

Zum Durchsuchen reicht, was die Anmeldung sehen kann. Die Verwaltungs-Tabs lesen:

- **Sitzungen**: die Prozessliste, aus `performance_schema.threads` bei MySQL 8 und aus
  `information_schema.PROCESSLIST` bei MariaDB, MySQL 5.7 oder einem Server mit ausgeschaltetem
  `performance_schema`. Wie viel von den Sitzungen anderer Benutzer der Server zeigt, ist die Regel des
  Servers, die die App nicht prüft.
- **Sperren**: `performance_schema.data_lock_waits`, `data_locks` und `metadata_locks` bei MySQL 8 (um
  die Wartezeiten anderer Sitzungen zu sehen, braucht es `PROCESS`; ohne es hat die Liste keine
  Wartezeiten und sagt das), `INNODB_LOCK_WAITS` in `information_schema` bei MariaDB und älterem MySQL.
- **Teuerste Anweisungen**: `performance_schema.events_statements_summary_by_digest`. Ist
  `performance_schema` aus, sagt der Tab, dass der Server damit gestartet werden soll.
- **Tabellen und Indizes**: `information_schema`, dazu `performance_schema`, um zu wissen, wie viel jeder
  Index gelesen wird; ohne es ist die Index-Nutzung unbekannt.
- **Anweisung abbrechen** und **Sitzung beenden** führen `KILL QUERY` und `KILL CONNECTION` aus.
- **Parameter** und **Dashboard**: Servervariablen und Statuszähler.
- **Benutzer und Rollen**: die Konten und Berechtigungen des Servers.
- **Wartung**: `ANALYZE`, `OPTIMIZE` und `CHECK TABLE`.
- **Sichern…** und **Wiederherstellen…** führen `mysqldump` und `mysql` aus, die im PATH liegen müssen.

## Was funktioniert

- Datenbearbeitung im Raster, über den Primärschlüssel (oder einen eindeutigen Schlüssel auf Spalten
  ohne NULL oder einen virtuellen Schlüssel).
- Manueller Commit und Rollback sowie das Abbrechen einer Anweisung auf dem Server.
- Explain: MySQL 8.0.16 und neuer liefern `EXPLAIN FORMAT=TREE`; älteres MySQL und MariaDB liefern
  `EXPLAIN FORMAT=JSON`. **Explain analyze** ist `EXPLAIN ANALYZE` ab MySQL 8.0.18 und
  `ANALYZE FORMAT=JSON` bei MariaDB; MySQL 5.7 und 8.0.0 bis 8.0.17 lehnen es ab.
- Jeder Verwaltungs-Tab oben und die Schema-Tools (Tabelleneditor, ER-Diagramm, Vergleich).
- Importe nutzen `LOAD DATA LOCAL`, wenn der Server es erlaubt (`local_infile` an), sonst
  `INSERT`-Anweisungen.
- Fehler nennen den Server, von dem sie kommen, z. B. `Unknown column 'x' in 'field list' (MySQL 5.7.44)`.

## Agenten

Schreibgeschützte Agenten und schreibgeschützte Verbindungen führen `SET SESSION TRANSACTION READ ONLY`
aus, und ihre Abfragen laufen in einer schreibgeschützten Transaktion. `server_activity` und
`top_queries` funktionieren. Eine `@variable`, temporäre Tabelle oder ein `SET` von MySQL oder MariaDB,
das ein Agent-Aufruf gemacht hat, ist im nächsten noch da. Siehe [Agenten und die CLI](../../AGENTS.md).

## Bekannte Grenzen

- Älter als MySQL 5.7 oder MariaDB 10.6: Die App verbindet sich mit einer Warnung, dass manche
  Funktionen vielleicht nicht gehen.
- Der Tabelleneditor speichert mit dem DDL von MySQL, das pro Anweisung committet: Ein fehlgeschlagenes
  Skript kann teilweise angewendet bleiben.
- Der Terminalmodus braucht `mysql` im PATH.

## Getestet auf

Die Konformitätssuite läuft auf MySQL 5.7, 8.0 und 8.4 sowie MariaDB 10.6 und 11.4, mit einem
Client-Zertifikat über gegenseitiges TLS auf MySQL 8.4 und mit Sitzungen, die der Server beendet hat.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
