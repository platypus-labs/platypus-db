<!-- source: b5b424fa73d7 -->
[← Datenbanken](README.md)

# ClickHouse

Der Treiber spricht mit der **HTTP-Schnittstelle** von ClickHouse und liest Ergebnisse im Arrow-Stream-Format.
Er ist Teil der App; es wird nichts heruntergeladen. Er durchsucht, fragt ab und entwirft Tabellen, bearbeitet
aber keine Zeilen im Raster.

## Verbinden

| Du fügst ein | Gelesen als |
|---|---|
| `clickhouse://default@ch.example.com/logs` | HTTP auf Port 8123, Datenbank `logs` |
| `clickhouse://ch.example.com/logs?sslmode=require` | HTTPS; ohne Port in der URL 8443 |
| `jdbc:clickhouse://host:8123/db` | eine JDBC-URL |
| `https://host:8443` in einem ClickHouse-Formular | die HTTP(S)-URL des Servers |

Nimm den **HTTP**-Port, nicht den nativen (9000). Der Benutzer ist `default`, wenn er leer ist; das Passwort
wird im Header `X-ClickHouse-Key` gesendet. Das Feld Datenbank führt nach dem Verbinden `USE <database>` aus.

**TLS** wird durch den Modus gewählt, da HTTP es nicht aushandeln kann: `disable` und `prefer` (der Standard)
verbinden per **einfachem HTTP**; `require` und die `verify`-Modi nutzen HTTPS. Bei HTTPS ist der Standardport
8443, wenn die URL keinen nennt; füllst du das Formular aus, tippe den Port selbst ein. `require` prüft das
Zertifikat nicht, `verify-ca` prüft die Kette, `verify-full` auch den Hostnamen; `sslrootcert` nennt die Datei
deiner CA, sonst werden die vertrauenswürdigen Zertifikate des Systems genutzt.

Eine Verbindung ist eine HTTP-Sitzung: `SET`, `USE` und temporäre Tabellen halten zwischen Anweisungen.

## Berechtigungen

Das Durchsuchen liest `system.databases`, `system.tables`, `system.columns` und
`system.data_skipping_indices`; die Anmeldung sieht dort, was ClickHouse ihr zeigt. Die Tabs lesen:

- **Parameter**: `system.settings` und `system.server_settings`.
- **Dashboard**: `system.metrics`, `system.events`, `system.parts` (Datenbankgrößen) und `system.replicas`;
  was die Anmeldung nicht lesen kann, zeigt „—“.
- **Benutzer und Rollen**: `system.users`, `system.roles`, `system.grants` und `system.role_grants`. Benutzer,
  die in den Konfigurationsdateien des Servers definiert sind, werden ebenfalls aufgelistet; der Server lehnt
  es ab, sie zu ändern.

## Was funktioniert

- Abfragen, Ergebnisse, der Navigator, Autovervollständigung, Objekt-DDL, Metadatensuche und der
  Tabellen-Designer (einschließlich der Engine-Klausel und der Skipping-Indizes).
- Explain zeigt den Plan von `EXPLAIN indexes = 1`; es gibt kein Explain analyze.
- Abbrechen stoppt die Anweisung auf dem Server mit `KILL QUERY`.
- Die Tabs Parameter, Dashboard und Benutzer und Rollen. Keine Tabs für Sitzungen, Sperren, Anweisungen oder
  Tabellen und Indizes.
- Exporte und Importe nach ClickHouse als `INSERT`-Anweisungen.

## Was nicht

- **Keine Datenbearbeitung im Raster**: Es sagt „ClickHouse-Tabellen werden nicht direkt bearbeitet“. Das
  Speichern würde die Zeilenzahl jeder Änderung innerhalb einer Transaktion prüfen, und ClickHouse hat weder
  Transaktionen noch Zeilenzahlen für Updates. Ändere Zeilen mit SQL.
- **Keine Transaktionen**: Die Symbolleiste für manuellen Commit ist ausgeblendet; jede Anweisung wirkt allein.
- Keine Fremdschlüssel oder Unique-Constraints (der Primärschlüssel ordnet die Daten nur).

## Agenten

Ein schreibgeschützter Agent und eine schreibgeschützte Verbindung senden jede Anweisung außer `SET` mit
`readonly = 1`, sodass der Server Schreibzugriffe und die Tabellenfunktionen ablehnt, die andere Server oder
seine Dateien erreichen (`url`, `s3`, `remote`, `file` und ähnliche). Die Anweisung eines Agenten wird
außerdem gegen eine Liste von Tabellenfunktionen geprüft, die nur lokale Daten lesen. Siehe
[Agenten und die CLI](../../../../AGENTS.md). Der Terminalmodus ist nicht verfügbar: `clickhouse-client` nutzt den
nativen Port.

## Bekannte Grenzen

- Einfaches HTTP, außer der Modus ist `require` oder stärker: Das standardmäßige `prefer` verschlüsselt nicht.
- Es wird nur die HTTP-Schnittstelle genutzt; ein Server, der nur den nativen Port anbietet, ist nicht
  erreichbar.

## Getestet auf

Die Konformitätssuite läuft auf ClickHouse 26.3. Ältere ClickHouse-Versionen sind nicht getestet, und die App
hat dafür keine Mindestversion.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
