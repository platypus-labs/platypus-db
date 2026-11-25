<!-- source: 81c6c1d2c8d1 -->
[← Bases de données](README.md)

# MongoDB

Le pilote est le pilote Rust officiel de MongoDB (avec rustls, donc sans OpenSSL). Il ouvre MongoDB 5.0 et les
versions suivantes. L'éditeur prend la **syntaxe de mongosh**, pas du SQL. Le pilote fait partie de
l'application ; rien n'est téléchargé.

## Connexion

| Vous collez | Interprété comme |
|---|---|
| `mongodb://user:pass@host:27017/shop?authSource=admin&replicaSet=rs0` | la forme URL ; port par défaut 27017 |
| `mongodb://a:27017,b:27018/?authSource=admin` | une liste de seeds, gardée entière dans le champ Hôte |
| `mongodb+srv://user:pass@cluster0.example.net/shop` | SRV : les hôtes viennent du DNS |

Ce qui ne rentre pas dans les champs du formulaire va dans le **champ Base de données**, après un `?`, sous
forme d'options de chaîne de connexion : `shop?authSource=admin&replicaSet=rs0`, avec `srv=true` pour
`mongodb+srv`. Le nom avant le `?` est la base de données dans laquelle les instructions s'exécutent jusqu'à
ce que `use` la change (`test` quand il est vide).

- **Connexion** : un utilisateur et un mot de passe, authentifiés auprès de `admin` sauf si `authSource` dit
  autre chose. Une URL collée dont le chemin nomme une base de données l'utilise comme source
  d'authentification, comme le dit la spécification de la chaîne de connexion. Les options qui contiennent un
  secret (un mot de passe dans `authMechanismProperties`, par exemple) ne sont pas conservées quand vous
  collez une URL ; le formulaire liste ce qu'il a laissé de côté.
- **TLS** : `tls=true` ou `ssl=true` dans l'URL veut dire `verify-full` ; `tls=false` veut dire `disable`.
  MongoDB n'a pas de TLS opportuniste, donc `prefer` (par défaut) garde ce que la chaîne de connexion
  implique : TLS activé pour `mongodb+srv`, désactivé sinon. `require` active TLS et accepte n'importe quel
  certificat ; `verify-ca` et `verify-full` vérifient tous deux la chaîne et le nom d'hôte. `sslrootcert` (ou
  `tlsCAFile`) désigne le fichier de votre CA ; un certificat client peut être donné dans le formulaire, ou
  avec `tlsCertificateKeyFile` dans le champ Base de données. Les options TLS de la chaîne de connexion sont
  conservées dans tous les modes.
- **Par un tunnel SSH**, seul le serveur situé derrière est atteint (`directConnection=true`).

## Privilèges

Le navigateur liste les bases de données et collections que le login peut lister, et échantillonne des
documents pour afficher les champs. Rien d'autre n'est lu pour l'administration : il n'y a pas d'onglets
d'administration. Les instructions que vous écrivez demandent les rôles dont elles ont besoin.

## Ce qui fonctionne

- Les instructions en syntaxe mongosh, avec les valeurs du shell (`ObjectId(...)`, `ISODate(...)`,
  `NumberLong(...)`, `/regex/i`) et l'Extended JSON :
  - **Lecture** : `find` avec `sort`, `limit`, `skip`, `projection`, `collation`, `hint`, `min`, `max`,
    `maxTimeMS`, `readConcern` et les semblables, `findOne`, `aggregate`, `countDocuments`,
    `estimatedDocumentCount`, `distinct`.
  - **Écriture** : `insertOne`/`insertMany`, `updateOne`/`updateMany`, `replaceOne`,
    `deleteOne`/`deleteMany`, `bulkWrite`, `findOneAndUpdate`/`findOneAndReplace`/`findOneAndDelete` et
    `findAndModify`, avec leurs options (`arrayFilters`, `collation`, `hint`, `let`, `upsert`,
    `ordered`, `writeConcern`). Une option que PlatypusDB ne connaît pas est refusée par son nom, jamais
    ignorée.
  - **Index et collections** : `createIndex`, `createIndexes`, `dropIndex`, `dropIndexes`, `hideIndex`,
    `unhideIndex`, `getIndexes`, `createCollection`, `createView`, `drop`, `renameCollection`,
    `convertToCapped`, `stats`, `validate`, `latencyStats`, `dataSize`, `storageSize`, `totalIndexSize`,
    `totalSize`, `isCapped`, `exists`.
  - **Plans** : `.explain()` après une instruction ou `db.c.explain().find(…)` avant elle, avec un niveau de
    détail, et **Explain** de l'éditeur. Un plan n'applique jamais une écriture.
  - **Le serveur** : `db.runCommand`, `db.adminCommand`, `db.aggregate`, `db.serverStatus`, `db.currentOp`,
    `db.killOp`, le profiler, les utilisateurs et rôles (`db.createUser` et les semblables), `rs.status`,
    `rs.conf`, `sh.status`, `show dbs`, `show collections`, `show users`, `show roles`, `show profile`,
    `show logs` et `use`.
- Les résultats sont une grille des champs de premier niveau, avec le document sélectionné sous forme d'arbre
  à côté (`F7`). Le navigateur liste les bases de données, les collections, les champs échantillonnés dans
  les documents, et les index ; l'éditeur complète les collections, les méthodes et les champs.
- Annuler arrête une instruction en cours ; un résultat abandonné libère la connexion.
- Le **commit manuel** sur un replica set ou un cluster shardé (voir [Transactions](#transactions)).
- **La modification de documents dans la grille** (voir
  [Modifier les documents dans la grille](#modifier-les-documents-dans-la-grille)).
- La définition d'une collection sous forme de DDL.
- **Générer des données** remplit une collection de faux documents.
- Le même détail est dans [FEATURES.md](../../FEATURES.md#mongodb).

## Ce qui ne fonctionne pas

- **Seuls certains résultats se modifient dans la grille** : les documents d'une collection (voir
  [Modifier les documents dans la grille](#modifier-les-documents-dans-la-grille)). Tout autre résultat, comme
  une agrégation, une vue ou un `findOne`, reste en lecture seule. Modifiez ces documents avec `updateOne`,
  `insertOne` et les semblables.
- **Pas de transactions sur un serveur autonome** : MongoDB ne les exécute que sur un replica set ou un
  cluster shardé. Le commit manuel y est refusé, en le disant.
- **Pas de JavaScript** : l'éditeur exécute des instructions, donc `forEach`, `map`, `next`, `mapReduce`, les
  anciens constructeurs de bulk (utilisez `bulkWrite`) et les variables sont refusés, en le disant. `watch`
  est refusé aussi : un change stream prendrait la main sur la connexion. Un `.comment()` sur une lecture est
  écarté : l'application s'en sert elle-même pour retrouver l'opération à annuler.
- `bulkWrite` n'est pas atomique, comme dans mongosh : ce qui s'est exécuté avant un échec reste, et l'erreur
  dit jusqu'où il est allé. Les insertions voisines voyagent ensemble, toute autre écriture fait son propre
  voyage.
- Pas de SQL : pas d'éditeur de tables, de diagramme ER, de comparaison de schémas, de constructeur de
  requêtes, d'import SQL, d'instantanés avant modifications, ni de mode terminal (`mongosh` ne prend le mot
  de passe que sur sa ligne de commande).

## Modifier les documents dans la grille

La grille modifie le résultat d'un simple `find` sur une collection : la visionneuse de données, ou une
instruction que vous saisissez, comme `db.orders.find({ status: 'paid' }).sort({ _id: 1 }).limit(20)`. La
collection peut être nommée avec `getCollection("my orders")` ou `db.getSiblingDB("shop").orders`. Après
`use`, la collection est celle de la base de données vers laquelle vous avez basculé. Une projection convient
quand elle ne fait que lister des champs à inclure et garde `_id`. Les vues, les agrégations, `findOne`,
`distinct`, les comptages, une projection sans `_id` et une projection qui exclut des champs restent en
lecture seule.

Un document est retrouvé par son `_id`. Un résultat sans `_id` est en lecture seule. **Aperçu du SQL** montre
le mongosh qui va s'exécuter, une instruction par document :

- un document modifié est `updateOne({ _id: … }, { $set: { … } })`. Une cellule NULL écrit `null`. Quand un
  nombre change, la mise à jour est plutôt un pipeline, `[{ $set: { … } }]` : le nombre est écrit avec un
  `$switch` sur le type que le document stocke (voir plus bas), et toute autre valeur avec un `$literal`, si
  bien qu'une chaîne comme `$total` ou un document avec des clés `$` est enregistré tel quel ;
- un nouveau document est `insertOne({ … })`. Les cellules laissées NULL ne sont pas envoyées. Sans `_id`,
  l'application crée un `ObjectId` ;
- un document supprimé est `deleteOne({ _id: … })`.

Les valeurs sont écrites avec le type BSON de leur colonne : `bool`, `string`, `objectId`, `date`, et
`document` et `array` à partir du JSON que la grille affiche. Une date s'écrit comme `2024-01-02 10:30:00`,
avec ou sans fuseau horaire (UTC sans fuseau). Les nombres à l'intérieur d'un document ou d'un tableau
gardent le type que leur donne le JSON : un nombre entier est un int (un long au-delà de 32 bits), un nombre
avec une fraction ou un exposant un double.

Un nombre garde le type que le document stocke, `int`, `long`, `double` ou `decimal`, chaque fois que ce type
contient exactement la nouvelle valeur : le type d'une colonne est seulement ce que ses documents partagent
(des int et des long font une colonne `long`, l'un d'eux avec des double une colonne `double`), il ne dit donc
pas ce qu'un document précis contient. Un int modifié en `7` reste un int, un long un long, un double un
double. Une valeur que le type stocké ne peut pas contenir prend le type de la colonne, ou le premier type
numérique qui la contient : `3.5` dans un champ int devient un double, `3000000000` un long. Un long au-delà
de 2^53 n'est jamais écrit comme un double, ce qui perdrait des chiffres. Rien n'est converti sur le serveur :
l'application écrit seulement le nombre dans les types dont elle sait qu'ils le contiennent exactement.

Ces cas sont refusés avant que rien ne s'exécute, avec la raison :

- une modification de `_id` : MongoDB ne le change jamais. Supprimez le document et insérez-le de nouveau ;
- une colonne de types mélangés (`mixed`) : le texte d'une valeur ne distingue pas une chaîne d'un nombre ;
- les données binaires et les autres types que la grille ne peut pas réécrire exactement (expressions
  régulières, timestamps, JavaScript) ;
- un champ dont le nom contient un `.` ou commence par `$` : `$set` le lirait comme un chemin ou un
  opérateur ;
- une valeur qui ne convient pas à son type, comme `many` dans une colonne `int` ou un tableau dans une
  colonne `document`.

Chaque instruction doit modifier exactement un document. Si l'une n'en trouve aucun, parce que le document a
été supprimé ou que son `_id` a changé depuis la lecture de la grille, l'enregistrement s'arrête et dit
quelle commande.

**Tout ou rien demande un replica set.** Sur un replica set ou un cluster shardé, un enregistrement s'exécute
dans une transaction à lui et ne valide que si toutes les commandes ont fonctionné. Si vous avez une
transaction ouverte (commit manuel), l'enregistrement s'y exécute et attend Valider. Si une commande échoue,
MongoDB n'a pas de savepoints : toute la transaction est annulée, avec les modifications que vous n'aviez pas
encore validées, et l'erreur le dit. Sur un serveur autonome, il n'y a pas de transactions : les commandes
s'exécutent l'une après l'autre, et un échec arrête les suivantes. Les commandes précédentes restent
enregistrées, et l'erreur dit combien.

Le script d'annulation qui suit un enregistrement remet les documents en place : `$set` des anciennes valeurs
pour les modifications, `deleteOne` pour les nouveaux documents, `insertOne` des champs chargés pour les
documents supprimés. Les lignes qui commencent par `//` disent ce qu'il ne peut pas restaurer exactement : les
champs que le résultat n'affichait pas, les colonnes de types que la grille ne peut pas écrire, les champs qui
étaient NULL (ils reviennent en `null`, ou sont laissés de côté), et les nombres dont la modification a pu
changer le type (ils reviennent comme des nombres du type qu'ils ont alors, par le même `$switch`).

## Transactions

Le commit manuel fonctionne sur un replica set ou un cluster shardé (un `mongos`) ; un replica set à un seul
nœud suffit. Sur un serveur autonome, il est refusé : le bouton **Validation auto** est désactivé et dit
pourquoi. Une connexion de production vers un replica set s'ouvre en commit manuel, avec Valider et Annuler.
La connexion garde une session : la première modification après le passage en commit manuel, ou après un
Valider ou un Annuler, ouvre une transaction dans cette session (une insertion, une mise à jour ou une
suppression, `createCollection`, `createIndex`, un enregistrement de la grille), et Valider ou Annuler la
termine. Les lectures qui précèdent n'en ouvrent aucune : elles voient ce qui est validé, comme en
autocommit, si bien que parcourir des données ne laisse jamais une transaction ouverte que le serveur
terminerait après sa durée de vie. Une fois qu'une transaction est ouverte, vos lectures s'y exécutent et
voient ses écritures ; les autres sessions ne les voient qu'après Valider.

- **Dans la transaction**, une fois qu'une est ouverte (les lectures) ou en l'ouvrant (le reste) : `find`,
  `findOne`, `aggregate`, `countDocuments`, `distinct`, les insertions, mises à jour, remplacements et
  suppressions, `bulkWrite`, les méthodes `findOneAnd…`, `createCollection` et `createIndex`, et
  `db.runCommand` ou `db.adminCommand` avec une commande qui lit ou écrit des documents (`find`, `aggregate`,
  `insert`, `update`, `delete`, `findAndModify`, …). MongoDB n'accepte un index dans une transaction que sur
  une collection créée dans la même transaction : sur une collection qui existe, le serveur le refuse et
  termine la transaction.
- **En dehors**, comme en mode autocommit, sans y toucher : ce qui lit des métadonnées ou parle au serveur.
  `show …`, `use`, `db.stats()`, `db.getCollectionNames()`, `getIndexes`, les statistiques de la collection,
  `estimatedDocumentCount`, `.explain()`, les autres commandes de `db.runCommand` et `db.adminCommand`,
  `rs.*`, `sh.*`, les utilisateurs et rôles, le profiler, le navigateur et la complétion. Les lectures des
  bases de données `admin`, `config` et `local` et des collections `system.*` s'exécutent aussi en dehors.
- **Refusé en commit manuel** : `drop`, `renameCollection`, `dropIndex`, `dropIndexes`, `hideIndex`,
  `convertToCapped`, `createView`, un `createCollection` de série temporelle, un `aggregate` avec `$out` ou
  `$merge`, et les écritures vers `admin`, `config`, `local` ou `system.*`. MongoDB ne les exécute pas dans une
  transaction, et exécutées en dehors elles s'appliqueraient aussitôt, ce qu'Annuler ne pourrait pas défaire.
  L'erreur le dit ; rien n'est envoyé et la transaction ouverte reste comme elle était. Passez en autocommit
  pour les exécuter.
- Un résultat lu dans une transaction continue de se charger pendant que vous faites défiler, un lot à la
  fois, et d'autres instructions peuvent s'exécuter entre-temps. Un résultat lu avant l'ouverture de la
  transaction continue de se charger en dehors. Ses colonnes viennent de ses premiers documents. Après Valider
  ou Annuler, le reste ne peut plus être lu : exécutez de nouveau l'instruction.

**Quand le serveur termine une transaction.** MongoDB annule une transaction qui reste ouverte plus longtemps
que `transactionLifetimeLimitSeconds`, 60 secondes par défaut. Il en annule aussi une pour certaines erreurs :
une clé en double, un conflit d'écriture avec une autre session, une instruction qu'il refuse dans une
transaction, une limite de temps (`maxTimeMS`) atteinte. D'autres erreurs, comme une étape de pipeline
inconnue, la laissent ouverte. Après une erreur, PlatypusDB demande au serveur si la transaction existe
encore. Quand ce n'est pas le cas, l'erreur de l'instruction ou de Valider dit que le serveur a terminé la
transaction et que ses modifications ont été abandonnées (avec la limite de temps quand c'était la raison), la
transaction s'affiche comme fermée, et l'instruction suivante en ouvre une nouvelle. Annuler une instruction
dans une transaction annule la transaction : le serveur termine une transaction dont l'opération est tuée, et
ce que l'instruction a fait est inconnu.

## Agents

`run_query`, `sample_rows` (`find({}).limit(n)`) et le reste fonctionnent avec des instructions mongosh ; une
instruction qui n'est pas une simple requête fait d'abord l'objet d'une question. Refusés sur MongoDB :
`explain`, `get_er_diagram` et `query_across_databases` ; `estimate_change` répond par une note.

`propose_edits` propose des documents à insérer, modifier ou supprimer dans une collection (pas une vue) : la
collection s'ouvre dans sa visionneuse de données avec un `find` des `_id` proposés et les modifications en
attente, que vous enregistrez ou abandonnez comme vos propres modifications. Un document est désigné par son
seul `_id` ; un `_id` de type `ObjectId` est envoyé sous forme de ses 24 chiffres hexadécimaux et lu comme un
`ObjectId` parce que le `_id` de la collection en est un. Les valeurs sont vérifiées selon les règles de
l'enregistrement de la grille (voir plus haut), avant que rien ne s'ouvre. Les colonnes sensibles ne sont
masquées que par champ de premier niveau : un champ sensible à l'intérieur d'un document n'est pas distingué.
Voir [Agents et la CLI](../../AGENTS.md).

## Limites connues

- Plus ancien que 5.0 : il se connecte avec un avertissement disant que certaines fonctionnalités peuvent ne
  pas marcher.
- Le pilote est un pool qui se reconnecte tout seul, donc une connexion perdue n'est pas signalée comme pour
  les autres bases de données.

## Testé sur

La suite de conformité tourne sur MongoDB 5.0 et 8.0 : informations du serveur, métadonnées, annulation,
résultats abandonnés, erreurs avec leur position, faux documents, et valeurs de documents ultérieurs qui
diffèrent du premier. Les transactions sont testées sur un replica set à un seul nœud de MongoDB 8.0 :
commit, rollback, lectures dedans et dehors, un résultat de plusieurs lots, métadonnées en commit manuel, et
une transaction que le serveur a terminée pour sa limite de temps.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
