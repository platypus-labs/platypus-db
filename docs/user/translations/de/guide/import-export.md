<!-- source: 925ae39e7303 -->
[← Benutzerhandbuch](README.md)

# Import und Export

Die folgenden Aktionen stehen im Kontextmenü des Navigators (Rechtsklick) und in der Befehlspalette
(`Ctrl+K`). Die vollständige Liste der Formate und Optionen steht in
[FEATURES.md](../FEATURES.md#import-export-and-transfer).

## Eine Datei in eine Tabelle importieren

![Die Vorschau einer zu importierenden Datei](../../screenshots/guide/import-preview.png)

1. Klicke im Navigator mit der rechten Maustaste auf die Tabelle, dann auf **Daten** › **Daten
   importieren…**.
2. Wähle die Datei: CSV, TSV, JSON, NDJSON, Parquet oder Excel.
3. Prüfe die Vorschau und die Zuordnung der Spalten.
4. Wähle **Zeilen hinzufügen**, **Alle Zeilen ersetzen** oder **Zeilen mit demselben Schlüssel
   aktualisieren, die anderen hinzufügen**, dann klicke auf **Importieren**. Die letzte braucht einen
   Primärschlüssel in der Tabelle und wird bei ClickHouse nicht angeboten.

**Daten importieren…** wird bei Tabellen angeboten, nicht bei Views. Für eine neue Tabelle klicke mit
der rechten Maustaste auf das Schema und wähle **Daten in eine neue Tabelle importieren…**: Die Typen
sind die, die DuckDB aus der Datei ableitet, und einen Modus kannst du nicht wählen. **CSV als neue
Tabelle einfügen…** (gleiches Menü) importiert die Zwischenablage.

Der Import wird bei einer schreibgeschützten Verbindung nicht angeboten, bei einer, die die
Datenbearbeitung verbietet, und bei NoSQL-Datenbanken ebenfalls nicht.

## Tabellen in eine Datei exportieren

1. Klicke mit der rechten Maustaste auf eine Tabelle oder wähle mehrere aus, dann auf **Daten** ›
   **Daten exportieren…**. Für alle Tabellen eines Schemas klicke mit der rechten Maustaste auf das
   Schema und wähle **Alle Tabellen exportieren…**.
2. Wähle unter **Formatieren** das Format (CSV, JSON, NDJSON, Parquet, Excel, SQL `INSERT`, Markdown, HTML oder XML)
   und klicke auf **In Ordner exportieren…**: Auch bei einer Tabelle fragt die App nach einem Ordner
   und schreibt pro Tabelle eine Datei, nach ihr benannt.
3. Lege optional **Komprimierung** (gzip oder zstd; nicht für Parquet und Excel, die schon gepackt
   sind) und **Auch hochladen nach…** in einen Bucket (S3, GCS, R2 oder MinIO) fest.

Wenn du stattdessen das Ergebnis einer Abfrage exportierst (der Export des Ergebnisrasters), fragt der
Dialog nach einer einzelnen Datei, und der Upload kann auch an eine signierte URL gehen (S3 oder GCS).

CSV-Zellen, die eine Tabellenkalkulation als Formel ausführen würde, bekommen ein führendes `'`, außer
du schaltest **Formeln als Text, für Tabellenkalkulationen** aus. Eine Verbindung kann Exporte verbieten
(Seite **Darstellung und Sicherheit**); die Aktion wird dann nicht angeboten. Siehe
[Sicherheit](safety.md).

Um das Ergebnis einer Abfrage herauszunehmen, kopiere es aus dem Raster als CSV, JSON, Markdown, HTML,
SQL `INSERT` oder als `IN`-Liste.

## Eine Tabelle oder ein Schema in eine andere Datenbank kopieren

Klicke mit der rechten Maustaste auf eine Tabelle und wähle **Daten** › **In eine Datenbank
kopieren…** (damit ordnest du die Spalten zu), wähle mehrere Tabellen eines Schemas aus und nimm
**Daten** › **Tabellen in eine Datenbank kopieren…**, oder klicke mit der rechten Maustaste auf das
Schema und wähle **Schema in eine Datenbank kopieren…**. Die Typen werden zugeordnet. Das Ziel ist
eine SQL-Verbindung, die nicht schreibgeschützt ist und Datenbearbeitung erlaubt. Mehrere Tabellen
oder ein Schema zu kopieren ist für SQL-Quellen; eine MongoDB-Collection oder ein Redis-Schlüsseltyp
lässt sich in eine SQL-Datenbank kopieren. Nichts davon wird angeboten, wenn die Verbindung Exporte
verbietet.

## Eine SQL-Datei ausführen oder in eine sichern

Klicke mit der rechten Maustaste auf eine Verbindung oder eine Datenbank und wähle **SQL-Datei
ausführen…**: Es führt eine `.sql`-Datei (oder `.sql.gz`, `.sql.zst`) beliebiger Größe Anweisung für Anweisung aus,
mit Fortschritt und Stopp, standardmäßig in einer Transaktion, die ein Fehler zurückrollt. **In SQL-Datei sichern…**
(bei einem Schema oder bei Tabellen unter **Daten**) schreibt das DDL eines Schemas oder gewählter Objekte und auf Wunsch ihre Zeilen in eine Datei.

## Eine Übertragung für später speichern

Jeder Export-, Import- und Kopierdialog hat **Als Profil speichern…**. **Übertragungsprofile…**
(Befehlspalette) führt sie aus, benennt sie um, löscht und plant sie. Der geheime Schlüssel eines
Buckets wird nie in der Profildatei gespeichert.

## Eine Datendatei abfragen, ohne sie zu importieren

Ziehe eine CSV-, JSON-, Parquet- oder Excel-Datei auf das Fenster oder klicke auf dem
Willkommensbildschirm auf **Datendatei öffnen**: Sie öffnet sich als Datenbank (jedes Excel-Blatt ist
eine Tabelle). Beim ersten Mal bietet die App an, die Bibliothek von DuckDB herunterzuladen.

## Verbindungen importieren

Auf dem Willkommensbildschirm liest **Verbindungen importieren** DBeaver, DataGrip und pgAdmin (und
`pg_service.conf`); Db2-Verbindungen aus DBeaver und DataGrip und `jdbc:db2:`-URLs werden ebenfalls
gelesen. **Verbindungen exportieren** schreibt eine `.platypusconn`-Datei für einen anderen Computer:
siehe [Projekte und Teilen](projects-sharing.md).
