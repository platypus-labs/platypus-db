<!-- source: fd0bf4386fb3 -->
[← Datenbanken](README.md)

# Oracle

Der Treiber nutzt die Client-Bibliothek von Oracle (ODPI-C, über das Crate `oracle`), die beim Verbinden
geladen wird. Die Bibliothek ist Oracle Instant Client, den die App bei der ersten Verwendung
herunterlädt.

## Erste Verwendung: Instant Client

Wählst du **Oracle** für eine neue Verbindung und der Client liegt nicht auf diesem Rechner, öffnet sich
über dem Formular ein Popup: *Oracle benötigt Oracle Instant Client*. Es zeigt das Paket (Basic Light
23.26.4), seine Größe, seine Lizenz (Oracle Free Use Terms and Conditions) und woher es kommt, und prüft
den Download gegen eine SHA-256. **Herunterladen** holt ihn in den Datenordner der App; **Abbrechen**
lässt das Formular benutzbar, und das Angebot kommt bei *Verbindung testen* und beim ersten Öffnen
wieder. *Einstellungen › Treiber* listet den Client auf und entfernt ihn.

- Die App lädt ihn unter Windows (x64) und Linux (x64 und ARM64) herunter. Unter macOS lädt sie nichts
  herunter: Installiere Instant Client von Oracle und lege seinen Ordner in den Bibliothekspfad.
- Hat die App keinen heruntergeladen, nutzt sie einen Instant Client oder vollen Client, den das System
  laden kann.
- Windows braucht das Microsoft Visual C++ Redistributable und Linux die Bibliothek `libaio` (`libaio1`,
  `libaio1t64` oder `libaio`, je nach Distribution); lässt sich der Client nicht laden, nennt der Fehler
  das fehlende Stück.

## Verbinden

Es gibt keine `oracle://`-URL; füge stattdessen eine JDBC-URL ein, etwa
`jdbc:oracle:thin:@host:1521/service` (Thin- und OCI-Formen werden gelesen), oder fülle das Formular
aus:

- **Host** und **Port** (Standard 1521) und **Dienstname** im Feld Datenbank (zum Beispiel `FREEPDB1`).
  Sie werden als Easy-Connect-Zeichenfolge geschrieben, `host:port/service`.
- **Ohne Host** ist das Feld Datenbank ein Net-Service-Name aus `tnsnames.ora` oder ein ganzer
  Connect-Descriptor, so geschrieben, wie er ist.
- **Benutzer und Passwort.** Ohne Benutzer meldet das Konto des Betriebssystems oder ein Oracle-Wallet an.
  Der Benutzer kann auf die SQL*Plus-Art geschrieben werden, `sys as sysdba`, oder du wählst die
  **Rolle** (`SYSDBA`, `SYSOPER`, `SYSBACKUP`, `SYSDG`, `SYSKM`, `SYSASM`).
- **TLS**: Die Modi `disable` und `prefer` nutzen einfaches TCP; `require` und darüber nutzen `tcps://` mit
  den vertrauenswürdigen Zertifikaten des Systems. `require` überspringt den Abgleich des Servernamens;
  `verify-ca` und `verify-full` prüfen ihn. Der Verbindungs-Timeout gilt. Durch einen SSH-Tunnel wird das
  lokale Ende des Tunnels genutzt.

## Berechtigungen

Das Durchsuchen liest die `ALL_*`-Views, daher sieht die Anmeldung die Objekte, auf die sie Rechte hat.
Abbrechen sendet einen Break; läuft die Anweisung nach ein paar Sekunden noch, bricht die App sie aus
einer zweiten Sitzung mit `ALTER SYSTEM CANCEL SQL` ab, was dieses Recht braucht, und gibt andernfalls die
Sitzung auf (ihre Transaktion wird zurückgerollt und eine neue Verbindung geöffnet).

## Was funktioniert

- Datenbearbeitung im Raster, über den Primärschlüssel (oder einen eindeutigen Schlüssel auf Spalten
  ohne NULL oder einen virtuellen Schlüssel).
- Manueller Commit und Rollback.
- PL/SQL-Blöcke: Ein Skript beendet einen Block mit einer `/`-Zeile. Der Text von `DBMS_OUTPUT` kommt als
  Meldungen zurück.
- Navigator (Tabellen, Views, materialisierte Views, Sequenzen, Synonyme, Funktionen, Prozeduren,
  Trigger, Typen und Packages), Objekt-DDL und Schema-Tools.
- Explain zeigt den Plan aus `EXPLAIN PLAN` und `DBMS_XPLAN`, nur geschätzt: Es gibt kein Explain
  analyze.
- Kompilierfehler, Zahlen und Nanosekunden-Zeiten, JSON, Vektoren, Cursor, lange LOBs und Dateispalten
  werden so zurückgelesen, wie die Datenbank sie hält.
- Keine Tabs für Sitzungen, Sperren, Dashboard, Anweisungen, Benutzer oder Parameter.
- Der Terminalmodus ist nicht verfügbar.

## Agenten

Schreibgeschützte Agenten verlassen sich allein auf die Prüfung der Anweisung: Oracle hat hier keine
schreibgeschützte Sitzung auf dem Server. Bei einer Anweisung, die ein Oracle-Package aufruft, etwa
`UTL_HTTP`, wird vor der Ausführung nachgefragt, wie bei einer Änderung. Siehe
[Agenten und die CLI](../../AGENTS.md).

## Bekannte Grenzen

- Die App vergleicht die Version des Servers nicht mit einem Minimum; der Client, den sie herunterlädt, ist
  23.26.4.
- Für einen Server älter als 12 liest der Navigator das Wörterbuch anders (kein `ORACLE_MAINTAINED`, kein
  `FETCH FIRST`). In den Tests läuft nur Oracle Free 23.26, daher ist dieser Pfad ungeprüft. Autonomous
  Database und andere Oracle-Clouddienste sind ebenfalls nicht getestet.
- Instant Client wird unter macOS nicht heruntergeladen.

## Getestet auf

Die Konformitätssuite läuft auf Oracle Free 23.26 (Container `gvenzl/oracle-free`), mit heruntergeladenem
Instant Client oder dem des Systems: Typen, lange Ergebnisse, Fehler, die auf den Fehler zeigen, PL/SQL,
Navigator, Commit und Rollback, das Speichern vieler bearbeiteter Zeilen, Abbrechen (auch eines
PL/SQL-Sleeps), Pläne, DDL, das sich zurückspielen lässt, Verbinden als `SYSDBA` und Schemavergleich.
Andere Versionen sind nicht getestet.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
