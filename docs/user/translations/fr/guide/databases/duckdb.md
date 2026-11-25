<!-- source: 728e6511c5f7 -->
[← Bases de données](README.md)

# DuckDB

DuckDB tourne à l'intérieur de l'application, comme SQLite : pas de serveur, d'hôte, d'utilisateur ni de mot
de passe. Contrairement à SQLite, l'application ne contient pas la bibliothèque de DuckDB ; elle **la
télécharge la première fois** qu'une base de données DuckDB ou un fichier de données est ouvert.

## Première utilisation : la bibliothèque de DuckDB

Quand vous choisissez **DuckDB** pour une nouvelle connexion et que la bibliothèque n'est pas sur cette
machine, une fenêtre s'ouvre au-dessus du formulaire : *Télécharger DuckDB ?* Elle montre le paquet (DuckDB
2.0.0, de la version de DuckDB sur GitHub, licence MIT), sa taille et sa source, et vérifie le téléchargement
par un SHA-256. **Télécharger** la place dans le dossier de données de l'application, une fois pour toutes les
bases de données et tous les fichiers DuckDB ; **Annuler** laisse le formulaire utilisable, et la proposition
revient à *Tester la connexion* et à la première ouverture. *Paramètres › Pilotes* liste la bibliothèque et
la supprime.

L'application la télécharge pour Windows (x64, ARM64), Linux (x64, ARM64) et macOS (une seule bibliothèque
pour les deux processeurs). L'application utilise exactement la version avec laquelle elle a été compilée
(2.0.x) et propose celle-là si une autre est installée. Hors ligne, avec la bibliothèque absente, la connexion
ne peut pas s'ouvrir.

## Connexion

Choisissez **DuckDB** et indiquez le chemin du fichier de base de données, ou collez une URL :

| Vous collez | Signification |
|---|---|
| `duckdb:///home/ana/analytics.duckdb` | ce fichier, créé s'il manque (son dossier doit exister) |
| `duckdb:///C:/data/analytics.duckdb` | un fichier sur un lecteur Windows |
| `duckdb:///:memory:` | la base de données en mémoire que partagent les sessions de l'application |
| `:memory:<name>` comme chemin | une base de données en mémoire à part, désignée par son nom |

**Des fichiers de données comme bases de données.** Un chemin qui se termine par `.csv`, `.tsv`, `.json`,
`.ndjson`, `.jsonl`, `.parquet`, `.xlsx`, `.xlsm`, `.xls` ou `.ods` s'ouvre comme une base de données en
mémoire sur ce fichier (vous pouvez aussi déposer un fichier sur la fenêtre). CSV, TSV, JSON, NDJSON et
Parquet deviennent une vue nommée d'après le fichier, que DuckDB relit à chaque requête ; chaque feuille d'un
classeur devient une table, chargée une fois. Un CSV ou TSV est d'abord vérifié (chaque ligne doit avoir
autant de valeurs que l'en-tête), son encodage de texte est détecté (ou vous le donnez), et les options
d'import permettent d'ignorer les mauvaises lignes.

Les extensions ne sont ni téléchargées ni chargées toutes seules ; seules celles intégrées à la bibliothèque
(Parquet, JSON) sont là.

## Privilèges

Ceux du système d'exploitation : accès en lecture au fichier, accès en écriture à lui et à son dossier pour
modifier quoi que ce soit. Un seul processus à la fois peut ouvrir un fichier DuckDB en écriture : tant que
l'application a un fichier ouvert, un autre programme (la ligne de commande `duckdb`, par exemple) ne peut pas
l'utiliser ; les sessions de l'application sur un même fichier partagent une seule instance. Le mode terminal
n'est donc pas proposé.

## Ce qui fonctionne

- La modification des données dans la grille, par clé primaire (ou clé unique sur des colonnes non nulles, ou
  clé virtuelle). Un fichier de données est lu en mémoire : une vue CSV, JSON ou Parquet n'est pas une table
  et ne peut pas être modifiée, et les modifications des feuilles d'un classeur ne sont pas réécrites dans le
  fichier.
- Le commit et le rollback manuels ; l'annulation d'une instruction (`duckdb_interrupt`).
- Explain montre le plan (`EXPLAIN (FORMAT JSON)`) ; **Explain analyze** exécute l'instruction dans une
  transaction qu'il annule, et est refusé tant que vous avez une transaction ouverte.
- Onglet Paramètres : les réglages de DuckDB. Pas d'onglets sessions, verrous, tableau de bord, utilisateurs
  ou instructions.
- La recherche de métadonnées, le DDL des objets, les outils de schéma. DuckDB modifie peu de choses sur
  place, donc changer une table la recrée et copie ses lignes.
- Les imports passent par l'appender de DuckDB, dans la transaction de la session.

## Agents

Un agent en lecture seule, et une connexion en lecture seule, ouvrent le fichier de base de données en
lecture seule, quand aucune autre session de l'application ne le tient déjà ouvert ; une base de données en
mémoire ou un fichier de données n'est jamais en lecture seule, et la vérification de l'instruction y arrête
les écritures. Les sessions DuckDB des agents ne peuvent ni lire ni écrire d'autres fichiers ni charger
d'extensions, et ferment le fichier après chaque appel pour que l'application puisse l'ouvrir.
`query_across_databases` s'exécute dans un DuckDB à part que l'application télécharge comme celui-ci. Voir
[Agents et la CLI](../../AGENTS.md).

## Limites connues

- Les fichiers Excel sont lus avec le lecteur propre de l'application, pas avec l'extension Excel de DuckDB.
- Une version de DuckDB autre que 2.0.x n'est pas chargée.

## Testé sur

La suite de conformité tourne sur un fichier DuckDB, avec la bibliothèque qui correspond aux liaisons de
l'application : transactions, plans, modification de tables, valeurs spéciales, nombres 128 bits et non
signés, et heures avec fuseau.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
