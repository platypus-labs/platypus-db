<!-- source: 6fd0f54f48e3 -->
[← Datenbanken](README.md)

# Snowflake, BigQuery, Databricks, Trino und andere ADBC-Treiber

Diese Datenbanken werden über **ADBC**-Treiber (Arrow Database Connectivity) erreicht, Bibliotheken, die die
App beim Verbinden lädt. Snowflake, BigQuery, Databricks und Trino haben je eine Kachel; jede andere
Datenbank mit einem ADBC-Treiber verbindet sich über **Anderer ADBC-Treiber**.

## Erste Verwendung: der Treiber-Download

Die vier genannten Datenbanken brauchen ihren Treiber auf diesem Rechner. Wählst du eine für eine neue
Verbindung und der Treiber ist nicht installiert, öffnet sich über dem Formular ein Popup: *Snowflake
benötigt einen Treiber* (und so weiter). Es sagt, dass es ein ADBC-Treiber ist, einmal für alle Verbindungen
dieser Datenbank heruntergeladen, und zeigt das Paket, seine Version, seine Lizenz und woher es kommt: die
[dbc-Treiber-Registry](https://docs.columnar.tech/dbc/) (Columnar). **Herunterladen** holt es, prüft seine
SHA-256 (ein Paket ohne eine wird abgelehnt) und bewahrt es bei den Daten der App auf; **Abbrechen** lässt das
Formular benutzbar, und das Angebot kommt bei *Verbindung testen* und beim ersten Öffnen wieder.

Das Angebot liest nur, was auf diesem Rechner liegt, daher erscheint es auch offline; der Download braucht das
Netzwerk. *Einstellungen › Treiber* listet die installierten Treiber auf und entfernt sie, sucht einmal am Tag
(oder bei *Nach Updates suchen*) nach neueren Versionen, installiert oder wechselt mit *Versionen…* zu einer
anderen Version und kann Updates selbstständig installieren. Nach einer Änderung werden die gespeicherten
Verbindungen, die den Treiber nutzen, ausprobiert (verbinden, Version, Schemas, `SELECT 1`), mit *Zurück zu
<Version>*, falls eine fehlschlägt. Die Wahl eines anderen Treibers lädt nichts herunter: Du verweist auf eine
Bibliotheksdatei.

## Verbinden

Jede Datenbank hat ihre eigenen Felder:

| Datenbank | Felder |
|---|---|
| Snowflake | **Konto** (zum Beispiel `xy12345.eu-west-1`), Benutzer, Passwort, Warehouse, Rolle, Datenbank, Schema |
| BigQuery | **Projekt**, Dataset und eine Dienstkonto-Schlüsseldatei (JSON); leer heißt Application Default Credentials von Google. Kein Passwort |
| Databricks | **Server-Hostname**, **HTTP-Pfad**, Katalog, Schema; das Passwortfeld ist das **Zugriffstoken** |
| Trino | **Host**, Port (standardmäßig 8080), **Benutzer**, Katalog, Schema, Passwort (optional) und TLS |
| Anderer ADBC-Treiber | die Datei der **Treiberbibliothek**, ein optionales Symbol als **Einstiegspunkt**, eine URI, Benutzer und Passwort; jede weitere Option, die du hinzufügst, wird an den Treiber übergeben |

Die **TLS**-Auswahl bei Trino ist *Automatisch* (HTTPS auf Port 443, sonst HTTP), *Aus* (HTTP), *An, Zertifikat
prüfen* und *An, ungeprüft*.

**Anderer ADBC-Treiber**: Die Bibliothek ist die Treiberdatei, die du heruntergeladen oder gebaut hast. Der
Einstiegspunkt wird nur für eine Bibliothek gebraucht, die `AdbcDriverInit` nicht exportiert (bei der
ADBC-Bibliothek von DuckDB ist es `duckdb_adbc_init`). Die URI und die Optionen sind, was dieser Treiber
dokumentiert. Eine fehlende Datei oder eine, die kein Treiber ist, wird beim Verbinden gemeldet.

## Berechtigungen

Was immer das Konto oder Token beim Dienst hat. Der Navigator liest Kataloge, Schemas, Tabellen und Spalten
mit dem Metadatenaufruf von ADBC.

## Was funktioniert

- Abfragen, Ergebnisse, der Navigator und Autovervollständigung. Das Ergebnis kommt als Arrow-Batches.
- Abbrechen: Die App bittet den Treiber abzubrechen; Treiber in Go brechen nur ihre eigenen Aufrufe ab, daher
  wird eine Anweisung, die nicht stoppt, mit ihrer Sitzung aufgegeben, und die App öffnet eine neue.
- Objekt-DDL nur bei **Trino** (`SHOW CREATE`). Snowflake, BigQuery, Databricks und andere Treiber zeigen kein
  DDL.
- Fremdschlüssel, die auf eine Tabelle zeigen, werden nicht aufgelistet (dafür müssten die Schlüssel jeder
  Tabelle gelesen werden).
- Exporte und Importe von Dateien (typisierte Importe sind bei Trino getestet).

## Was nicht

- **Kein manueller Commit**: Jede Anweisung committet von selbst, und das Ausschalten von Autocommit sagt, dass
  manueller Commit mit einem ADBC-Treiber nicht unterstützt wird.
- **Kein Explain** und keine Verwaltungs-Tabs.
- **Keine Datenbearbeitung im Raster**: Ein Speichern prüft jede Änderung in einer Transaktion, und diese
  Verbindungen haben keine. Ergebnisse bleiben schreibgeschützt; nimm SQL für Änderungen.
- Der Terminalmodus ist nicht verfügbar.

## Agenten

`run_query`, `sample_rows` und die anderen Tools funktionieren, im Rahmen der Prüfungen. Ohne
schreibgeschützte Sitzung auf dem Server verlassen sich schreibgeschützte Agenten allein auf die Prüfung der
Anweisung. Anweisungen, die außerhalb davon wirken, werden vor der Ausführung nachgefragt: `query()` bei Trino
und die `SYSTEM$`-Funktionen von Snowflake. Siehe [Agenten und die CLI](../../AGENTS.md).

## Bekannte Grenzen

- Eine Anmeldung bei einem Cloud-Warehouse, die nicht zu den obigen Feldern passt (Schlüsselpaar, Single
  Sign-on), steht nicht im Formular; der generische Treiber nimmt die eigenen Optionen des Treibers.
- Snowflake, BigQuery und Databricks stehen im Code und teilen die Treiberschicht mit Trino, haben aber keinen
  Test gegen die echten Dienste.

## Getestet auf

Die Konformitätssuite führt **Trino 484** über seinen heruntergeladenen Treiber aus (Typen, lange Ergebnisse,
Abbrechen, Metadaten, Fehler, Anweisungen, die mit einem Abschlusszeichen enden, typisierte Importe sowie Zeilen
und UUIDs) und den **generischen ADBC-Treiber** auf dem ADBC-Treiber von Apache für SQLite und auf dem von
DuckDB. Snowflake, BigQuery und Databricks sind nicht getestet.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
