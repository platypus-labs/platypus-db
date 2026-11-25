<!-- source: 284fda1cc34a -->
[← Benutzerhandbuch](README.md)

# Mit einer Datenbank verbinden

![Die Datenbank für eine neue Verbindung wählen](../../screenshots/connections.png)

## Über eine URL

1. Klicke auf **Neue Verbindung** (oder auf das `+` des Navigators).
2. Füge die URL unter den Logos ein: `postgres://`, `mysql://`, `sqlserver://`, `clickhouse://`,
   `libsql://`, `mongodb://`, `redis://`, `rediss://`, `elasticsearch://`, `opensearch://`, eine
   JDBC-URL oder eine ADO.NET-/ODBC-Zeichenfolge. Das Formular öffnet sich für diese Datenbank
   ausgefüllt.
3. Trage das Passwort in das Feld **Passwort** ein, nicht in die URL: Das Formular weist darauf hin,
   wenn die URL eines enthält.
4. Klicke auf **Verbindung testen**, dann auf **Speichern**.

Oder wähle die Datenbank bei den Logos (die Suche oben nimmt mit `Enter` den ersten Treffer). Die
Auswahl legt den Treiber fest; **Zurück** führt zu den Logos zurück.

## Das Formular

Das Formular hat vier Seiten: **Allgemein**, **Darstellung und Sicherheit**, **Netzwerk** und
**Initialisierung**. Ein Punkt hinter dem Namen einer Seite bedeutet, dass sie Einstellungen enthält.

- **Allgemein**: Host, Port, Benutzer, Datenbank und woher das Passwort kommt (siehe unten).
- **Darstellung und Sicherheit**: die Umgebung (Entwicklung, Test oder Produktion), eine Farbe, das
  Kästchen **Schreibgeschützt (lehnt Datenänderungen ab)**, die Kästchen, die Exporte und
  Datenbearbeitung sperren, und der Zugriff der Agenten. Siehe [Sicherheit](safety.md) und
  [Agenten](agents.md).
- **Netzwerk**: SSH-Tunnel, SSL-Modus und Zertifikate, Proxy, Portweiterleitung und Befehle rund um
  die Verbindung.
- **Initialisierung**: eine Variablendatei, ein Standardschema oder eine Zeitzone und Anweisungen, die
  bei jedem Verbindungsaufbau laufen.

## Passwörter

Unter **Allgemein** kommt das Passwort aus einer dieser Quellen:

- **Im Schlüsselbund gespeichert**: der Schlüsselbund des Systems. **Verbindung testen** sendet es nur
  an den Server, für den es gespeichert wurde: Änderst du den Host, den Port oder die SSH-Server auf
  dem Weg, fragt es dich, das Passwort für diesen Test einzugeben. Eine andere Datenbank desselben
  PostgreSQL- oder MySQL-Servers ist kein anderer Server.
- **Jedes Mal fragen**: wird bei jeder Verbindung eingegeben. Agenten können eine solche Verbindung
  nicht öffnen.
- **.pgpass-Datei** (nur PostgreSQL; Host, Port, Datenbank und Benutzer der Verbindung wählen die
  Zeile, wie es libpq macht).
- **Shell-Befehl**: ein Befehl, der das Passwort ausgibt.

Jedes Feld kann eine Umgebungsvariable lesen, geschrieben als `${env:NAME}`. Passwortmanager
(1Password, Bitwarden, HashiCorp Vault, AWS Secrets Manager) stehen in derselben Liste bei jeder
Datenbank, deren Formular nach einem Passwort oder einem Token fragt; keiner ist auf eine Datenbank
beschränkt. Gib die Referenz des Geheimnisses an: Die App baut daraus den Shell-Befehl (`op read`,
`bw get password`, `vault kv get`, `aws secretsmanager get-secret-value`) und führt ihn bei jeder
Verbindung aus. Ihr Kommandozeilen-Tool muss installiert und angemeldet sein. Nichts davon wurde
gegen ein echtes 1Password-, Bitwarden-, Vault- oder AWS-Konto ausgeführt: Getestet sind nur die
Befehle, die die App baut, und der Shell-Befehl selbst.

Manche Datenbanken melden sich ganz ohne Passwort an, und dieselbe Liste bietet sie an: ein Token aus
dem Kommandozeilen-Tool einer Cloud (AWS IAM und Google Cloud IAM für PostgreSQL und MySQL, Microsoft
Entra ID für diese beiden und SQL Server), die Windows-Anmeldung für SQL Server und ein
**Kerberos-Ticket (GSSAPI)** für PostgreSQL unter Windows und Linux. Führe zuerst `kinit` aus oder setze
`KRB5CCNAME` oder `KRB5_CLIENT_KTNAME`; unter Windows ist das Ticket das deiner Sitzung. Das Ticket ist
das Einzige, was an den Server geht, und ein Test ohne eines sagt, was auszuführen ist. Die
[PostgreSQL-Seite](databases/postgresql.md) hat die Details und was getestet ist und was nicht: Ein
echtes Ticket hat sich von Linux aus angemeldet, noch nicht über das SSPI von Windows.

### Master-Passwort

Ein Master-Passwort hält die gespeicherten Passwörter in einer verschlüsselten Datei statt im
Schlüsselbund.

1. Öffne die Einstellungen (`Ctrl+,`) › **Master-Passwort**.
2. Klicke auf **Master-Passwort festlegen…**, gib es zweimal ein (mindestens 8 Zeichen) und bestätige.
3. Um zum Schlüsselbund zurückzukehren, klicke auf derselben Seite auf **Ausschalten…**. **Jetzt
   sperren** sperrt sofort.

Es lässt sich nicht wiederherstellen: Ohne es sind die gespeicherten Passwörter verloren. Agenten
können es nicht eingeben (siehe
[Fehlerbehebung](troubleshooting.md#ein-agent-sieht-eine-verbindung-nicht)).

## TLS

Auf der Seite **Netzwerk** nimmt **SSL-Modus** die libpq-Werte: `disable`, `prefer`, `require`,
`verify-ca` und `verify-full`. `prefer` und `require` verschlüsseln, ohne das Zertifikat zu prüfen;
`verify-ca` und `verify-full` prüfen es, daher kann ein **CA-Zertifikat (PEM)** nötig sein. Für Server,
die Clients per Zertifikat authentifizieren, gib das **Client-Zertifikat (PEM)** und den
**Client-Schlüssel (PEM)** an (PostgreSQL und MySQL).

## SSH-Tunnel

![Ein SSH-Tunnel auf der Seite Netzwerk](../../screenshots/guide/connect-network.png)

1. Hake auf der Seite **Netzwerk** **Über einen SSH-Tunnel verbinden** an.
2. Fülle **SSH-Host**, den Port und **SSH-Benutzer** aus.
3. Melde dich mit einem Passwort, einer **Datei des privaten Schlüssels** (und ihrer Passphrase) oder
   dem **SSH-Agent** an. Für einen Bastion-Host ergänze einen **Jump-Host** (**Zuerst über einen
   Jump-Host**).
4. Klicke auf **Verbindung testen**.

Der Schlüssel des Servers wird gegen `~/.ssh/known_hosts` geprüft: Ein unbekannter Server wird beim
ersten Mal gelernt, und ein Schlüssel, der sich seitdem geändert hat, wird abgelehnt. Ein
**Netzwerkprofil** teilt einen Tunnel und die SSL-Einstellungen zwischen Verbindungen.

## Proxy und Portweiterleitung

Ebenfalls unter **Netzwerk**: ein SOCKS5- oder HTTP-Proxy (`socks5://user:password@host:1080`,
`http://host:3128`) oder eine Portweiterleitung über `kubectl` oder eine AWS-Systems-Manager-Sitzung
(das CLI muss installiert und angemeldet sein). Eine Portweiterleitung steht allein: Entferne den
SSH-Tunnel oder den Proxy.

## Weiter

Details zu jeder Datenbank: [Datenbanken](databases/README.md). Cloud-Anmeldung, Pooler, Importe aus
DBeaver, DataGrip und pgAdmin (Db2-Verbindungen eingeschlossen) und das Exportieren von Verbindungen
stehen in [FEATURES.md](../FEATURES.md#connections). Wenn die Verbindung nicht klappt, siehe
[Fehlerbehebung](troubleshooting.md#verbindungsfehler).
