<!-- source: eeb36912ad18 -->
[← Bases de données](README.md)

# SQLite

SQLite tourne à l'intérieur de l'application (`rusqlite` avec SQLite intégré) : pas de serveur, pas d'hôte,
pas d'utilisateur, pas de mot de passe, rien à télécharger.

## Connexion

Choisissez **SQLite** et indiquez le chemin du fichier, ou collez une URL :

| Vous collez | Signification |
|---|---|
| `sqlite:///home/ana/app.db` | le fichier `/home/ana/app.db` |
| `sqlite:///C:/data/app.db` | un fichier sur un lecteur Windows |
| `sqlite:///:memory:` | une base de données en mémoire |
| `file:app.db?mode=ro` dans le champ du chemin | une URI SQLite, transmise telle quelle à SQLite |

Le fichier est créé, comme base de données vide, quand il n'existe pas mais que son dossier existe. Un dossier
absent est une erreur (plutôt une faute de frappe ou un lecteur non monté), de même qu'un chemin relatif qui
n'existe pas : donnez le chemin complet. Un fichier qui n'est pas une base de données SQLite est refusé à la
connexion.

Une connexion en lecture seule (et un agent en lecture seule) ouvre le fichier en lecture seule, si bien que
SQLite lui-même refuse une écriture. Les réglages SSL, SSH et proxy ne s'appliquent pas à une base de
données embarquée.

## Privilèges

Ceux du système d'exploitation : l'application doit pouvoir lire le fichier, et l'écrire ainsi que son dossier
(pour le journal) pour modifier quoi que ce soit. Un fichier occupé est attendu avant d'afficher une erreur.

## Ce qui fonctionne

- La modification des données dans la grille, par clé primaire (ou clé unique sur des colonnes non nulles, ou
  clé virtuelle). Une colonne `INTEGER PRIMARY KEY` compte comme une clé bien que SQLite la déclare nullable.
- Le commit et le rollback manuels ; l'annulation d'une instruction en cours (`sqlite3_interrupt`).
- Explain ne montre que le plan de requête de SQLite : il n'y a pas d'Explain analyze.
- Onglet Paramètres : les pragmas qui comptent, avec les valeurs par défaut documentées de SQLite.
- Pas d'onglets sessions, verrous, tableau de bord, utilisateurs ou instructions.
- L'éditeur de tables, le diagramme ER et la comparaison. SQLite modifie peu de choses sur place, donc
  changer une table la recrée et copie ses lignes ; la comparaison et la reconstruction conservent les
  index d'expression et partiels, les clés différées et le compteur `AUTOINCREMENT`.
- La recherche de métadonnées, le DDL des objets, et les exports, imports et copies comme pour toute base de
  données SQL.
- Le mode terminal exécute `sqlite3`, s'il est dans le PATH.

## Agents

Les agents en lecture seule et les connexions en lecture seule ouvrent le fichier en lecture seule, et
exécutent leurs requêtes avec `query_only`. `estimate_change` n'a pas d'estimation de lignes à donner sur
SQLite. Voir [Agents et la CLI](../../../../AGENTS.md).

## Limites connues

- Un autre processus peut tenir le fichier : l'application attend jusqu'à 5 secondes pour un fichier occupé,
  puis SQLite signale l'erreur.
- L'instruction d'un agent qui appelle `load_extension(…)` fait d'abord l'objet d'une question, comme une
  modification.

## Testé sur

La suite de conformité tourne sur le SQLite intégré à l'application, y compris les transactions, les plans,
la modification de tables, la reconstruction qui conserve `AUTOINCREMENT`, et les fichiers ouverts en lecture
seule.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
