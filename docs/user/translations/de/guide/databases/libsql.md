<!-- source: 7b39ba524d58 -->
[← Datenbanken](README.md)

# libSQL und Turso

Der Treiber spricht das Protokoll **Hrana 3** über HTTP(S) mit einem libSQL-Server (`sqld`) oder mit Turso. Das
SQL ist das von SQLite, also sind auch der Navigator, die Typen und die Pläne die von SQLite. Der Treiber ist
Teil der App; es wird nichts heruntergeladen.

## Verbinden

| Du fügst ein | Gelesen als |
|---|---|
| `libsql://db-acme.turso.io?authToken=…` | HTTPS zu diesem Host, das Token als Passwort |
| `libsql://:token@localhost:8080?sslmode=disable` | einfaches HTTP, das Token an der Stelle des Passworts |
| `libsql://localhost:8080?tls=0&authToken=t` | `tls=0` ist einfaches HTTP, `tls=1` ist HTTPS |
| `wss://…`, `ws://…` | derselbe Server über HTTP(S): `wss` ist TLS, `ws` nicht |
| `https://host` oder `http://host` in einem libSQL-Formular | die HTTP-Adresse des Servers |

Ohne Port wird der Standard des Schemas genutzt: 443, oder 80, wenn TLS aus ist.

- **Token**: das Passwortfeld oder `authToken` in der URL.
- **TLS**: Eine `libsql://`-URL ist HTTPS mit geprüftem Zertifikat und Hostnamen (`verify-full`), weil die URL
  das Token trägt. Auch die Turso-Kachel im Formular startet mit `verify-full`. Für ein lokales `sqld` ohne TLS
  wähle `disable` oder nimm `tls=0` in der URL. `sslrootcert` nennt die Datei deiner CA; sonst werden die
  vertrauenswürdigen Zertifikate des Systems genutzt.

## Berechtigungen

Was immer das Token erlaubt: Die App sendet die Anweisungen, die du ausführst, und die Katalogabfragen von
SQLite für den Navigator.

## Was funktioniert

- Datenbearbeitung im Raster, über den Primärschlüssel (oder einen eindeutigen Schlüssel auf Spalten ohne NULL
  oder einen virtuellen Schlüssel).
- Manueller Commit und Rollback. Eine Anweisung läuft auf einem Stream, den der Server zwischen Anfragen offen
  hält (ein `baton`), sodass eine Transaktion und temporäre Tabellen halten. Beendet der Server den Stream
  außerhalb einer Transaktion, öffnet die App einen anderen und macht weiter; innerhalb einer Transaktion meldet
  sich die Verbindung als verloren, und mit ihr die Transaktion.
- Explain zeigt den Abfrageplan von SQLite; es gibt kein Explain analyze.
- Metadatensuche und Objekt-DDL sowie Schema-Tools wie bei SQLite.
- Keine Tabs für Sitzungen, Sperren, Dashboard, Benutzer oder Parameter.

## Was nicht

- **Abbrechen**: HTTP kann eine laufende Anweisung nicht stoppen, daher antwortet Abbrechen, dass das
  Abbrechen von libSQL-Anweisungen nicht unterstützt wird.
- Ergebnisse kommen ganz vom Server, nicht Zeile für Zeile.
- Der Terminalmodus ist nicht verfügbar: libSQL-Server haben keinen Kommandozeilen-Client, mit dem sich Skripte
  ausführen ließen.

## Agenten

Schreibgeschützte Agenten und schreibgeschützte Verbindungen verlassen sich allein auf die Prüfung der
Anweisung; dem Server wird nicht gesagt, Schreibzugriffe abzulehnen. Siehe [Agenten und die CLI](../../../../AGENTS.md).

## Bekannte Grenzen

- Nach einem Netzwerkfehler, nachdem eine Anweisung gesendet wurde, kann unklar bleiben, ob sie lief; die App
  sendet sie nicht erneut.

## Getestet auf

Die Konformitätssuite läuft auf `sqld` v0.24.33 (Container `ghcr.io/tursodatabase/libsql-server`), ohne TLS. Die
Cloud von Turso ist nicht getestet.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
