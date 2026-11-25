<!-- source: 25c359215e93 -->
[← Benutzerhandbuch](README.md)

# Fehlerbehebung und FAQ

Beginne mit **Verbindung testen** im Formular der Verbindung: Der Test meldet den Fehler mit der
eigenen Meldung des Servers und nennt die Server-Version, wenn das hilft.

## Verbindungsfehler

### Der Host ist nicht erreichbar

Prüfe zuerst Host und Port (das Formular meldet „Der Host fehlt.“ oder „Der Port muss eine Zahl von
1 bis 65535 sein.“). Schau dann, was zwischen dir und dem Server liegt: ein SSH-Tunnel, ein Proxy,
eine Portweiterleitung oder ein Befehl rund um die Verbindung, der ein VPN startet (Seite
**Netzwerk**). Jedes davon liefert eine eigene Meldung, siehe unten.

### TLS

Auf der Seite **Netzwerk** legt **SSL-Modus** fest, was geprüft wird (siehe [Verbinden](connect.md#tls)).

- Ein Server, der unverschlüsselte Verbindungen ablehnt, braucht `require` oder höher. Ein Server mit
  einem Zertifikat, dem dein Computer nicht vertraut, braucht `verify-ca` oder `verify-full` mit dem
  **CA-Zertifikat (PEM)**, oder `require`, um zu verschlüsseln, ohne zu prüfen.
- „invalid certificate“ und „invalid root certificates“ bedeuten eine PEM-Datei, die sich nicht lesen
  lässt: Prüfe, ob es das Zertifikat selbst ist und nicht ein Schlüssel.
- „a client certificate needs both the certificate and its key“: Fülle **Client-Zertifikat (PEM)**
  und **Client-Schlüssel (PEM)** zusammen aus. „invalid client certificate or key“ heißt, dass sie
  nicht zusammenpassen oder kein PEM sind.
- Redis, Elasticsearch und Cassandra übernehmen TLS ab einem SSL-Modus von `require` oder höher.

### SSH

Die Fehler beginnen mit `SSH <user>@<host>:<port>:`.

- `no answer within 10 s` oder `cannot reach <host>:<port>`: Der SSH-Server (oder der Proxy davor)
  ist von hier aus nicht erreichbar.
- `cannot read the key <path>`: Die **Datei des privaten Schlüssels** fehlt, ist nicht lesbar oder
  braucht ihre Passphrase.
- `no SSH agent is running`: Starte deinen SSH-Agent oder nimm eine Schlüsseldatei oder ein Passwort.
- Ein abgelehntes Passwort oder ein abgelehnter Schlüssel ist die Antwort des Servers: Prüfe
  **SSH-Benutzer** und die Methode.
- Ein Server, dessen Schlüssel sich seit der ersten Verbindung geändert hat, wird abgelehnt. Die
  Schlüssel stehen in `~/.ssh/known_hosts`, gemeinsam mit OpenSSH: Entferne dort die alte Zeile,
  wenn die Änderung erwartet ist.

### Proxy

Die Fehler nennen den Proxy, zum Beispiel `SOCKS5 proxy host:1080: …` oder `HTTP proxy host:3128: …`.

- `no answer within 10 s`: Der Proxy antwortet nicht.
- `the proxy asks for a user and password, or refused them`: Schreibe sie in die URL
  (`http://user:password@host:3128`).
- `cannot reach <host>:<port>: …`: Der Proxy hat geantwortet, konnte die Datenbank aber nicht
  erreichen.
- Eine URL, die nicht verstanden wird, wird als `proxy "<url>": …` gemeldet. Eine Portweiterleitung
  steht allein: Entferne den SSH-Tunnel oder den Proxy.

### Schlüsselbund

- `keychain error: …` oder „the system has no keychain to save the password in“: Unter Linux muss ein
  Secret Service wie GNOME Keyring oder KWallet laufen. Ohne einen lege ein Master-Passwort fest
  (Einstellungen › **Master-Passwort**) oder wähle **Jedes Mal fragen** für das Passwort.
- „Für diese Verbindung ist kein Passwort gespeichert: Es wird im Schlüsselbund gespeichert.“ ist nur
  ein Hinweis.
- Passwörter, die aus einem anderen Tool ohne ihre Passwörter importiert wurden (zum Beispiel DBeaver
  mit eigenem Master-Passwort), werden bei der ersten Verwendung abgefragt.

### Master-Passwort

- „PlatypusDB ist gesperrt“ oder „Master-Passwort eingeben, um sie zu verwenden“: Gib es ein und
  klicke auf **Entsperren**. Das Sperren bei Inaktivität steht unter Einstellungen › **Sicherheit**;
  Produktionsverbindungen verbinden sich bei Bedarf neu.
- Es lässt sich nicht wiederherstellen. Geht es verloren, sind die gespeicherten Passwörter verloren:
  Lege die Passwörter in jeder Verbindung neu fest.
- Um zum Schlüsselbund zurückzukehren: Einstellungen › **Master-Passwort** › **Ausschalten…** (das
  aktuelle Passwort wird abgefragt).

## Ein Treiber-Download schlägt fehl

DuckDB, Snowflake, BigQuery, Databricks, Trino, andere ADBC-Treiber, Oracle und IBM Db2 laden ihre
Bibliothek bei der ersten Verwendung herunter. Das Popup zeigt „Treiber für `<name>` wird
heruntergeladen…“ mit einem **Abbrechen**, und bei einem Fehler erscheint eine Meldung mit **Erneut
versuchen**.

- `the download failed (check the internet connection)` oder `cannot reach the driver registry`: Es
  gibt keinen Weg zur Registry; prüfe das Netzwerk, den Proxy oder die Firewall und klicke dann auf
  **Erneut versuchen**.
- `the downloaded driver is corrupted (its SHA-256 does not match)`: Die Datei wurde verändert oder
  auf dem Weg abgeschnitten; versuche es erneut und melde es, wenn es bleibt.
- `no <name> driver for this platform` oder „PlatypusDB lädt DuckDB unter Windows, macOS und Linux auf
  x64- und ARM-Prozessoren herunter; nicht auf diesem System.“: Für dein System gibt es keinen Build.
- `the driver's license (…) does not allow downloading it here`: Die App lädt keine Treiber herunter,
  deren Lizenz es verbietet.
- IBM Db2 ist vorerst nur unter Windows über ODBC erreichbar; das Registrieren seines Treibers
  verlangt Administratorrechte (Benutzerkontensteuerung). Antworte mit Ja, sonst wird der Treiber
  nicht registriert.
- **Abbrechen** lässt das Formular benutzbar; das Angebot kommt bei **Verbindung testen** oder beim
  ersten Öffnen wieder.

Einstellungen › **Treiber** listet auf, was heruntergeladen ist, entfernt einen Treiber und zeigt die
ODBC-Treiber des Systems. Ein Entfernen, das Windows abgelehnt hat, wird beim nächsten Start
abgeschlossen.

## Ein Agent sieht eine Verbindung nicht

Gehe diese Liste von oben nach unten durch.

1. **Der Zugriff ist aus.** Der Agent antwortet „`<name>` is not available to agents“. Stelle die
   Verbindung in Einstellungen › **Agenten** › **Sicherheit** auf **Nur lesen** oder **Lesen und
   schreiben**. Eine Verbindung aus einem geteilten Projekt startet auf deinem Computer mit **Aus**.
2. **Ein Master-Passwort.** Der Agent antwortet „the saved password of `<name>` is locked by
   PlatypusDB's master password, which agents cannot enter“. Gib dieser Verbindung einen
   Passwort-Befehl oder `.pgpass`, oder schalte das Master-Passwort aus.
3. **Das Passwort wird jedes Mal abgefragt.** Der Agent antwortet „`<name>` asks for its password at
   each connection, and agents cannot type it“. Speichere es im Schlüsselbund oder nimm `.pgpass`
   oder einen Passwort-Befehl.
4. **Schreibzugriffe abgelehnt.** Produktions- und schreibgeschützte Verbindungen sind nie mehr als
   „Nur lesen“; bei den anderen wartet die Änderung auf deine Antwort in der App
   ([Agenten](agents.md)).
5. **Der Client wurde nicht neu gestartet** nach **Installieren**, oder der Eintrag fehlt:
   Einstellungen › **Agenten** › **Einrichtung** zeigt, ob PlatypusDB in der Datei des Clients steht.
6. **Eine portable Kopie oder ein anderes Profil.** Der Agent liest die Verbindungen des Profils, von
   dem aus er gestartet wurde; ein Client, der mit der installierten App eingerichtet wurde, sieht
   den `data`-Ordner einer portablen Kopie nicht.

## Die portable Kopie

- Sie ist eine ausführbare Datei mit einem leeren `data`-Ordner daneben. Fehlt der Ordner beim Start,
  nimmt die App stattdessen die Ordner des Systems; ein umbenannter `data`-Ordner sieht also aus wie
  verlorene Verbindungen: Benenne ihn zurück.
- Alles liegt in `data`, auch die Logs; der Ordner zieht mit der ausführbaren Datei um. Gespeicherte
  Passwörter bleiben im Schlüsselbund des Computers, außer ein Master-Passwort hält sie auch im
  Ordner: Auf einem anderen Computer lege zuerst das Master-Passwort fest oder gib die Passwörter
  erneut ein.
- Sie aktualisiert sich nicht selbst: Ersetze die ausführbare Datei durch die neue. Der
  Willkommensbildschirm zeigt `portabel` neben der Version.

## Wo Daten und Logs liegen

Die App legt Verbindungen und Einstellungen (`connections.toml`, `settings.toml` und `secrets.vault`
mit einem Master-Passwort) in ihrem Konfigurationsordner ab, und Verlauf, Skripte, heruntergeladene
Treiber und Logs in ihrem Datenordner; beide heißen `dev.platypusdb.desktop`:

| System | Konfigurationsordner | Datenordner |
|---|---|---|
| Windows | `%APPDATA%\dev.platypusdb.desktop` | `%APPDATA%\dev.platypusdb.desktop` |
| macOS | `~/Library/Application Support/dev.platypusdb.desktop` | derselbe |
| Linux | `~/.config/dev.platypusdb.desktop` | `~/.local/share/dev.platypusdb.desktop` |

Eine portable Kopie hält alles in ihrem `data`-Ordner, und die Umgebungsvariable `PLATYPUS_DATA_DIR`
legt alles in den Ordner, den sie nennt.

![Einstellungen, die Seite Diagnose](../../../../screenshots/guide/diagnostics.png)

Das Log ist `logs/platypus.log` im Datenordner: eine Zeile pro Start, Fehler und Absturz, rotiert bei
1 MB mit zwei älteren Dateien (`platypus.1.log`, `platypus.2.log`). Einstellungen › **Diagnose** zeigt
den Ordner (**Kopieren**) und **Diagnosepaket speichern…**, ein Zip mit der Version, dem System, den
Einstellungen ohne Passwörter und den Logs, zum Anhängen an einen Fehlerbericht. Nichts wird irgendwohin
gesendet; lies die Logs vor dem Teilen, denn ein Fehler kann einen Server oder eine Tabelle nennen.
Nach einem Absturz bietet der nächste Start dasselbe Paket an.

## Updates

- Beim Start sucht die App nach einer neueren Version und zeigt „PlatypusDB `<version>` ist
  verfügbar“ mit **Später** und **Installieren und neu starten**.
- Eine portable Kopie sagt es nur: Ersetze ihre ausführbare Datei. Eine Installation über einen
  Paketmanager (Flatpak, das AUR) sagt es ebenfalls und überlässt ihm das Update.
- Heruntergeladene Treiber werden getrennt aktualisiert: Einstellungen › **Treiber** › **Nach Updates
  suchen**, oder die Einstellungen **Beim Start nach Treiber-Updates suchen** (an, höchstens einmal am
  Tag) und **Treiber-Updates selbstständig installieren** (aus). Die eingebauten Treiber werden mit
  der App aktualisiert.

## FAQ

**Kann ich PlatypusDB offline nutzen?** Ja, mit einer lokalen Datenbank. Downloads (Treiber, Updates)
brauchen das Netzwerk.

**Wo melde ich einen Fehler?** Im [Issue-Tracker](https://github.com/platypus-labs/platypus-db/issues),
wenn möglich mit dem Diagnosepaket.

**Wo sind die Tastenkürzel?** In [SHORTCUTS.md](../../../SHORTCUTS.md), und unter Einstellungen › Tastenkürzel
kannst du sie ändern.
