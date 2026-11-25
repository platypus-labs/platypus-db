<!-- source: eeb36912ad18 -->
[← Datenbanken](README.md)

# SQLite

SQLite läuft in der App (`rusqlite` mit eingebautem SQLite): kein Server, kein Host, kein Benutzer, kein
Passwort, nichts herunterzuladen.

## Verbinden

Wähle **SQLite** und gib den Pfad der Datei an, oder füge eine URL ein:

| Du fügst ein | Bedeutung |
|---|---|
| `sqlite:///home/ana/app.db` | die Datei `/home/ana/app.db` |
| `sqlite:///C:/data/app.db` | eine Datei auf einem Windows-Laufwerk |
| `sqlite:///:memory:` | eine In-Memory-Datenbank |
| `file:app.db?mode=ro` im Pfadfeld | eine SQLite-URI, so wie geschrieben an SQLite übergeben |

Die Datei wird als leere Datenbank angelegt, wenn sie nicht existiert, ihr Ordner aber schon. Ein
fehlender Ordner ist ein Fehler (eher ein Tippfehler oder ein nicht eingebundenes Laufwerk), und ebenso
ein relativer Pfad, der nicht existiert: Gib den vollständigen Pfad an. Eine Datei, die keine
SQLite-Datenbank ist, wird beim Verbinden abgelehnt.

Eine schreibgeschützte Verbindung (und ein schreibgeschützter Agent) öffnet die Datei nur lesend, sodass
SQLite selbst einen Schreibzugriff ablehnt. SSL-, SSH- und Proxy-Einstellungen gelten nicht für eine
eingebettete Datenbank.

## Berechtigungen

Die des Betriebssystems: Die App muss die Datei lesen und, um etwas zu ändern, sie und ihren Ordner (für
das Journal) beschreiben dürfen. Auf eine belegte Datei wird gewartet, bevor ein Fehler kommt.

## Was funktioniert

- Datenbearbeitung im Raster, über den Primärschlüssel (oder einen eindeutigen Schlüssel auf Spalten
  ohne NULL oder einen virtuellen Schlüssel). Eine Spalte `INTEGER PRIMARY KEY` zählt als Schlüssel,
  obwohl SQLite sie als nullable meldet.
- Manueller Commit und Rollback; Abbrechen einer laufenden Anweisung (`sqlite3_interrupt`).
- Explain zeigt nur den Abfrageplan von SQLite: Es gibt kein Explain analyze.
- Tab Parameter: die wichtigen Pragmas, mit den dokumentierten Standardwerten von SQLite.
- Keine Tabs für Sitzungen, Sperren, Dashboard, Benutzer oder Anweisungen.
- Der Tabelleneditor, das ER-Diagramm und der Vergleich. SQLite ändert wenig an Ort und Stelle, daher
  legt das Ändern einer Tabelle sie neu an und kopiert ihre Zeilen; der Vergleich und der Neuaufbau
  behalten Ausdrucks- und Teilindizes, zurückgestellte Schlüssel und den Zähler `AUTOINCREMENT`.
- Metadatensuche, Objekt-DDL sowie Exporte, Importe und Kopien wie bei jeder SQL-Datenbank.
- Der Terminalmodus führt `sqlite3` aus, falls es im PATH liegt.

## Agenten

Schreibgeschützte Agenten und schreibgeschützte Verbindungen öffnen die Datei nur lesend und führen ihre
Abfragen mit `query_only` aus. `estimate_change` hat bei SQLite keine Zeilenschätzung zu bieten. Siehe
[Agenten und die CLI](../../../../AGENTS.md).

## Bekannte Grenzen

- Ein anderer Prozess hält die Datei vielleicht: Die App wartet bis zu 5 Sekunden auf eine belegte Datei,
  dann meldet SQLite es.
- Bei der Anweisung eines Agenten, die `load_extension(…)` aufruft, wird zuerst nachgefragt, wie bei einer
  Änderung.

## Getestet auf

Die Konformitätssuite läuft auf dem in die App eingebauten SQLite, mit Transaktionen, Plänen,
Tabellenbearbeitung, dem Neuaufbau, der `AUTOINCREMENT` behält, und nur lesend geöffneten Dateien.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
