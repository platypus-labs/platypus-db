<!-- source: 81c6c1d2c8d1 -->
[← Datenbanken](README.md)

# MongoDB

Der Treiber ist der offizielle Rust-Treiber von MongoDB (mit rustls, also ohne OpenSSL). Er öffnet MongoDB 5.0
und neuer. Der Editor nimmt **mongosh-Syntax**, nicht SQL. Der Treiber ist Teil der App; es wird nichts
heruntergeladen.

## Verbinden

| Du fügst ein | Gelesen als |
|---|---|
| `mongodb://user:pass@host:27017/shop?authSource=admin&replicaSet=rs0` | die URL-Form; Standardport 27017 |
| `mongodb://a:27017,b:27018/?authSource=admin` | eine Seed-Liste, im Feld Host als Ganzes behalten |
| `mongodb+srv://user:pass@cluster0.example.net/shop` | SRV: Die Hosts kommen aus DNS |

Was nicht in die Felder des Formulars passt, kommt als Optionen der Verbindungszeichenfolge nach einem `?` in das
**Feld Datenbank**: `shop?authSource=admin&replicaSet=rs0`, mit `srv=true` für `mongodb+srv`. Der Name vor dem `?`
ist die Datenbank, in der Anweisungen laufen, bis `use` sie ändert (`test`, wenn leer).

- **Anmelden**: ein Benutzer und ein Passwort, gegen `admin` authentifiziert, außer `authSource` sagt etwas
  anderes. Eine eingefügte URL, deren Pfad eine Datenbank nennt, nutzt sie als Auth-Quelle, wie die Spezifikation
  der Verbindungszeichenfolge es sagt. Optionen, die ein Geheimnis enthalten (zum Beispiel ein Passwort in
  `authMechanismProperties`), werden beim Einfügen einer URL nicht behalten; das Formular listet auf, was es
  weggelassen hat.
- **TLS**: `tls=true` oder `ssl=true` in der URL bedeutet `verify-full`; `tls=false` bedeutet `disable`.
  MongoDB hat kein opportunistisches TLS, daher behält `prefer` (der Standard), was die Verbindungszeichenfolge
  nahelegt: TLS an bei `mongodb+srv`, sonst aus. `require` schaltet TLS ein und akzeptiert jedes Zertifikat;
  `verify-ca` und `verify-full` prüfen beide die Kette und den Hostnamen. `sslrootcert` (oder `tlsCAFile`) nennt
  die Datei deiner CA; ein Client-Zertifikat lässt sich im Formular angeben oder mit `tlsCertificateKeyFile` im
  Feld Datenbank. Die TLS-Optionen der Verbindungszeichenfolge bleiben in jedem Modus erhalten.
- **Durch einen SSH-Tunnel** wird nur der eine Server dahinter erreicht (`directConnection=true`).

## Berechtigungen

Der Navigator listet Datenbanken und Collections auf, die die Anmeldung auflisten kann, und zieht Stichproben von
Dokumenten, um Felder zu zeigen. Sonst wird für die Verwaltung nichts gelesen: Es gibt keine Verwaltungs-Tabs. Die
Anweisungen, die du schreibst, brauchen die Rollen, die sie brauchen.

## Was funktioniert

- Anweisungen in mongosh-Syntax, mit den Werten der Shell (`ObjectId(...)`, `ISODate(...)`, `NumberLong(...)`,
  `/regex/i`) und Extended JSON:
  - **Lesen**: `find` mit `sort`, `limit`, `skip`, `projection`, `collation`, `hint`, `min`, `max`, `maxTimeMS`,
    `readConcern` und ähnlichem, `findOne`, `aggregate`, `countDocuments`, `estimatedDocumentCount`, `distinct`.
  - **Schreiben**: `insertOne`/`insertMany`, `updateOne`/`updateMany`, `replaceOne`, `deleteOne`/`deleteMany`,
    `bulkWrite`, `findOneAndUpdate`/`findOneAndReplace`/`findOneAndDelete` und `findAndModify`, mit ihren
    Optionen (`arrayFilters`, `collation`, `hint`, `let`, `upsert`, `ordered`, `writeConcern`). Eine Option, die
    PlatypusDB nicht kennt, wird mit Namen abgelehnt, nie fallen gelassen.
  - **Indizes und Collections**: `createIndex`, `createIndexes`, `dropIndex`, `dropIndexes`, `hideIndex`,
    `unhideIndex`, `getIndexes`, `createCollection`, `createView`, `drop`, `renameCollection`, `convertToCapped`,
    `stats`, `validate`, `latencyStats`, `dataSize`, `storageSize`, `totalIndexSize`, `totalSize`, `isCapped`,
    `exists`.
  - **Pläne**: `.explain()` nach einer Anweisung oder `db.c.explain().find(…)` davor, mit einer Ausführlichkeit,
    und das **Explain** des Editors. Ein Plan wendet nie einen Schreibzugriff an.
  - **Der Server**: `db.runCommand`, `db.adminCommand`, `db.aggregate`, `db.serverStatus`, `db.currentOp`,
    `db.killOp`, der Profiler, Benutzer und Rollen (`db.createUser` und ähnliche), `rs.status`, `rs.conf`,
    `sh.status`, `show dbs`, `show collections`, `show users`, `show roles`, `show profile`, `show logs` und
    `use`.
- Ergebnisse sind ein Raster der Felder der obersten Ebene, mit dem fokussierten Dokument als Baum daneben
  (`F7`). Der Navigator listet Datenbanken, Collections, aus den Dokumenten gezogene Felder und Indizes auf; der
  Editor vervollständigt Collections, Methoden und Felder.
- Abbrechen stoppt eine laufende Anweisung; ein verlassenes Ergebnis gibt die Verbindung frei.
- **Manueller Commit** bei einem Replica Set oder einem Sharded Cluster (siehe
  [Transaktionen](#transaktionen)).
- **Dokumente im Raster bearbeiten** (siehe [Dokumente im Raster bearbeiten](#dokumente-im-raster-bearbeiten)).
- Die Definition einer Collection als DDL.
- **Daten generieren…** füllt eine Collection mit Testdokumenten.
- Dasselbe Detail steht in [FEATURES.md](../../../../FEATURES.md#mongodb).

## Was nicht

- **Nur manche Ergebnisse sind im Raster bearbeitbar**: die Dokumente einer Collection (siehe
  [Dokumente im Raster bearbeiten](#dokumente-im-raster-bearbeiten)). Jedes andere Ergebnis, etwa eine
  Aggregation, eine View oder ein `findOne`, bleibt schreibgeschützt. Ändere diese Dokumente mit `updateOne`,
  `insertOne` und ähnlichem.
- **Keine Transaktionen auf einem Standalone-Server**: MongoDB führt sie nur auf einem Replica Set oder einem
  Sharded Cluster aus. Manueller Commit wird dort abgelehnt und sagt das.
- **Kein JavaScript**: Der Editor führt Anweisungen aus, daher werden `forEach`, `map`, `next`, `mapReduce`, die
  älteren Bulk-Builder (nimm `bulkWrite`) und Variablen abgelehnt und sagen das. Auch `watch` wird abgelehnt: Ein
  Change Stream würde die Verbindung übernehmen. Ein `.comment()` bei einem Lesezugriff wird weggelassen: Mit dem
  eigenen der App findet sie die Operation, um sie abzubrechen.
- `bulkWrite` ist nicht atomar, wie in mongosh: Was vor einem Fehler lief, bleibt, und der Fehler sagt, wie weit
  es kam. Benachbarte Inserts reisen zusammen, jeder andere Schreibzugriff auf eigener Fahrt.
- Kein SQL: kein Tabelleneditor, kein ER-Diagramm, kein Schemavergleich, kein Query Builder, kein SQL-Import,
  keine Snapshots vor Änderungen und kein Terminalmodus (`mongosh` nimmt das Passwort nur auf seiner
  Kommandozeile).

## Dokumente im Raster bearbeiten

Das Raster bearbeitet das Ergebnis eines einfachen `find` auf eine Collection: den Datenviewer oder eine
Anweisung, die du tippst, etwa `db.orders.find({ status: 'paid' }).sort({ _id: 1 }).limit(20)`. Die Collection
lässt sich mit `getCollection("my orders")` oder `db.getSiblingDB("shop").orders` benennen. Nach `use` ist die
Collection die der Datenbank, zu der du gewechselt hast. Eine Projektion ist in Ordnung, wenn sie nur Felder zum
Einschließen auflistet und `_id` behält. Views, Aggregationen, `findOne`, `distinct`, Zählungen, eine Projektion
ohne `_id` und eine Projektion, die Felder auslässt, bleiben schreibgeschützt.

Ein Dokument wird über seine `_id` gefunden. Ein Ergebnis ohne `_id` ist schreibgeschützt. **SQL-Vorschau** zeigt
das mongosh, das läuft, eine Anweisung pro Dokument:

- ein bearbeitetes Dokument ist `updateOne({ _id: … }, { $set: { … } })`. Eine NULL-Zelle schreibt `null`. Ändert
  sich eine Zahl, ist das Update stattdessen eine Pipeline, `[{ $set: { … } }]`: Die Zahl wird mit einem `$switch`
  auf den Typ geschrieben, den das Dokument speichert (siehe unten), und jeder andere Wert als `$literal`, damit
  ein String wie `$total` oder ein Dokument mit `$`-Schlüsseln so gespeichert wird, wie es ist;
- ein neues Dokument ist `insertOne({ … })`. Zellen, die NULL bleiben, werden nicht gesendet. Ohne `_id` erzeugt
  die App eine `ObjectId`;
- ein gelöschtes Dokument ist `deleteOne({ _id: … })`.

Werte werden mit dem BSON-Typ ihrer Spalte geschrieben: `bool`, `string`, `objectId`, `date` sowie `document` und
`array` aus dem JSON, das das Raster zeigt. Ein Datum wird wie `2024-01-02 10:30:00` geschrieben, mit oder ohne
Zeitzone (ohne eine UTC). Zahlen innerhalb eines Dokuments oder Arrays behalten den Typ, den JSON ihnen gibt: Eine
ganze Zahl ist ein int (ein long jenseits von 32 Bit), eine Zahl mit Bruchteil oder Exponent ein double.

Eine Zahl behält den Typ, den das Dokument speichert, `int`, `long`, `double` oder `decimal`, wann immer dieser
Typ den neuen Wert genau fasst: Der Typ einer Spalte ist nur, was ihre Dokumente gemeinsam haben (ints und longs
ergeben eine `long`-Spalte, jede davon mit doubles eine `double`-Spalte), er sagt also nicht, was ein einzelnes
Dokument hält. Ein int, der auf `7` bearbeitet wird, bleibt ein int, ein long ein long, ein double ein double. Ein
Wert, den der gespeicherte Typ nicht fassen kann, nimmt den Typ der Spalte oder den ersten numerischen Typ, der
ihn fasst: `3.5` in einem int-Feld wird ein double, `3000000000` ein long. Ein long jenseits von 2^53 wird nie als
double geschrieben, das würde Stellen verlieren. Auf dem Server wird nichts umgewandelt: Die App schreibt die Zahl
nur in den Typen, von denen sie weiß, dass sie sie genau fassen.

Diese werden vor jeder Ausführung mit Begründung abgelehnt:

- eine Änderung von `_id`: MongoDB ändert sie nie. Lösche das Dokument und füge es neu ein;
- eine Spalte mit gemischten Typen (`mixed`): Der Text eines Werts unterscheidet keinen String von einer Zahl;
- Binärdaten und die anderen Typen, die das Raster nicht genau zurückschreiben kann (reguläre Ausdrücke,
  Timestamps, JavaScript);
- ein Feld mit einem `.` im Namen oder einem Namen, der mit `$` beginnt: `$set` würde ihn als Pfad oder Operator
  lesen;
- ein Wert, der nicht zu seinem Typ passt, etwa `many` in einer `int`-Spalte oder ein Array in einer
  `document`-Spalte.

Jede Anweisung muss genau ein Dokument ändern. Findet eine keines, weil das Dokument gelöscht wurde oder sich seine
`_id` geändert hat, seit das Raster es gelesen hat, stoppt das Speichern und nennt den Befehl.

**Alles oder nichts braucht ein Replica Set.** Bei einem Replica Set oder einem Sharded Cluster läuft ein Speichern
in einer eigenen Transaktion und committet nur, wenn jeder Befehl funktioniert hat. Hast du eine Transaktion offen
(manueller Commit), läuft das Speichern darin und wartet auf Commit. Schlägt ein Befehl fehl, hat MongoDB keine
Savepoints: Die ganze Transaktion wird zurückgerollt, mit den Änderungen, die du noch nicht committet hattest, und
der Fehler sagt das. Auf einem Standalone-Server gibt es keine Transaktionen: Die Befehle laufen nacheinander, und
ein Fehler stoppt den Rest. Die Befehle davor bleiben gespeichert, und der Fehler sagt, wie viele.

Das Rückgängig-Skript, das auf ein Speichern folgt, holt die Dokumente zurück: `$set` der alten Werte bei
Bearbeitungen, `deleteOne` bei neuen Dokumenten, `insertOne` der geladenen Felder bei gelöschten. Zeilen, die mit
`//` beginnen, sagen, was es nicht genau wiederherstellen kann: Felder, die das Ergebnis nicht zeigte, Spalten mit
Typen, die das Raster nicht schreiben kann, Felder, die NULL waren (sie kommen als `null` zurück oder werden
ausgelassen), und Zahlen, deren Typ die Bearbeitung vielleicht geändert hat (sie kommen als Zahlen des Typs
zurück, den sie dann haben, über dasselbe `$switch`).

## Transaktionen

Manueller Commit funktioniert bei einem Replica Set oder einem Sharded Cluster (einem `mongos`); ein
Replica Set mit einem Knoten genügt. Auf einem Standalone-Server wird er abgelehnt: Die Schaltfläche
**Auto-Commit** ist deaktiviert und sagt, warum. Eine Produktionsverbindung zu einem Replica Set öffnet sich mit
manuellem Commit, mit Commit und Rollback. Die Verbindung hält eine Sitzung: Die erste Änderung, nachdem du auf
manuellen Commit umgestellt hast oder nach einem Commit oder Rollback, öffnet darin eine Transaktion (ein Insert,
Update oder Delete, `createCollection`, `createIndex`, ein Speichern des Rasters), und Commit oder Rollback beendet
sie. Lesezugriffe davor öffnen keine: Sie sehen, was committet ist, wie bei Autocommit, sodass das Durchsuchen von
Daten nie eine Transaktion offen lässt, die der Server nach ihrer Lebensdauer beendet. Ist eine Transaktion offen,
laufen deine Lesezugriffe darin und sehen ihre Schreibzugriffe; andere Sitzungen sehen sie erst nach Commit.

- **In der Transaktion**, sobald eine offen ist (die Lesezugriffe) oder sie öffnet (der Rest): `find`, `findOne`,
  `aggregate`, `countDocuments`, `distinct`, die Inserts, Updates, Replaces und Deletes, `bulkWrite`, die
  `findOneAnd…`-Methoden, `createCollection` und `createIndex` sowie `db.runCommand` oder `db.adminCommand` mit
  einem Befehl, der Dokumente liest oder schreibt (`find`, `aggregate`, `insert`, `update`, `delete`,
  `findAndModify`, …). MongoDB nimmt einen Index in einer Transaktion nur bei einer Collection, die in derselben
  Transaktion angelegt wurde: Bei einer vorhandenen lehnt der Server ihn ab und beendet die Transaktion.
- **Außerhalb davon**, wie im Autocommit-Modus, ohne sie zu berühren: was Metadaten liest oder mit dem Server
  spricht. `show …`, `use`, `db.stats()`, `db.getCollectionNames()`, `getIndexes`, die Statistiken der Collection,
  `estimatedDocumentCount`, `.explain()`, die anderen Befehle von `db.runCommand` und `db.adminCommand`, `rs.*`,
  `sh.*`, Benutzer und Rollen, der Profiler, der Navigator und die Vervollständigung. Lesezugriffe auf die
  Datenbanken `admin`, `config` und `local` und auf `system.*`-Collections laufen ebenfalls außerhalb.
- **Im manuellen Commit abgelehnt**: `drop`, `renameCollection`, `dropIndex`, `dropIndexes`, `hideIndex`,
  `convertToCapped`, `createView`, ein Time-Series-`createCollection`, ein `aggregate` mit `$out` oder `$merge` und
  Schreibzugriffe auf `admin`, `config`, `local` oder `system.*`. MongoDB führt sie nicht in einer Transaktion aus,
  und außerhalb würden sie sofort wirken, was Rollback nicht rückgängig machen könnte. Der Fehler sagt das; es wird
  nichts gesendet, und die offene Transaktion bleibt, wie sie war. Wechsle zu Autocommit, um sie auszuführen.
- Ein in einer Transaktion gelesenes Ergebnis lädt beim Scrollen weiter, Batch für Batch, und dazwischen können
  andere Anweisungen laufen. Ein Ergebnis, das gelesen wurde, bevor die Transaktion sich öffnete, lädt außerhalb
  weiter. Seine Spalten kommen aus seinen ersten Dokumenten. Nach Commit oder Rollback lässt sich der Rest nicht
  mehr lesen: Führe die Anweisung erneut aus.

**Wenn der Server eine Transaktion beendet.** MongoDB bricht eine Transaktion ab, die länger offen bleibt als
`transactionLifetimeLimitSeconds`, standardmäßig 60 Sekunden. Es bricht sie auch bei manchen Fehlern ab: ein
doppelter Schlüssel, ein Schreibkonflikt mit einer anderen Sitzung, eine Anweisung, die es in einer Transaktion
ablehnt, ein erreichtes Zeitlimit (`maxTimeMS`). Andere Fehler, etwa eine unbekannte Pipeline-Stufe, lassen sie
offen. Nach einem Fehler fragt PlatypusDB den Server, ob die Transaktion noch existiert. Wenn nicht, sagt der Fehler
der Anweisung oder von Commit, dass der Server die Transaktion beendet hat und ihre Änderungen verworfen wurden (mit
dem Zeitlimit, wenn das der Grund war), die Transaktion wird als geschlossen angezeigt, und die nächste Anweisung
öffnet eine neue. Das Abbrechen einer Anweisung in einer Transaktion rollt die Transaktion zurück: Der Server beendet
eine Transaktion, deren Operation abgebrochen wird, und was die Anweisung getan hat, ist unbekannt.

## Agenten

`run_query`, `sample_rows` (`find({}).limit(n)`) und der Rest funktionieren mit mongosh-Anweisungen; bei einer
Anweisung, die keine einfache Abfrage ist, wird zuerst nachgefragt. Bei MongoDB abgelehnt: `explain`,
`get_er_diagram` und `query_across_databases`; `estimate_change` antwortet mit einem Hinweis.

`propose_edits` schlägt Dokumente zum Einfügen, Aktualisieren oder Löschen in einer Collection vor (nicht einer
View): Die Collection öffnet sich in ihrem Datenviewer mit einem `find` der vorgeschlagenen `_id`s und den
ausstehenden Änderungen, die du wie deine eigenen Bearbeitungen speicherst oder verwirfst. Ein Dokument wird allein
über `_id` benannt; eine `_id` vom Typ `ObjectId` wird als ihre 24 Hexadezimalziffern gesendet und als `ObjectId`
gelesen, weil die `_id` der Collection eine ist. Werte werden nach den Regeln des Speicherns im Raster geprüft
(siehe oben), bevor sich etwas öffnet. Sensible Spalten werden nur nach Feld der obersten Ebene maskiert: Ein
sensibles Feld innerhalb eines Dokuments wird nicht unterschieden. Siehe [Agenten und die CLI](../../../../AGENTS.md).

## Bekannte Grenzen

- Älter als 5.0: Die App verbindet sich mit einer Warnung, dass manche Funktionen vielleicht nicht gehen.
- Der Treiber ist ein Pool, der sich selbst wieder verbindet, daher wird eine verlorene Verbindung nicht so
  gemeldet wie bei den anderen Datenbanken.

## Getestet auf

Die Konformitätssuite läuft auf MongoDB 5.0 und 8.0: Serverinfo, Metadaten, Abbrechen, verlassene Ergebnisse,
Fehler mit ihrer Position, Testdokumente und Werte in späteren Dokumenten, die vom ersten abweichen. Transaktionen
sind auf einem Replica Set mit einem Knoten von MongoDB 8.0 getestet: Commit, Rollback, Lesezugriffe innerhalb und
außerhalb, ein Ergebnis aus mehreren Batches, Metadaten im manuellen Commit und eine Transaktion, die der Server
wegen ihres Zeitlimits beendet hat.

Siehe auch: [Verbinden](../connect.md), [Fehlerbehebung](../troubleshooting.md).
