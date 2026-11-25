<!-- source: 641268954234 -->
[← Datenbanken](README.md)

# Apache Cassandra und ScyllaDB

Ein Treiber, gebaut auf dem Crate `scylla` (mit rustls), spricht CQL über das native Protokoll mit beiden. Der
Server sagt, welcher er ist, und die Verbindung zeigt es. Der Treiber ist Teil der App; es wird nichts
heruntergeladen. Der Editor nimmt CQL.

## Verbinden

| Du fügst ein | Gelesen als |
|---|---|
| `cassandra://cassandra:s%40cret@node1:9043/shop` | einfach, Port 9043, Keyspace `shop` |
| `scylla://node1` | derselbe Treiber, für ScyllaDB |
| `cassandras://…` und `scyllas://…` | mit TLS (geprüft: `verify-full`) |

Der Standardport ist 9042. Das Feld **Host** ist ein Contact Point oder mehrere `host:port`-Paare, durch Kommas
getrennt. Das Feld **Keyspace** (optional) ist der Keyspace, in dem die Sitzung startet; der Navigator listet in
jedem Fall alle auf.

- **Ein Contact Point oder ein Tunnel**: Dieser Knoten allein bedient jede Anweisung. Ein Cluster gibt die
  eigenen Adressen seiner Knoten bekannt, die ein Container, ein NAT oder ein Tunnel verbirgt, daher folgt der
  Treiber ihnen nicht. Bei mehreren Contact Points bekommt der Treiber alle und darf jeden nutzen.
- **Anmelden**: ein Benutzer und ein Passwort, für den Passwort-Authenticator des Servers. Ohne Benutzer ist die
  Verbindung anonym.
- **TLS**: CQL kann es nicht aushandeln, daher ist TLS an bei `cassandras://`- und `scyllas://`-URLs und bei
  SSL-Modi ab `require`; `prefer` ist einfach. `require` prüft das Zertifikat nicht; `verify-ca` und
  `verify-full` tun es. Eine CA-Datei und ein Client-Zertifikat kommen in die TLS-Felder.
- **Anweisungen haben kein Timeout** des Treibers: Eine läuft, bis sie fertig ist oder du abbrichst.

## Berechtigungen

Der Navigator liest `system_schema`, das Cassandra 3 und neuer und ScyllaDB teilen; die Anmeldung sieht dort,
was der Server sie lesen lässt. Es gibt keine Verwaltungs-Tabs. Schreibzugriffe brauchen die Berechtigungen, die
CQL verlangt.

## Was funktioniert

- CQL-Skripte, vom Editor aufgeteilt (`$$ … $$`-Strings, `//`-Kommentare und ein `BEGIN BATCH … APPLY BATCH`
  werden verstanden), mit typisierten Ergebnissen, die beim Scrollen seitenweise gelesen werden, über den
  Paging-State des Servers.
- Der Navigator: Keyspaces als Schemas, Tabellen, materialisierte Views, benutzerdefinierte Typen, Funktionen und
  Aggregate, mit den Partitions- und Clustering-Schlüsseln jeder Tabelle und ihren Indizes; das DDL jedes
  Objekts.
- **Das Raster bearbeitet Zeilen**, über den vollständigen Primärschlüssel (Partitions- und Clustering-Spalten):
  Eine bearbeitete Zeile ist ein `UPDATE`, eine neue Zeile ein `INSERT`, das jede Schlüsselspalte braucht, eine
  gelöschte Zeile ein `DELETE`. Die Vorschau zeigt die Anweisungen, die nacheinander laufen; eine, die der
  Server ablehnt, stoppt den Rest. *Rückgängig-Skript* stellt die alten Werte wieder her. Schlüsselspalten
  lassen sich nicht ändern (lösche die Zeile und füge eine andere ein), und Counter werden abgelehnt.
- Lightweight Transactions (`IF …`) werden als solche mit ihrem Ergebnis gemeldet.
- Abbrechen stoppt ein Ergebnis, das gelesen wird. Fehler behalten die Meldung des Servers.
- `USE keyspace` ändert den Keyspace der Sitzung, den der Treiber behält.

## Was nicht

- **Keine Transaktionen**: Jede Anweisung wirkt für sich, und es gibt keinen manuellen Commit.
- **CQL-Schreibzugriffe sind Upserts und melden keine Zeilenzahl**, daher meldet eine Anweisung, die keine
  Zeilen liefert, keine geänderte; ein Update einer Zeile, die inzwischen jemand gelöscht hat, schreibt sie neu.
- Der Datenviewer hat keine Filter- und Sortierleiste und zeigt keine Zeilenzahl.
- Kein Explain und keine SQL-Tools: kein Tabelleneditor, kein ER-Diagramm, kein Schemavergleich, kein Query
  Builder, kein SQL-Import, keine Snapshots vor Änderungen und kein Terminalmodus (`cqlsh` nimmt das Passwort
  nur auf seiner Kommandozeile).

## Agenten

`run_query` und `sample_rows` (ein CQL-`SELECT … LIMIT n`) funktionieren; bei einer Anweisung, die Daten
ändert, wird zuerst nachgefragt. Bei Cassandra abgelehnt: `explain` und `propose_edits`; `estimate_change`
antwortet mit einem Hinweis. Siehe [Agenten und die CLI](../../../../AGENTS.md).

## Bekannte Grenzen

- Cassandra älter als 4.0 und ScyllaDB älter als 5.4 werden beim Verbinden mit „older than … the oldest
  version supported“ abgelehnt, anders als die anderen Datenbanken, die sich mit einer Warnung verbinden.
- Keine Joins, kein `OFFSET`, und Zeilen werden nur über ihren Primärschlüssel gefunden, wie CQL es hat.

## Getestet auf

Die Konformitätssuite läuft auf Apache Cassandra 5 und ScyllaDB 2025.1, ohne Anmeldung: Serverinfo, Skripte,
jeder Typ als typisierte Spalte, Paging, Lightweight Transactions, Fehler, Abbrechen, der Katalog und sein DDL
(auch Funktionen und Aggregate), Speichern der Rasteränderungen und ihr Rückgängigmachen, eine falsche oder
fehlende Anmeldung, die abgelehnt wird, und eine Tunneladresse. Cassandra 4.0 und ScyllaDB 5.4 sind die
niedrigsten Versionen, die der Treiber akzeptiert, und werden von der Suite nicht ausgeführt.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
