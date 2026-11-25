<!-- source: 728e6511c5f7 -->
[← Datenbanken](README.md)

# DuckDB

DuckDB läuft in der App, wie SQLite: kein Server, Host, Benutzer oder Passwort. Anders als bei SQLite bringt die
App die Bibliothek von DuckDB nicht mit; sie **lädt sie beim ersten Mal herunter**, wenn eine DuckDB-Datenbank
oder eine Datendatei geöffnet wird.

## Erste Verwendung: die Bibliothek von DuckDB

Wählst du **DuckDB** für eine neue Verbindung und die Bibliothek liegt nicht auf diesem Rechner, öffnet sich über
dem Formular ein Popup: *DuckDB herunterladen?* Es zeigt das Paket (DuckDB 2.0.0, aus dem Release von DuckDB auf
GitHub, MIT-Lizenz), seine Größe und seine Quelle und prüft den Download gegen eine SHA-256. **Herunterladen**
legt sie in den Datenordner der App, einmal für alle DuckDB-Datenbanken und -Dateien; **Abbrechen** lässt das
Formular benutzbar, und das Angebot kommt bei *Verbindung testen* und beim ersten Öffnen wieder.
*Einstellungen › Treiber* listet die Bibliothek auf und entfernt sie.

Die App lädt sie für Windows (x64, ARM64), Linux (x64, ARM64) und macOS (eine Bibliothek für beide Prozessoren)
herunter. Die App nutzt genau die Version, mit der sie gebaut wurde (2.0.x), und bietet diese an, wenn eine
andere installiert ist. Offline und ohne die Bibliothek lässt sich die Verbindung nicht öffnen.

## Verbinden

Wähle **DuckDB** und gib den Pfad der Datenbankdatei an, oder füge eine URL ein:

| Du fügst ein | Bedeutung |
|---|---|
| `duckdb:///home/ana/analytics.duckdb` | diese Datei, angelegt, wenn sie fehlt (ihr Ordner muss existieren) |
| `duckdb:///C:/data/analytics.duckdb` | eine Datei auf einem Windows-Laufwerk |
| `duckdb:///:memory:` | die In-Memory-Datenbank, die sich die Sitzungen der App teilen |
| `:memory:<name>` als Pfad | eine eigene In-Memory-Datenbank, über den Namen |

**Datendateien als Datenbanken.** Ein Pfad, der auf `.csv`, `.tsv`, `.json`, `.ndjson`, `.jsonl`, `.parquet`,
`.xlsx`, `.xlsm`, `.xls` oder `.ods` endet, öffnet sich als In-Memory-Datenbank auf dieser Datei (du kannst eine
Datei auch auf das Fenster ziehen). CSV, TSV, JSON, NDJSON und Parquet werden zu einer View mit dem Namen der
Datei, die DuckDB bei jeder Abfrage neu liest; jedes Blatt einer Arbeitsmappe wird zu einer Tabelle, einmal
geladen. Eine CSV oder TSV wird zuerst geprüft (jede Zeile muss so viele Werte haben wie die Kopfzeile), ihre
Textkodierung wird erkannt (oder du gibst sie an), und mit den Importoptionen kannst du fehlerhafte Zeilen
überspringen.

Erweiterungen werden weder von selbst heruntergeladen noch geladen; nur die in die Bibliothek eingebauten
(Parquet, JSON) sind da.

## Berechtigungen

Die des Betriebssystems: Lesezugriff auf die Datei, Schreibzugriff auf sie und ihren Ordner, um etwas zu ändern.
Ein Prozess zur Zeit darf eine DuckDB-Datei zum Schreiben öffnen; solange die App eine Datei offen hat, kann
also ein anderes Programm (etwa die Kommandozeile `duckdb`) sie nicht nutzen; die Sitzungen der App auf einer
Datei teilen sich eine Instanz. Der Terminalmodus wird deshalb nicht angeboten.

## Was funktioniert

- Datenbearbeitung im Raster, über den Primärschlüssel (oder einen eindeutigen Schlüssel auf Spalten ohne NULL
  oder einen virtuellen Schlüssel). Eine Datendatei wird in den Speicher gelesen: Eine CSV-, JSON- oder
  Parquet-View ist keine Tabelle und lässt sich nicht bearbeiten, und die Änderungen an den Blättern einer
  Arbeitsmappe werden nicht in die Datei zurückgeschrieben.
- Manueller Commit und Rollback; Abbrechen einer Anweisung (`duckdb_interrupt`).
- Explain zeigt den Plan (`EXPLAIN (FORMAT JSON)`); **Explain analyze** führt die Anweisung in einer
  Transaktion aus, die es zurückrollt, und wird abgelehnt, solange du eine Transaktion offen hast.
- Tab Parameter: die Einstellungen von DuckDB. Keine Tabs für Sitzungen, Sperren, Dashboard, Benutzer oder
  Anweisungen.
- Metadatensuche, Objekt-DDL, Schema-Tools. DuckDB ändert wenig an Ort und Stelle, daher legt das Ändern einer
  Tabelle sie neu an und kopiert ihre Zeilen.
- Importe laufen über den Appender von DuckDB, in der Transaktion der Sitzung.

## Agenten

Ein schreibgeschützter Agent und eine schreibgeschützte Verbindung öffnen die Datenbankdatei nur lesend, wenn
keine andere Sitzung der App sie schon offen hält; eine In-Memory-Datenbank oder eine Datendatei ist nie
schreibgeschützt, und die Prüfung der Anweisung hält dort Schreibzugriffe auf. Die DuckDB-Sitzungen der Agenten
können weder andere Dateien lesen oder schreiben noch Erweiterungen laden und schließen die Datei nach jedem
Aufruf, damit die App sie öffnen kann. `query_across_databases` läuft in einem eigenen DuckDB, das die App wie
dieses herunterlädt. Siehe [Agenten und die CLI](../../../../AGENTS.md).

## Bekannte Grenzen

- Excel-Dateien werden mit dem eigenen Leser der App gelesen, nicht mit der Excel-Erweiterung von DuckDB.
- Eine andere DuckDB-Version als 2.0.x wird nicht geladen.

## Getestet auf

Die Konformitätssuite läuft auf einer DuckDB-Datei, mit der Bibliothek, die zu den Bindings der App passt:
Transaktionen, Pläne, Tabellenbearbeitung, Sonderwerte, 128-Bit- und vorzeichenlose Zahlen und Zeiten mit
Zeitzone.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
