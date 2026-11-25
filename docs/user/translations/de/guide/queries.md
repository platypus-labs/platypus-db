<!-- source: d4ffd9bf1ac2 -->
[← Benutzerhandbuch](README.md)

# Abfragen ausführen und Ergebnisse lesen

![JOIN-Vervollständigung aus den Fremdschlüsseln](../../screenshots/autocomplete.png)

## Eine Anweisung ausführen

1. Doppelklicke auf eine Verbindung (oder eine Tabelle), um einen SQL-Editor zu öffnen.
2. Tippe eine Anweisung. `Ctrl+Space` vervollständigt Schlüsselwörter, Tabellen und Spalten; `JOIN `
   bietet die verwandten Tabellen aus den Fremdschlüsseln an.
3. Drücke `Ctrl+Enter`, um die Anweisung unter dem Cursor (oder die Auswahl) auszuführen, oder `Alt+X`,
   um das ganze Skript auszuführen. `Ctrl+\` führt in einem neuen Ergebnis-Tab aus.
4. `Ctrl+.` bricht eine laufende Anweisung ab.

Anweisungen enden bei einem `;` oder bei einer Leerzeile vor einem Wort, das eine neue beginnt. Wenn
eine Anweisung fehlschlägt, wird der Fehler im Text markiert, und das Fehlerpanel bietet **Zum
Fehler** an. `Ctrl+Shift+F` formatiert das Skript; `Ctrl+S` speichert es als `.sql`-Datei.

## Parameter und Variablen

`:id` und `?` in einer Anweisung sowie `${name}`-Variablen werden vor dem Ausführen in einem Dialog
abgefragt und in einem Panel Variablen aufbewahrt. Ein `${name}`-Wert geht unverändert ins SQL, daher
wird eine Anweisung mit Variablen ausgefüllt noch einmal geprüft (siehe [Sicherheit](safety.md)).

## Die Ergebnisse lesen

- Das Raster liest Zeilen seitenweise. Klicke auf eine Spaltenüberschrift, um zu sortieren: Das
  schreibt das `ORDER BY` in den Editor und führt erneut aus; ein zweiter Klick kehrt es um, ein
  dritter stellt das eigene der Abfrage wieder her.
- Öffne eine Tabelle im Navigator, um sie zu durchsuchen; ein im Editor getipptes `WHERE` filtert sie.
- `F7` öffnet den Wertebereich: langer Text, JSON und XML formatiert, Hex und Bilder aus Binärspalten.
- Eine Zelle, die eine Datei enthält (ein PDF, ein Bild, ein ZIP und andere, an ihren Bytes erkannt),
  zeigt ihren Typ und ihre Größe, etwa `PDF · 2.3 MB`. Das Rastermenü bietet **PDF speichern…** (oder
  den Typ der Datei) und, bei mehreren Zellen einer Spalte, **N Dateien in einen Ordner speichern…**.
  Der Wertebereich hat dieselbe Schaltfläche.
  Eine Textzelle, die eine Datei enthält (eine `data:`-URL, Base64 oder Hex), hat im Raster keine
  Beschriftung, denn dafür müsste jede Textzelle gelesen werden: Das Rastermenü und der Wertebereich
  zeigen sie für die Zellen, die du auswählst.
- `Ctrl+F` sucht in den geladenen Zeilen; `Ctrl+Shift+C` kopiert die Auswahl mit Überschriften.
- Ein Ergebnis kann in der Statusleiste vom Raster zu einem Diagramm wechseln.

![Zellen mit Dateien, mit Typ und Größe](../../screenshots/guide/file-cell.png)

## Eine langsame Abfrage verstehen

Drücke `Ctrl+Shift+E` auf einer Anweisung, um ihren Plan zu sehen. Siehe
[FEATURES.md](../FEATURES.md#query-plans-and-performance).

## Mehr

Gespeicherte Skripte, Snippets, Verlauf, Vim-Modus, ein Skript auf mehreren Verbindungen ausführen,
der Query Builder und die Datenanalyse stehen in [FEATURES.md](../FEATURES.md#sql-editor). Das
Ergebnisraster steht in [FEATURES.md](../FEATURES.md#results-and-the-data-viewer).
