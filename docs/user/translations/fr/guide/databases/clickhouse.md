<!-- source: b5b424fa73d7 -->
[← Bases de données](README.md)

# ClickHouse

Le pilote parle à l'**interface HTTP** de ClickHouse et lit les résultats au format de flux Arrow. Il fait
partie de l'application ; rien n'est téléchargé. Il parcourt, interroge et conçoit des tables, mais ne
modifie pas les lignes dans la grille.

## Connexion

| Vous collez | Interprété comme |
|---|---|
| `clickhouse://default@ch.example.com/logs` | HTTP sur le port 8123, base de données `logs` |
| `clickhouse://ch.example.com/logs?sslmode=require` | HTTPS ; sans port dans l'URL, 8443 |
| `jdbc:clickhouse://host:8123/db` | une URL JDBC |
| `https://host:8443` dans un formulaire ClickHouse | l'URL HTTP(S) du serveur |

Utilisez le port **HTTP**, pas le port natif (9000). L'utilisateur est `default` quand il est vide ; le mot
de passe est envoyé dans l'en-tête `X-ClickHouse-Key`. Le champ Base de données exécute `USE <database>`
après la connexion.

**TLS** est choisi par le mode, puisque HTTP ne peut pas le négocier : `disable` et `prefer` (par défaut) se
connectent en **HTTP simple** ; `require` et les modes `verify` utilisent HTTPS. Avec HTTPS, le port par
défaut est 8443 quand l'URL n'en donne pas ; si vous remplissez le formulaire, saisissez vous-même le port.
`require` ne vérifie pas le certificat, `verify-ca` vérifie la chaîne, `verify-full` aussi le nom d'hôte ;
`sslrootcert` désigne le fichier de votre CA, sinon les certificats de confiance du système sont utilisés.

Une connexion est une session HTTP : `SET`, `USE` et les tables temporaires durent d'une instruction à
l'autre.

## Privilèges

Parcourir lit `system.databases`, `system.tables`, `system.columns` et `system.data_skipping_indices` ; le
login voit ce que ClickHouse lui laisse voir à cet endroit. Les onglets lisent :

- **Paramètres** : `system.settings` et `system.server_settings`.
- **Tableau de bord** : `system.metrics`, `system.events`, `system.parts` (tailles des bases de données) et
  `system.replicas` ; ce que le login ne peut pas lire affiche « — ».
- **Utilisateurs et rôles** : `system.users`, `system.roles`, `system.grants` et `system.role_grants`. Les
  utilisateurs définis dans les fichiers de configuration du serveur sont listés aussi ; le serveur refuse de
  les modifier.

## Ce qui fonctionne

- Les requêtes, les résultats, le navigateur, la saisie semi-automatique, le DDL des objets, la recherche de
  métadonnées, et le concepteur de tables (y compris la clause du moteur et les index de saut).
- Explain montre le plan de `EXPLAIN indexes = 1` ; il n'y a pas d'Explain analyze.
- Annuler arrête l'instruction sur le serveur avec `KILL QUERY`.
- Les onglets Paramètres, Tableau de bord et Utilisateurs et rôles. Pas d'onglets sessions, verrous,
  instructions ou tables et index.
- Les exports, et les imports vers ClickHouse sous forme d'instructions `INSERT`.

## Ce qui ne fonctionne pas

- **Pas de modification des données dans la grille** : elle dit « les tables ClickHouse ne se modifient pas
  directement ». L'enregistrement vérifierait le nombre de lignes de chaque modification dans une transaction,
  et ClickHouse n'a ni transactions ni nombres de lignes pour les mises à jour. Modifiez les lignes en SQL.
- **Pas de transactions** : la barre d'outils du commit manuel est masquée ; chaque instruction prend effet
  seule.
- Pas de clés étrangères ni de contraintes d'unicité (la clé primaire ne fait qu'ordonner les données).

## Agents

Un agent en lecture seule, et une connexion en lecture seule, envoient chaque instruction, sauf `SET`, avec
`readonly = 1`, si bien que le serveur refuse les écritures et les fonctions de table qui atteignent d'autres
serveurs ou ses fichiers (`url`, `s3`, `remote`, `file` et les semblables). L'instruction d'un agent est aussi
comparée à une liste de fonctions de table qui ne lisent que des données locales. Voir
[Agents et la CLI](../../AGENTS.md). Le mode terminal n'est pas disponible : `clickhouse-client` utilise le
port natif.

## Limites connues

- HTTP simple sauf si le mode est `require` ou plus strict : le `prefer` par défaut ne chiffre pas.
- Seule l'interface HTTP est utilisée ; un serveur qui n'expose que le port natif ne peut pas être atteint.

## Testé sur

La suite de conformité tourne sur ClickHouse 26.3. Les versions plus anciennes de ClickHouse ne sont pas
testées, et l'application n'a pas de version minimale pour lui.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
