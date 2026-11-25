<!-- source: 6f25d7181e09 -->
[← Bases de données](README.md)

# MySQL et MariaDB

Un seul pilote (le client `mysql_async`) ouvre MySQL 5.7 et les versions suivantes, et MariaDB 10.6 et les
versions suivantes. Les deux ont une tuile dans la grille des bases de données ; le pilote reconnaît le
serveur atteint et l'affiche dans la connexion (`Connected: MariaDB 11.4`). Rien n'est téléchargé.

## Connexion

| Vous collez | Interprété comme |
|---|---|
| `mysql://user:pass@host:3306/db?sslMode=REQUIRED` (ou `mariadb://`) | la forme URL ; port par défaut 3306 |
| `jdbc:mysql://host:3307/db`, `jdbc:mariadb://…` | une URL JDBC |
| `Server=db;Port=3306;Database=shop;Uid=app;Pwd=…` | une chaîne de connexion Connector/NET, si elle contient aussi `Driver=` ou si vous l'avez collée dans un formulaire MySQL |

Paramètres d'URL que l'application connaît : `sslmode` (aussi écrit `ssl-mode`, `sslMode`, `ssl_mode`),
`sslrootcert`, `sslcert`, `sslkey`, `application_name` et `connect_timeout`.

**TLS** : `disable` le désactive. `prefer` (par défaut) utilise TLS et se rabat sur une connexion en clair
quand le serveur n'en a pas ou n'offre que des versions que le client refuse (les images de MySQL 5.7 sont
dans ce cas) ; il ne se rabat jamais en clair quand la connexion se fait avec un jeton. `prefer` et `require`
ne vérifient pas le certificat. `verify-ca` vérifie la chaîne avec `sslrootcert` et pas le nom d'hôte ;
`verify-full` vérifie les deux. Les noms propres à MySQL (`REQUIRED`, `VERIFY_IDENTITY`) sont compris. Un
certificat client demande à la fois `sslcert` et `sslkey`.

**La connexion** se fait par mot de passe, ou par un jeton AWS IAM, Google Cloud IAM ou Microsoft Entra ID
créé par l'outil `aws`, `gcloud` ou `az` ; le jeton passe par TLS comme le serveur le demande. Kerberos
(`auth_gssapi` de MariaDB, le plugin Kerberos de MySQL) n'est pas encore pris en charge.

La connexion est toujours TCP : un socket Unix ou un tube nommé Windows n'est pas utilisé.

## Privilèges

Parcourir ne demande que ce que le login peut voir. Les onglets d'administration lisent :

- **Sessions** : la liste des processus, depuis `performance_schema.threads` sur MySQL 8 et depuis
  `information_schema.PROCESSLIST` sur MariaDB, MySQL 5.7 ou un serveur dont `performance_schema` est
  désactivé. Ce que le serveur montre des sessions des autres utilisateurs relève de sa propre règle, que
  l'application ne vérifie pas.
- **Verrous** : `performance_schema.data_lock_waits`, `data_locks` et `metadata_locks` sur MySQL 8 (voir les
  attentes des autres sessions demande `PROCESS` ; sans ce droit, la liste n'a aucune attente et le dit),
  `INNODB_LOCK_WAITS` dans `information_schema` sur MariaDB et les MySQL plus anciens.
- **Instructions les plus coûteuses** : `performance_schema.events_statements_summary_by_digest`. Si
  `performance_schema` est désactivé, l'onglet dit de démarrer le serveur avec.
- **Tables et index** : `information_schema`, plus `performance_schema` pour savoir combien chaque index est
  lu ; sans lui, l'usage des index est inconnu.
- **Annuler l'instruction** et **Terminer la session** exécutent `KILL QUERY` et `KILL CONNECTION`.
- **Paramètres** et **Tableau de bord** : les variables du serveur et les compteurs d'état.
- **Utilisateurs et rôles** : les comptes et les droits du serveur.
- **Maintenance** : `ANALYZE`, `OPTIMIZE` et `CHECK TABLE`.
- **Sauvegarder… et Restaurer…** lancent `mysqldump` et `mysql`, qui doivent être dans le PATH.

## Ce qui fonctionne

- La modification des données dans la grille, par clé primaire (ou clé unique sur des colonnes non nulles, ou
  clé virtuelle).
- Le commit et le rollback manuels, et l'annulation d'une instruction côté serveur.
- Explain : MySQL 8.0.16 et les versions suivantes donnent `EXPLAIN FORMAT=TREE` ; les MySQL plus anciens et
  MariaDB donnent `EXPLAIN FORMAT=JSON`. **Explain analyze** est `EXPLAIN ANALYZE` sur MySQL 8.0.18 et les
  versions suivantes et `ANALYZE FORMAT=JSON` sur MariaDB ; MySQL 5.7 et 8.0.0 à 8.0.17 le refusent.
- Tous les onglets d'administration ci-dessus, et les outils de schéma (éditeur de tables, diagramme ER,
  comparaison).
- Les imports utilisent `LOAD DATA LOCAL` quand le serveur l'autorise (`local_infile` activé), sinon des
  instructions `INSERT`.
- Les erreurs nomment le serveur d'où elles viennent, par exemple `Unknown column 'x' in 'field list' (MySQL
  5.7.44)`.

## Agents

Les agents en lecture seule et les connexions en lecture seule exécutent `SET SESSION TRANSACTION READ ONLY`,
et leurs requêtes s'exécutent dans une transaction en lecture seule. `server_activity` et `top_queries`
fonctionnent. Une `@variable`, une table temporaire ou un `SET` MySQL ou MariaDB créés par un appel d'agent
sont encore là au suivant. Voir [Agents et la CLI](../../../../AGENTS.md).

## Limites connues

- Plus ancien que MySQL 5.7 ou MariaDB 10.6 : il se connecte, avec un avertissement disant que certaines
  fonctionnalités peuvent ne pas marcher.
- L'éditeur de tables enregistre avec le DDL de MySQL, qui valide à chaque instruction : un script qui échoue
  peut en laisser une partie appliquée.
- Le mode terminal demande `mysql` dans le PATH.

## Testé sur

La suite de conformité tourne sur MySQL 5.7, 8.0 et 8.4 et MariaDB 10.6 et 11.4, avec un certificat client sur
TLS mutuel sur MySQL 8.4, et des sessions que le serveur a coupées.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
