<!-- source: 2a42aace96dc -->
[← Datenbanken](README.md)

# Elasticsearch und OpenSearch

Ein Treiber spricht mit beiden über ihre HTTP-API (`reqwest` mit rustls). Der Server sagt beim Verbinden, welcher
er ist, und die Verbindung zeigt es (`Connected: OpenSearch 2.17.1`). Der Treiber ist Teil der App; es wird
nichts heruntergeladen.

## Verbinden

| Du fügst ein | Gelesen als |
|---|---|
| `elasticsearch://user:pass@host:9200/logs-*` | einfaches HTTP; der Pfad ist ein Index-Muster |
| `elasticsearchs://user:pass@host:9200` | HTTPS (geprüft: `verify-full`) |
| `opensearch://…` und `opensearchs://…` | dasselbe, als OpenSearch gemerkt, damit die URL so zurückgeschrieben wird |

Der Standardport ist 9200. Das Feld **Datenbank** ist ein optionales Index-Muster (`logs-*` oder mehrere, durch
Kommas getrennt), das eingrenzt, was der Navigator auflistet; leer listet jeden Index auf, der nicht versteckt
ist.

- **Anmelden**: Ein Benutzer und ein Passwort nutzen Basic Authentication. Ist der Benutzer leer, ist das
  Passwortfeld ein **API-Schlüssel** (der `encoded`-Wert), gesendet als `Authorization: ApiKey …`.
- **TLS**: HTTPS bei `elasticsearchs://`- und `opensearchs://`-URLs und bei SSL-Modi ab `require`;
  `elasticsearch://` und `prefer` sind einfaches HTTP. `require` prüft das Zertifikat nicht; `verify-ca` und
  `verify-full` tun es. Eine CA-Datei und ein Client-Zertifikat kommen in die TLS-Felder.

## Berechtigungen

Die, die die Anfragen brauchen. Der Navigator liest die Indizes, Aliase, Data Streams und Mappings, die die
Anmeldung sehen kann; es gibt keine Verwaltungs-Tabs.

## Was funktioniert

- **Anfragen, wie die Dev-Tools-Konsole von Kibana sie schreibt**: eine Zeile wie `GET /orders/_search` (der
  Schrägstrich ist optional; Methoden `GET`, `POST`, `PUT`, `DELETE`, `HEAD`), dann ihr JSON-Body. `_bulk` und
  `_msearch` nehmen NDJSON-Bodys, die an einer Leerzeile oder der nächsten Anfrage enden.
- **SQL**: Alles andere ist SQL (`_sql` bei Elasticsearch, `_plugins/_sql` bei OpenSearch), mit typisierten
  Spalten und dem Cursor des Servers für lange Ergebnisse. Fehler behalten den Grund des Servers, und ein
  SQL-Fehler zeigt auf seine Stelle in der Anweisung.
- Eine Suche liefert Dokumente wie die Ergebnisse von MongoDB: eine Spalte pro Feld der obersten Ebene, mit dem
  fokussierten Dokument als Baum (`F7`). Aggregationen kommen als eigene Ergebnisse.
- Der Navigator listet Indizes als Tabellen (Health, Shards und Replicas im Tooltip) und Aliase und Data Streams
  als Views. Ein Index öffnet sich als Suche über alle seine Dokumente, je 1.000 Zeilen gelesen mit einem Point
  in Time und `search_after` (ein Scroll bei OpenSearch), sodass das Lesen nicht bei 10.000 Treffern endet. Ein
  Feld ohne gemappte Spalte kommt als JSON in eine letzte Spalte `_other`.
- **Das Raster bearbeitet Dokumente**: Eine geänderte Zelle ist ein `_update` über `_id`, eine neue Zeile ein
  `_create` (eine `_id`, die existiert, wird vom Server abgelehnt) oder ein generiertes Dokument, eine gelöschte
  Zeile ein `DELETE`. Die Vorschau zeigt die Anfragen, die nacheinander laufen; eine, die der Server ablehnt,
  stoppt den Rest, und die früheren bleiben. *Rückgängig-Skript* stellt die alte `_source` wieder her (ein
  Dokument mit einer generierten `_id` muss von Hand gelöscht werden).
- `_delete_by_query` und `_update_by_query` ohne Abfrage (oder `match_all`) und `DELETE /_all` fragen zuerst
  nach, wie ein `DELETE` ohne `WHERE`. Schreibgeschützte Verbindungen führen nur die Anfragen und das SQL aus,
  die lesen.
- Abbrechen stoppt eine Suche auf dem Server. Die Objekt-DDL eines Index ist die Anfrage, die ihn neu anlegt.
- Die Details stehen in [FEATURES.md](../../../../FEATURES.md#elasticsearch-and-opensearch).

## Was nicht

- Keine Transaktionen (kein manueller Commit) und kein Explain.
- Keine SQL-Tools: kein Tabelleneditor, kein ER-Diagramm, kein Schemavergleich, kein Query Builder, kein
  SQL-Import, keine Snapshots vor Änderungen und kein Terminalmodus.
- Der Datenviewer hat keine Filter- und Sortierleiste.

## Agenten

`run_query` und `sample_rows` funktionieren mit Konsolenanfragen und SQL, und bei einer Anfrage, die Daten ändert,
wird zuerst nachgefragt. Bei Elasticsearch und OpenSearch abgelehnt: `explain`, `get_er_diagram`,
`propose_edits` und `query_across_databases`. Siehe [Agenten und die CLI](../../../../AGENTS.md).

## Bekannte Grenzen

- Die App lehnt Elasticsearch älter als 7.10 und OpenSearch älter als 1.0 ab: Anders als bei den anderen
  Datenbanken schlägt die Verbindung mit „older than … the oldest version supported“ fehl.
- Getestet auf Elasticsearch 8 und OpenSearch 2, nicht auf 7.x.

## Getestet auf

Die Konformitätssuite läuft auf Elasticsearch 8.15.3 (Anmeldung `elastic`) und OpenSearch 2.17.1: Serverinfo,
SQL mit Typen und Paging, Suchen, die über eine Seite hinaus blättern, der Katalog und die Mappings, Speichern
der Rasteränderungen und ihr Rückgängigmachen, Abbrechen einer langsamen Suche und die Fehler des Servers.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
