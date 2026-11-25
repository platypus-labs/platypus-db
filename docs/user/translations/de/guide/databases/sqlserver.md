<!-- source: 3830a0c639a7 -->
[← Datenbanken](README.md)

# SQL Server und Azure SQL

Der Treiber spricht TDS selbst (ein angepasstes `tiberius`), daher gibt es nichts zu installieren oder
herunterzuladen. Er ist für SQL Server 2017 (14.0) und neuer geschrieben und für Azure SQL (nicht gegen
den Dienst getestet: siehe das Ende dieser Seite).

## Verbinden

| Du fügst ein | Gelesen als |
|---|---|
| `sqlserver://sa:pass@host:1433/master?encrypt=true&trustServerCertificate=true` (oder `mssql://`) | die URL-Form; Standardport 1433 |
| `jdbc:sqlserver://host:1433;databaseName=shop;encrypt=true` | eine JDBC-URL (Treiber von Microsoft, jTDS oder das alte `jdbc:microsoft:`) |
| `Server=tcp:db.example.com,1433;Database=shop;User Id=sa;Password=…;Encrypt=True` | eine ADO.NET- oder ODBC-Verbindungszeichenfolge |

In einer URL nennt `database` oder `databaseName` die Datenbank, und `encrypt` und
`trustServerCertificate` werden so gelesen, wie die JDBC- und ADO.NET-Zeichenfolgen sie lesen (außer
`sslmode` ist ebenfalls angegeben).

**Benannte Instanzen** (`host\SQLEXPRESS`) werden nicht über den SQL Server Browser nachgeschlagen: Gib
den Port der Instanz an. Eine eingefügte Zeichenfolge mit einer Instanz und ohne Port sagt das.

**TLS**: `disable` schaltet die Verschlüsselung aus. `prefer` (der Standard) verschlüsselt und prüft das
Zertifikat nicht (Server haben oft ein selbstsigniertes) und fällt auf unverschlüsselt zurück, wenn der
Server keine Verschlüsselung hat. `require` verschlüsselt ohne Prüfung. `verify-ca` und `verify-full`
prüfen beide die Kette **und** den Hostnamen, weil die TLS-Bibliothek den Namen immer prüft; verweise
`sslrootcert` auf die Datei deiner CA, sonst werden die vertrauenswürdigen Zertifikate des Systems
genutzt.

**Die Anmeldung**:

- *SQL-Server-Anmeldung*: Benutzer und Passwort.
- *Windows-Anmeldung* (`Integrated Security=SSPI` in einer eingefügten Zeichenfolge): nur, wenn
  PlatypusDB unter Windows läuft; anderswo ist es ein Fehler.
- *Microsoft Entra ID*: ein Token, das das Tool `az` erzeugt, das installiert und angemeldet sein muss.
  So wird die Entra-Anmeldung von Azure SQL erreicht.

**Azure SQL**: Das Gateway kann den Client zum eigenen Knoten der Datenbank schicken; die App folgt einer
solchen Weiterleitung, außer durch einen SSH-Tunnel, der nur zu einer Adresse führt.

## Berechtigungen

Zum Durchsuchen reicht, was die Anmeldung sehen kann. Die Verwaltungs-Tabs lesen:

- **Sitzungen** und **Sperren**: `sys.dm_exec_sessions`, `dm_exec_requests`, `dm_exec_connections` und
  `dm_tran_locks`. Um zu sehen, was andere Anmeldungen tun, braucht die Anmeldung `VIEW SERVER STATE`.
- **Teuerste Anweisungen**: `sys.dm_exec_query_stats`, der Plan-Cache.
- **Tabellen und Indizes**: `sys.tables`, `dm_db_partition_stats`, `dm_db_index_usage_stats` und
  `sys.stats`.
- **Parameter**: `sys.configurations`; *Im Editor öffnen* schreibt eine `sp_configure`-Anweisung.
- **Dashboard**: Leistungsindikatoren, die Größen aus `sys.databases` und `sys.master_files` und
  Verfügbarkeitsgruppen, wo es welche gibt; eine Zahl, die die Anmeldung nicht lesen kann, zeigt „—“.
- **Benutzer und Rollen**: die Anmeldungen, Rollen und Berechtigungen des Servers.
- **Wartung**: `UPDATE STATISTICS`, `ALTER INDEX ALL … REBUILD` und `DBCC CHECKTABLE`.

## Was funktioniert

- Datenbearbeitung im Raster, über den Primärschlüssel (oder einen eindeutigen Schlüssel auf Spalten
  ohne NULL oder einen virtuellen Schlüssel).
- Manueller Commit und Rollback; das Abbrechen einer laufenden Anweisung (eine TDS-Attention) behält die
  Sitzung, ihre Transaktion und ihre temporären Tabellen.
- Mehrere Ergebnismengen aus einem Batch und `PRINT`-Meldungen.
- Mehrere Datenbanken pro Verbindung: Eine Verbindung, die ihre Datenbank nennt, zeigt nur diese;
  *Alle Datenbanken anzeigen* listet die übrigen auf.
- Explain zeigt den **geschätzten** Plan (`SHOWPLAN`). Explain analyze wird abgelehnt.
- *Sitzung beenden* ist `KILL`. Die Anweisung einer anderen Sitzung abzubrechen ist bei SQL Server nicht
  möglich, daher bietet der Tab nur an, die Sitzung zu beenden.
- Importe nutzen den Bulk-Copy von SQL Server (`OPENJSON`-Inserts für Tabellen mit `money`, `xml` oder
  Legacy-Spalten).
- Schema-Tools (Tabelleneditor, ER-Diagramm, Vergleich), auch für berechnete und mit Sortierung versehene
  Spalten, reichhaltige Indizes und temporale Tabellen. XML- und räumliche Indizes bleiben im Vergleich
  außen vor.
- Der Terminalmodus führt `sqlcmd` aus, falls es im PATH liegt.

## Agenten

Bei SQL Server hält allein die Prüfung der Anweisung Schreibzugriffe schreibgeschützter Agenten auf (keine
schreibgeschützte Sitzung auf dem Server); die Transaktion eines Agenten wird zurückgerollt.
`server_activity` und `top_queries` funktionieren. Siehe [Agenten und die CLI](../../AGENTS.md).

## Bekannte Grenzen

- Älter als 2017: Die App verbindet sich und warnt, dass manche Funktionen vielleicht nicht gehen.
- Kein Explain analyze und kein Nachschlagen benannter Instanzen.

## Getestet auf

Die Konformitätssuite läuft auf SQL Server 2017 und 2022 (Container), und der generische ODBC-Treiber wird
gegen denselben 2022-Container ausprobiert. Azure SQL ist nicht getestet: Seine Regeln für Weiterleitung,
Entra-Anmeldung und Verschlüsselung stehen im Code, wurden aber nicht gegen den Dienst ausgeführt.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
