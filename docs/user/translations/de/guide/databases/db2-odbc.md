<!-- source: 1206da228de0 -->
[← Datenbanken](README.md)

# IBM Db2 und ODBC (Windows)

Db2 und jede Datenbank mit einem ODBC-Treiber oder einer Datenquelle (DSN) werden über den ODBC-Treibermanager
des Systems erreicht. Das funktioniert vorerst **nur unter Windows**: Unter macOS und Linux sagt eine Db2- oder
ODBC-Verbindung, dass ODBC-Treiber nur unter Windows erreichbar sind.

## Erste Verwendung: der ODBC-Treiber von IBM (Db2)

Wählst du **IBM Db2** für eine neue Verbindung und auf diesem Rechner ist kein ODBC-Treiber von IBM
registriert, öffnet sich über dem Formular ein Popup: *IBM Db2 benötigt IBM Data Server Driver for ODBC and
CLI*. Es zeigt das Paket (Version 12.1.4), seine Größe, die Bedingungen von IBM und woher es kommt, und prüft
den Download gegen eine SHA-256. **Herunterladen und installieren** holt es in den Datenordner der App und
registriert es mit `db2cli install -setup`; **Windows verlangt dafür Administratorrechte
(Benutzerkontensteuerung)**. **Abbrechen** lässt das Formular benutzbar, und das Angebot kommt bei
*Verbindung testen* und beim ersten Öffnen wieder.

Ein Treiber, der schon im System ist, zählt: Die App sucht einen namens `IBM DB2 ODBC DRIVER…` oder
`IBM DATA SERVER…`, egal ob die eigene Registrierung oder ein vollständiger IBM-Client ihn angelegt hat, und
nimmt den neuesten nach Namen. Die Option `driver` der Verbindung ersetzt diese Wahl. *Einstellungen › Treiber*
listet den heruntergeladenen Treiber auf und entfernt ihn und zeigt die ODBC-Treiber und Datenquellen des
Systems.

## Verbinden

**IBM Db2**: Host, Port (standardmäßig 50000), **Datenbank** (Host und Datenbank sind beide Pflicht), Benutzer
und Passwort. Die App baut die Verbindungszeichenfolge selbst (`DATABASE`, `HOSTNAME`, `PORT`, `PROTOCOL=TCPIP`,
`UID`, `PWD`). TLS: Die Modi `disable` und `prefer` verbinden ohne; `require` und darüber fügen `Security=SSL`
hinzu, und `sslrootcert` wird zu `SSLServerCertificate`. Db2-Server hören auf TLS an einem anderen Port als dem
einfachen, setze also den Port. Durch einen SSH-Tunnel wird das lokale Ende des Tunnels genutzt.

**Plattform** wählt die Art des Db2-Servers:

- *Db2 for Linux, UNIX and Windows (LUW)*, der Standard, auch für alte 8.x-Server.
- *Db2 for z/OS*: derselbe IBM-Treiber, standardmäßig Port 446, und die **Datenbank** ist der Location-Name. Der
  Treiber von IBM erreicht z/OS nur mit einer Db2-Connect-Lizenz auf diesem Rechner.
- *Db2 for IBM i* (AS/400): der eigene *IBM i Access ODBC Driver* von IBM (IBM i Access Client Solutions, von IBM
  installiert, nicht von der App; der ältere iSeries-Access-Treiber funktioniert auch). Der Host ist der Name des
  Systems, die Datenbank ist optional und setzt die Standardbibliothek, es gibt keinen Port, und TLS fügt `SSL=1`
  hinzu. Es geht nicht durch einen SSH-Tunnel, da der Treiber mehrere Ports des Servers öffnet.

**Anderer ODBC-Treiber** (die generische Wahl): Gib eines an von

- einer **Datenquelle (DSN)**, die das System kennt, getippt oder aus den Vorschlägen genommen;
- einem **Treiber**-Namen (aus den ODBC-Treibern des Systems vorgeschlagen) mit einer **Verbindungszeichenfolge**,
  die den Rest enthält, etwa `Server=host;Database=sales`;
- einer **Verbindungszeichenfolge** allein, so verwendet, wie sie geschrieben ist (sie kann ihr eigenes
  `DRIVER=` nennen).

Die Felder Benutzer und Passwort werden als `UID` und `PWD` hinzugefügt, außer die Zeichenfolge hat sie schon.
Eine eingefügte ADO.NET- oder ODBC-Zeichenfolge mit `Driver={…}` wird in das passende Formular gelesen, wenn der
Treiber SQL Server, PostgreSQL, MySQL/MariaDB oder ClickHouse ist; jeder andere Treiber oder ein `DSN=` wird zu
einer generischen ODBC-Verbindung mit der Zeichenfolge, wie sie ist.

## Berechtigungen

Was immer die Datenbankanmeldung hat. Der Navigator nutzt die Katalogaufrufe von ODBC (`SQLTables`,
`SQLColumns`, `SQLPrimaryKeys`, `SQLForeignKeys`). Es gibt keine Verwaltungs-Tabs.

## Was funktioniert

- Abfragen und Ergebnisse, mit den Typen, die der Treiber meldet.
- Manueller Commit und Rollback.
- Abbrechen: Der Cancel-Aufruf von ODBC ist nicht erreichbar, daher wird eine Anweisung, die noch nicht
  gestartet ist, mit ihrer Sitzung aufgegeben (die App öffnet eine neue); eine, die Zeilen liefert, stoppt,
  wenn ihr Stream abgebrochen wird.
- Der Navigator, aus den Katalogaufrufen. SQL für Db2 wird mit dem Quoting von Db2 geschrieben (einfache Namen
  werden in Großbuchstaben gefaltet, Zeilen werden mit `FETCH FIRST` begrenzt); der generische Treiber schreibt
  Standard-SQL.

## Was nicht

- Kein Explain, keine Objekt-DDL, keine Verwaltungs-Tabs, kein Bulk-Laden (Importe nutzen `INSERT`).
- **Text- und Binärwerte über 64 KiB sind ein Fehler**, statt eines riesigen Puffers.
- **Datenbearbeitung im Raster** ist bei diesen Datenbanken nicht getestet. Das Raster blockiert sie nicht,
  aber ohne Savepoints rollt ein fehlgeschlagenes Speichern im Modus mit manuellem Commit die ganze Transaktion
  der Sitzung zurück und sagt das.
- Der Terminalmodus ist nicht verfügbar.

## Agenten

Schreibgeschützte Agenten verlassen sich allein auf die Prüfung der Anweisung. Siehe
[Agenten und die CLI](../../AGENTS.md).

## Bekannte Grenzen

- Nur Windows.
- Db2 hat keinen Container im Testaufbau, daher werden die Verbindungszeichenfolge von Db2 und die
  Treibererkennung nur durch Unit-Tests geprüft.

## Getestet auf

Der generische ODBC-Treiber läuft gegen SQL Server 2022 über den eigenen Treiber `SQL Server` von Windows: Typen,
lange Ergebnisse, Fehler mit ihrem SQLSTATE, Metadaten, Commit und Rollback, Abbrechen einer wartenden
Anweisung, die eigenen Literale und Typen von SQL Server und Importe. IBM Db2 ist nicht gegen einen Server
getestet.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
