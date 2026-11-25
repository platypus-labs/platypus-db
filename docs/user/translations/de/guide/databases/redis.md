<!-- source: 8b43e38d9f7d -->
[← Datenbanken](README.md)

# Redis

Der Treiber baut auf dem Crate `redis` auf (RESP3, wenn der Server es spricht, mit rustls für TLS). Er öffnet
Redis 6 und neuer sowie Redis Stack für JSON-Schlüssel. Der Editor nimmt **Redis-Befehle**, einen pro Zeile.
Der Treiber ist Teil der App; es wird nichts heruntergeladen.

## Verbinden

| Du fügst ein | Gelesen als |
|---|---|
| `redis://:s%40cret@cache:6380/3` | einfaches TCP, nur Passwort, Datenbank 3 |
| `redis://app:pw@cache/0` | ein ACL-Benutzer und sein Passwort |
| `rediss://app:pw@cache` | TLS (geprüft: `verify-full`) |

Der Standardport ist 6379. Der Pfad (oder das Feld Datenbank) ist die **Datenbanknummer**, `0`, wenn leer; alles
andere ist ein Fehler. Benutzer und Passwort sind die der ACL: Ein Passwort allein meldet als Standardbenutzer
an.

**TLS** ist an bei `rediss://`-URLs und bei SSL-Modi ab `require` (`redis://` heißt kein TLS, und `prefer`
schaltet es nicht ein, weil Redis TLS auf einem eigenen Port nimmt). `require` prüft das Zertifikat nicht;
`verify-ca` und `verify-full` tun es. Eine CA-Datei und ein Client-Zertifikat kommen in die TLS-Felder. Die
Sitzung öffnet pro genutzter Datenbank eine Verbindung, weil `SELECT` auf einer gemeinsamen Verbindung alle
anderen Nutzer davon verschieben würde.

## Berechtigungen

Die ACL-Regeln der Anmeldung entscheiden, was der Navigator und die Befehle können. Der Navigator liest `INFO`
und führt für jeden Schlüsseltyp `SCAN … TYPE` aus; das Durchsuchen eines Typs führt `BROWSE` aus, nie `KEYS`.
Es gibt keine Verwaltungs-Tabs.

## Was funktioniert

- Befehle im Editor, so in Anführungszeichen gesetzt, wie `redis-cli` es tut; Zeilen mit `#` und `--` sind
  Kommentare. Antworten sind Zeilen, verschachtelte Antworten als JSON im Dokumentbaum (`F7`).
- `SELECT n` verschiebt die Sitzung in eine andere Datenbank. `MULTI` stellt Befehle in eine Warteschlange bis
  `EXEC`, das sie als eine Transaktion sendet und jede Antwort zeigt.
- Der Navigator listet die Datenbanken auf, die Schlüssel halten, jede mit einer Tabelle pro Schlüsseltyp:
  `string`, `hash`, `list`, `set`, `zset`, `stream` und `json` mit RedisJSON. Das Öffnen einer führt
  `BROWSE db0.hash` aus, optional mit `MATCH` und `LIMIT`, und liest große Schlüssel Stück für Stück.
- **Das Raster bearbeitet Zeilen**: Werte, TTLs, Schlüsselnamen, Hash-Felder, Mitglieder und Scores; neue Zeilen
  fügen Schlüssel und Elemente hinzu, gelöschte Zeilen entfernen sie. Die Vorschau zeigt die Befehle, die als
  ein `MULTI`/`EXEC` laufen; ein Befehl, den der Server ablehnt, macht die anderen nicht rückgängig. Ein
  *Rückgängig-Skript* stellt die Werte wieder her. Ein Schlüssel, Feld oder Mitglied, das schon existiert,
  stoppt das Speichern, bevor etwas läuft.
- `FLUSHDB` und `FLUSHALL` fragen zuerst nach, wie ein `UPDATE` ohne `WHERE`; bei Produktionsverbindungen
  fragen Befehle, die den Server ändern (`CONFIG SET`, `SHUTDOWN`), wie DDL nach. Schreibgeschützte
  Verbindungen führen nur lesende Befehle aus.
- Abbrechen stoppt einen blockierenden Befehl (`BLPOP`, `XREAD BLOCK`).
- Der Terminalmodus führt `redis-cli` aus, falls es im PATH liegt, mit dem Passwort in seiner Umgebung.
- Alle Befehle und Schlüsseltypen sind in [FEATURES.md](../../FEATURES.md#redis) beschrieben.

## Was nicht

- Subscriptions (`SUBSCRIBE` und ähnliche) und `MONITOR` werden abgelehnt: Sie würden die Verbindung
  übernehmen.
- Kein manueller Commit: `MULTI`/`EXEC` ist die einzige Transaktion.
- Kein Explain, keine Objekt-DDL, keine SQL-Tools (Tabelleneditor, ER-Diagramm, Schemavergleich, Query Builder,
  SQL-Import), keine Snapshots vor Änderungen.

## Agenten

`run_query` und `sample_rows` (`BROWSE … LIMIT n`) funktionieren; bei jedem Befehl, der kein Lesen ist, wird
zuerst nachgefragt. Die Befehle, die die Zugangsdaten des Servers zeigen (`ACL GETUSER`, `ACL LIST` und ein
`CONFIG GET`, dessen Muster auf eine Passwort-Einstellung passt), werden schreibgeschützten Agenten verweigert.
Bei Redis abgelehnt: `explain`, `get_er_diagram`, `propose_edits` und `query_across_databases`. Siehe
[Agenten und die CLI](../../AGENTS.md).

## Bekannte Grenzen

- Älter als 6.0: Die App verbindet sich mit einer Warnung, dass manche Funktionen vielleicht nicht gehen.
- Die Schlüsselzahl einer Tabelle erscheint nur, wenn die Datenbank bis zu 10.000 Schlüssel hat.
- Werte, die kein UTF-8 sind, erscheinen in Anführungszeichen mit Escapes.

## Getestet auf

Die Konformitätssuite läuft auf Redis Stack 7.4 (mit RedisJSON): Metadaten, `BROWSE` auf jedem Typ, Befehle und
ihre Fehler, Speichern und Rückgängigmachen von Rasteränderungen, Abbrechen blockierender Befehle, ACL-Benutzer
und Datenbankauswahl sowie eine verlorene Verbindung, die sich wieder öffnet. Redis 6 ist die niedrigste Version,
die der Treiber akzeptiert, wird aber von der Suite nicht ausgeführt.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
