<!-- source: aa482a6e3843 -->
[← Benutzerhandbuch](README.md)

# Projekte und Teilen

Ein Projekt ist eine Gruppe aus Verbindungen, Skripten und offenen Tabs, die in der Leiste links
angezeigt wird. Die Details stehen in [FEATURES.md](../FEATURES.md#workspace) und
[FEATURES.md](../FEATURES.md#sharing-and-sync).

## Ein Projekt anlegen

1. Führe **Neues Projekt…** in der Befehlspalette (`Ctrl+K`) aus.
2. Gib ihm einen Namen und wähle dann die Verbindungen, die darin erscheinen sollen. Eine Verbindung
   lässt sich auch aus ihrem eigenen Formular hinzufügen: **Projekte** am Ende der Seite
   **Darstellung und Sicherheit**.
3. `Ctrl+1` zeigt alle Verbindungen, `Ctrl+2` bis `Ctrl+9` die Projekte in der Reihenfolge der Leiste.

## Eine Abfrage teilen

1. Führe in einem SQL-Editor-Tab **Abfrage teilen…** aus (Tab-Menü oder Befehlspalette).
2. Kopiere den `platypus://…`-Link oder speichere eine `.platypus`-Datei. Sie enthält das SQL, seinen
   Titel und, wenn du willst, die Parameterwerte; sie nennt die Datenbank, aber nie ein Passwort.
3. Die Kollegin oder der Kollege öffnet sie mit **Geteilten Link öffnen…** oder **Geteilte Abfrage
   öffnen…** oder indem die Datei auf das Fenster gezogen wird. Die App findet die passende gespeicherte
   Verbindung oder bietet eine neue ausgefüllt an und zeigt die Abfrage, ohne sie auszuführen.

## Ein Projekt mit einem Team teilen

1. Klicke mit der rechten Maustaste auf das Projekt in der Leiste und wähle **Projekt teilen…**, dann
   einen Ordner, den das Team in Git oder auf einem gemeinsamen Laufwerk hält. Er enthält die
   Verbindungen (ohne Zugangsdaten) und die Skripte.
2. Jede Kollegin und jeder Kollege klickt auf dem Willkommensbildschirm auf **Geteiltes Projekt
   öffnen…**, wählt den Ordner und gibt die eigenen Passwörter ein.
3. Nach einer Änderung im Ordner führst du im Menü des Projekts **Geteilten Ordner erneut lesen** aus.

Sicher von Haus aus: Eine Verbindung, die neu aus einem geteilten Projekt kommt, gibt Agenten auf
deinem Computer **keinen Zugriff**, bis du etwas anderes wählst, und Anweisungen, die die Datei bei
jedem Verbindungsaufbau ausführt, werden zuerst angezeigt und erst ausgeführt, nachdem du genau diesen
Text freigegeben hast.

## Verbindungen auf einen anderen Computer mitnehmen

Auf dem Willkommensbildschirm schreibt **Verbindungen exportieren** eine `.platypusconn`-Datei: Wähle
die Verbindungen und optional ein Passwort mit mindestens 8 Zeichen, das die Datei verschlüsselt und
sie die gespeicherten Passwörter mitführen lässt. Auf dem anderen Computer liest **Verbindungen
importieren** sie. Ohne Passwort enthält die Datei keine Zugangsdaten.

Um Einstellungen, Verbindungen und Skripte auf deinen Computern auf dem gleichen Stand zu halten,
nimm Einstellungen › **Synchronisierung** mit einem Ordner in einem Cloud-Laufwerk. Siehe
[FEATURES.md](../FEATURES.md#sharing-and-sync).

## Skripte und Git

Ist der Skriptordner eines Projekts ein Git-Repository, zeigt das Panel Skripte den Branch und die
Änderungen pro Skript und bietet **Commit…**, **Pull** und **Push** an. Git muss installiert sein.
