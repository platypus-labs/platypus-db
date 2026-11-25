<!-- source: 641268954234 -->
[← Bases de données](README.md)

# Apache Cassandra et ScyllaDB

Un seul pilote, construit sur le crate `scylla` (avec rustls), parle CQL sur le protocole natif aux deux. Le
serveur dit lequel il est, et la connexion l'affiche. Le pilote fait partie de l'application ; rien n'est
téléchargé. L'éditeur prend du CQL.

## Connexion

| Vous collez | Interprété comme |
|---|---|
| `cassandra://cassandra:s%40cret@node1:9043/shop` | sans TLS, port 9043, keyspace `shop` |
| `scylla://node1` | le même pilote, pour ScyllaDB |
| `cassandras://…` et `scyllas://…` | avec TLS (vérifié : `verify-full`) |

Le port par défaut est 9042. Le champ **Hôte** est un point de contact, ou plusieurs paires `host:port`
séparées par des virgules. Le champ **Keyspace** (facultatif) est le keyspace dans lequel la session démarre ;
le navigateur les liste tous dans les deux cas.

- **Un seul point de contact, ou un tunnel** : ce nœud seul traite chaque instruction. Un cluster annonce les
  adresses propres de ses nœuds, qu'un conteneur, un NAT ou un tunnel cache, donc le pilote ne les suit pas.
  Avec plusieurs points de contact, le pilote les reçoit tous et peut en utiliser n'importe lequel.
- **Connexion** : un utilisateur et un mot de passe, pour l'authentificateur par mot de passe du serveur. Sans
  utilisateur, la connexion est anonyme.
- **TLS** : CQL ne peut pas le négocier, donc TLS est activé pour les URL `cassandras://` et `scyllas://` et
  pour les modes SSL à partir de `require` ; `prefer` est sans TLS. `require` ne vérifie pas le certificat ;
  `verify-ca` et `verify-full` le font. Un fichier CA et un certificat client vont dans les champs TLS.
- **Les instructions n'ont pas de délai d'expiration** propre au pilote : l'une s'exécute jusqu'à sa fin ou
  jusqu'à ce que vous l'annuliez.

## Privilèges

Le navigateur lit `system_schema`, que Cassandra 3 et les versions suivantes et ScyllaDB partagent ; le login
voit ce que le serveur lui laisse lire à cet endroit. Il n'y a pas d'onglets d'administration. Les écritures
demandent les permissions que CQL exige.

## Ce qui fonctionne

- Les scripts CQL, découpés par l'éditeur (les chaînes `$$ … $$`, les commentaires `//` et un `BEGIN BATCH …
  APPLY BATCH` sont compris), avec des résultats typés lus page par page pendant que vous faites défiler,
  grâce à l'état de pagination du serveur.
- Le navigateur : les keyspaces comme schémas, les tables, les vues matérialisées, les types définis par
  l'utilisateur, les fonctions et les agrégats, avec les clés de partition et de clustering de chaque table et
  ses index ; le DDL de chaque objet.
- **La grille modifie les lignes**, par la clé primaire complète (colonnes de partition et de clustering) :
  une ligne modifiée est un `UPDATE`, une nouvelle ligne un `INSERT` qui demande toutes les colonnes de clé,
  une ligne supprimée un `DELETE`. L'aperçu montre les instructions, qui s'exécutent l'une après l'autre ; si
  le serveur en refuse une, les suivantes s'arrêtent. *Script d'annulation* remet les anciennes valeurs. Les
  colonnes de clé ne peuvent pas être modifiées (supprimez la ligne et insérez-en une autre), et les compteurs
  sont refusés.
- Les transactions légères (`IF …`) sont signalées comme telles avec leur résultat.
- Annuler arrête la lecture d'un résultat. Les erreurs gardent le message du serveur.
- `USE keyspace` change le keyspace de la session, que le pilote conserve.

## Ce qui ne fonctionne pas

- **Pas de transactions** : chaque instruction prend effet seule, et il n'y a pas de commit manuel.
- **Les écritures CQL sont des upserts et ne rapportent aucun nombre de lignes**, donc une instruction qui ne
  renvoie aucune ligne n'en signale aucune modifiée ; la mise à jour d'une ligne que quelqu'un a supprimée
  entre-temps la réécrit.
- La visionneuse de données n'a pas de barre de filtre et de tri, et n'affiche pas de nombre de lignes.
- Pas d'explain, et pas d'outils SQL : pas d'éditeur de tables, de diagramme ER, de comparaison de schémas,
  de constructeur de requêtes, d'import SQL, d'instantanés avant modifications, ni de mode terminal (`cqlsh`
  ne prend le mot de passe que sur sa ligne de commande).

## Agents

`run_query` et `sample_rows` (un `SELECT … LIMIT n` CQL) fonctionnent ; une instruction qui modifie des
données fait d'abord l'objet d'une question. Refusés sur Cassandra : `explain` et `propose_edits` ;
`estimate_change` répond par une note. Voir [Agents et la CLI](../../AGENTS.md).

## Limites connues

- Cassandra antérieur à 4.0 et ScyllaDB antérieur à 5.4 sont refusés à la connexion, avec « older than … the
  oldest version supported », contrairement aux autres bases de données, qui se connectent avec un
  avertissement.
- Pas de jointures, pas de `OFFSET`, et les lignes ne se retrouvent que par leur clé primaire, comme CQL le
  veut.

## Testé sur

La suite de conformité tourne sur Apache Cassandra 5 et ScyllaDB 2025.1, sans login : informations du
serveur, scripts, chaque type comme colonne typée, pagination, transactions légères, erreurs, annulation, le
catalogue et son DDL (aussi les fonctions et les agrégats), enregistrement et annulation des modifications de
la grille, un login faux ou absent refusé, et une adresse de tunnel. Cassandra 4.0 et ScyllaDB 5.4 sont les
plus anciennes versions que le pilote accepte et la suite ne les fait pas tourner.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
