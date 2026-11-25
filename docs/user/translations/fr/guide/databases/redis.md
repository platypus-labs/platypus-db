<!-- source: 8b43e38d9f7d -->
[← Bases de données](README.md)

# Redis

Le pilote est construit sur le crate `redis` (RESP3 quand le serveur le parle, avec rustls pour TLS). Il ouvre
Redis 6 et les versions suivantes, et Redis Stack pour les clés JSON. L'éditeur prend des **commandes
Redis**, une par ligne. Le pilote fait partie de l'application ; rien n'est téléchargé.

## Connexion

| Vous collez | Interprété comme |
|---|---|
| `redis://:s%40cret@cache:6380/3` | TCP simple, mot de passe seul, base de données 3 |
| `redis://app:pw@cache/0` | un utilisateur ACL et son mot de passe |
| `rediss://app:pw@cache` | TLS (vérifié : `verify-full`) |

Le port par défaut est 6379. Le chemin (ou le champ Base de données) est le **numéro de base de données**,
`0` quand il est vide ; toute autre chose est une erreur. L'utilisateur et le mot de passe sont ceux de l'ACL :
un mot de passe seul connecte comme l'utilisateur par défaut.

**TLS** est activé pour les URL `rediss://` et pour les modes SSL à partir de `require` (`redis://` veut dire
pas de TLS et `prefer` ne l'active pas, parce que Redis prend TLS sur un port à part). `require` ne vérifie
pas le certificat ; `verify-ca` et `verify-full` le font. Un fichier CA et un certificat client vont dans les
champs TLS. La session ouvre une connexion par base de données utilisée, parce qu'un `SELECT` sur une
connexion partagée déplacerait tous les autres qui l'utilisent.

## Privilèges

Les règles ACL du login décident de ce que le navigateur et les commandes peuvent faire. Le navigateur lit
`INFO` et, pour chaque type de clé, exécute `SCAN … TYPE` ; parcourir un type exécute `BROWSE`, jamais `KEYS`.
Il n'y a pas d'onglets d'administration.

## Ce qui fonctionne

- Les commandes dans l'éditeur, entre guillemets comme `redis-cli` les met ; les lignes `#` et `--` sont des
  commentaires. Les réponses sont des lignes, les réponses imbriquées étant du JSON dans l'arbre du document
  (`F7`).
- `SELECT n` déplace la session vers une autre base de données. `MULTI` met les commandes en file jusqu'à
  `EXEC`, qui les envoie comme une seule transaction et montre chaque réponse.
- Le navigateur liste les bases de données qui contiennent des clés, chacune avec une table par type de clé :
  `string`, `hash`, `list`, `set`, `zset`, `stream`, et `json` avec RedisJSON. En ouvrir une exécute
  `BROWSE db0.hash`, éventuellement avec `MATCH` et `LIMIT`, en lisant les grosses clés par morceaux.
- **La grille modifie les lignes** : valeurs, TTL, noms de clés, champs de hash, membres et scores ; les
  nouvelles lignes ajoutent des clés et des éléments, les lignes supprimées les retirent. L'aperçu montre les
  commandes, qui s'exécutent en un seul `MULTI`/`EXEC` ; une commande que le serveur refuse n'annule pas les
  autres. Un *Script d'annulation* remet les valeurs en place. Une clé, un champ ou un membre qui existe déjà
  arrête l'enregistrement avant que rien ne s'exécute.
- `FLUSHDB` et `FLUSHALL` demandent d'abord, comme un `UPDATE` sans `WHERE` ; sur les connexions de production,
  les commandes qui modifient le serveur (`CONFIG SET`, `SHUTDOWN`) demandent comme le fait le DDL. Les
  connexions en lecture seule n'exécutent que les commandes qui lisent.
- Annuler arrête une commande bloquante (`BLPOP`, `XREAD BLOCK`).
- Le mode terminal exécute `redis-cli` s'il est dans le PATH, avec le mot de passe dans son environnement.
- Toutes les commandes et tous les types de clés sont décrits dans [FEATURES.md](../../../../FEATURES.md#redis).

## Ce qui ne fonctionne pas

- Les abonnements (`SUBSCRIBE` et les semblables) et `MONITOR` sont refusés : ils prendraient la main sur la
  connexion.
- Pas de commit manuel : `MULTI`/`EXEC` est la seule transaction.
- Pas d'explain, pas de DDL d'objet, pas d'outils SQL (éditeur de tables, diagramme ER, comparaison de
  schémas, constructeur de requêtes, import SQL), pas d'instantanés avant modifications.

## Agents

`run_query` et `sample_rows` (`BROWSE … LIMIT n`) fonctionnent ; toute commande qui n'est pas une lecture fait
d'abord l'objet d'une question. Les commandes qui montrent les identifiants du serveur (`ACL GETUSER`,
`ACL LIST`, et un `CONFIG GET` dont le motif correspond à un réglage de mot de passe) sont refusées aux agents
en lecture seule. Refusés sur Redis : `explain`, `get_er_diagram`, `propose_edits` et
`query_across_databases`. Voir [Agents et la CLI](../../../../AGENTS.md).

## Limites connues

- Plus ancien que 6.0 : il se connecte avec un avertissement disant que certaines fonctionnalités peuvent ne
  pas marcher.
- Le nombre de clés d'une table ne s'affiche que quand la base de données a au plus 10 000 clés.
- Les valeurs qui ne sont pas en UTF-8 s'affichent entre guillemets, avec des échappements.

## Testé sur

La suite de conformité tourne sur Redis Stack 7.4 (avec RedisJSON) : métadonnées, `BROWSE` sur chaque type,
commandes et leurs erreurs, enregistrement et annulation des modifications de la grille, annulation des
commandes bloquantes, utilisateurs ACL et sélection de la base de données, et une connexion perdue qui se
rouvre. Redis 6 est la plus ancienne version que le pilote accepte mais la suite ne la fait pas tourner.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
