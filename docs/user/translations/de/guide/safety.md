<!-- source: db93b8bc91b0 -->
[← Benutzerhandbuch](README.md)

# Sicher arbeiten: Produktion, Schreibschutz, Bestätigungen, Maskierung

Alles hier steht auch in [FEATURES.md](../FEATURES.md#transactions-and-safety).

![Einstellungen, die Seite Sicherheit](../../screenshots/guide/safety.png)

## Eine Verbindung als Produktion markieren

1. Bearbeite die Verbindung und öffne die Seite **Darstellung und Sicherheit**.
2. Stelle die Umgebung auf **Produktion** (die anderen Möglichkeiten sind Entwicklung und Test).
   Produktion fragt vor Änderungen nach und startet mit manuellem Commit.
3. Wähle optional eine Farbe, damit sich die Verbindung leicht unterscheiden lässt.

Von da an wird jede Änderung auf dieser Verbindung in einem Dialog mit dem Titel „Änderungen auf
`<name>` (Produktion)“ mit den Anweisungen aufgelistet und läuft erst nach deiner Bestätigung.
Einstellungen (`Ctrl+,`) › **Sicherheit** › **Vor Änderungen auf Produktionsverbindungen fragen**
schaltet das für die App aus. Agenten sind auf einer Produktionsverbindung nie mehr als „Nur lesen“.

## Eine Verbindung schreibgeschützt machen

1. Hake auf derselben Seite **Schreibgeschützt (lehnt Datenänderungen ab)** an.
2. Eine Anweisung, die Daten ändern könnte, läuft nicht. Der Editor zeigt „`<name>` ist
   schreibgeschützt und dies kann Daten ändern.“ ohne Möglichkeit, daran vorbeizukommen. Dasselbe gilt
   für Explain analyze („diese Anweisung kann Daten ändern.“), eine mit `@include` eingebundene Datei,
   eine Notebook-Zelle („Diese Zelle könnte Daten ändern.“), den Kommandozeilen-Client und einen Lauf
   auf mehreren Verbindungen, der die Verbindung als übersprungen auflistet („Schreibgeschützte
   Verbindung“).
3. Gar nicht angeboten werden: **Daten importieren…**, **Daten in eine neue Tabelle importieren…**,
   **CSV als neue Tabelle einfügen…**, **Daten generieren…**, **SQL-Datei ausführen…**, das Anlegen
   von Objekten, das Speichern im Tabellen-Designer und das Bearbeiten von Zeilen im Raster. Agenten
   bleiben „Nur lesen“.

Die Schloss-Schaltfläche eines Editor-Tabs macht nur diesen Tab schreibgeschützt, ohne die gespeicherte
Verbindung zu ändern. Das ist der eine Fall, der nachfragt: Der Editor zeigt dieselbe Meldung mit einer
Schaltfläche **Trotzdem ausführen**, und der Kommandozeilen-Client fragt „Trotzdem ausführen?“.

Dieselbe Seite hat zwei weitere Kästchen: **Keine Exporte** (ihre Daten gehen in keine Datei, keine
andere Datenbank und an keinen Agenten) und **Keine Datenbearbeitung** (Zeilen ändern sich nur per
SQL, nicht im Raster, durch Importe oder generierte Daten).

## Bestätigungen

Einstellungen › **Sicherheit** › **Vor Änderungen**:

- **UPDATE oder DELETE ohne WHERE** (auch `TRUNCATE`): **Vor dem Ausführen fragen** (der Standard),
  **Nie ausführen** oder **Ohne Nachfrage ausführen**. Der Dialog fragt „Jede Zeile ändern?“ und sagt
  ungefähr, wie viele Zeilen betroffen sind.
- **Warnen, wenn eine Transaktion offen bleibt für (Minuten)**: standardmäßig 10, bei 0 gibt es nie
  eine Warnung.
- **Abfragen ohne LIMIT ein LIMIT hinzufügen (Zeilen)**: 0 (der Standard) fügt nie eines hinzu.
- **Fragen, bevor Agenten Daten ändern**: siehe [Agenten](agents.md).

## Eine Änderung rückgängig machen

Standardmäßig an, bewahrt **Zeilen, die ein UPDATE oder DELETE ändert, aufbewahren, um sie
zurückzusetzen** (Einstellungen › Sicherheit › **Sperre und Sicherungen**) die Zeilen eines `UPDATE`
oder `DELETE` auf eine Tabelle auf, bis zu 10.000, aus dem Editor ausgeführt. Öffne das
**Transaktionsprotokoll** in der Symbolleiste des Editors und klicke auf **Wiederherstellen…**, oder
führe **Snapshots vor Änderungen…** in der Befehlspalette aus, um das Skript zu bekommen, das sie
zurückholt.

## Sensible Daten maskieren

1. Drücke `Ctrl+Alt+M` (oder führe **Sensible Daten maskieren** in der Befehlspalette aus): Die Werte
   sensibler Spalten erscheinen als Punkte, solange der Bildschirm geteilt wird. Kopien behalten die
   echten Werte.
2. Einstellungen › **Sicherheit** › **Maskierung** legt fest, welche Spalten sensibel sind, nach Name
   (`*password*`, `*token*`, `*email*` und weitere standardmäßig; `*` steht für beliebige Zeichen) und
   nach Typ. Du kannst eine Spalte auch im Tab ihrer Tabelle markieren.

Agenten lesen die sensiblen Spalten ebenfalls maskiert, außer du hakst für diese Verbindung in
Einstellungen › Agenten › **Sicherheit** **Agenten die Werte sensibler Spalten zeigen** an. Ein
sensibles Feld in einem MongoDB-Dokument (`user.password`) wird ebenfalls maskiert. Wenn ein Agent
fragt, woran du gerade arbeitest, wird auch eine Spalte maskiert, die unter anderem Namen gelesen wird
(`SELECT email AS contact`); versteckt die Anweisung sie hinter einer Unterabfrage, wird das ganze
geteilte Ergebnis maskiert.

Eine Regel oder Markierung, die außerhalb der App entfernt wurde (ein Agent mit Dateitools, der
`settings.toml` oder `catalog.db` bearbeitet), erreicht Agenten erst, wenn du sie annimmst: Die App
fragt nach, und Einstellungen › Agenten › **Sicherheit** listet sie auf, mit der Möglichkeit, sie
anzunehmen oder rückgängig zu machen. Eine außerhalb der App hinzugefügte gilt sofort. Das betrifft
Änderungen an diesen Dateien und nichts darüber hinaus. Ein Agent, der Befehle als dein Benutzer
ausführen kann, kann die Datenbank auf anderen Wegen lesen oder den Schlüsselbund des Systems, in dem
das Siegel liegt: Um Werte vor einem solchen Agenten zu schützen, führe ihn als anderen Benutzer aus und
gib der Verbindung einen Datenbankbenutzer ohne Zugriff auf diese Spalten.

Eine Antwort gilt für das, was die Frage aufgelistet hat: Ändern sich die Dateien erneut, während sie
offen ist, nimmt **Änderung annehmen** nichts an, und die Liste wird erneut gezeigt.

## Die App sperren

Ist ein Master-Passwort gesetzt (siehe [Verbinden](connect.md#master-passwort)), sperrt Einstellungen ›
**Sicherheit** › **App nach so vielen untätigen Minuten sperren** (0, der Standard, sperrt nie)
PlatypusDB und schließt die Produktionsverbindungen. **PlatypusDB sperren** in der Befehlspalette sperrt
sie sofort. Gib zum Entsperren das Master-Passwort ein: Produktionsverbindungen verbinden sich bei
Bedarf neu.
