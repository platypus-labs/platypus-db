<!-- source: 6fd0f54f48e3 -->
[← Bases de données](README.md)

# Snowflake, BigQuery, Databricks, Trino et autres pilotes ADBC

Ces bases de données sont atteintes par des pilotes **ADBC** (Arrow Database Connectivity), des bibliothèques
que l'application charge à la connexion. Snowflake, BigQuery, Databricks et Trino ont chacun une tuile ; toute
autre base de données dotée d'un pilote ADBC se connecte par **Autre pilote ADBC**.

## Première utilisation : le téléchargement du pilote

Les quatre bases de données nommées ont besoin de leur pilote sur cette machine. Quand vous en choisissez une
pour une nouvelle connexion et que le pilote n'est pas installé, une fenêtre s'ouvre au-dessus du formulaire :
*Snowflake nécessite un pilote* (et ainsi de suite). Elle dit qu'il s'agit d'un pilote ADBC, téléchargé une
fois pour toutes les connexions de cette base de données, et montre le paquet, sa version, sa licence et d'où
il vient : le [registre de pilotes dbc](https://docs.columnar.tech/dbc/) (Columnar). **Télécharger** le
récupère, vérifie son SHA-256 (un paquet qui n'en a pas est refusé) et le garde avec les données de
l'application ; **Annuler** laisse le formulaire utilisable, et la proposition revient à *Tester la connexion*
et à la première ouverture.

La proposition ne lit que ce qui est sur cette machine, donc elle apparaît aussi hors ligne ; le
téléchargement demande le réseau. *Paramètres › Pilotes* liste les pilotes installés et les supprime,
recherche des versions plus récentes une fois par jour (ou avec *Rechercher des mises à jour*), installe ou
revient à une autre version avec *Versions…*, et peut installer seul les mises à jour. Après un changement,
les connexions enregistrées qui utilisent le pilote sont essayées (connexion, version, schémas, `SELECT 1`),
avec *Revenir* si l'une échoue. Le choix d'un autre pilote ne télécharge rien : vous le pointez vers un
fichier de bibliothèque.

## Connexion

Chaque base de données a ses propres champs :

| Base de données | Champs |
|---|---|
| Snowflake | **Compte** (par exemple `xy12345.eu-west-1`), utilisateur, mot de passe, warehouse, rôle, base de données, schéma |
| BigQuery | **Projet**, dataset, et un fichier de clé de compte de service (JSON) ; vide veut dire les Application Default Credentials de Google. Pas de mot de passe |
| Databricks | **Nom d'hôte du serveur**, **Chemin HTTP**, catalogue, schéma ; le champ du mot de passe est le **Jeton d'accès** |
| Trino | **Hôte**, port (8080 par défaut), **utilisateur**, catalogue, schéma, mot de passe (facultatif), et TLS |
| Autre pilote ADBC | le fichier de la **Bibliothèque du pilote**, un symbole de **Point d'entrée** facultatif, une URI, un utilisateur et un mot de passe ; toute autre option que vous ajoutez est transmise au pilote |

**Les choix TLS de Trino** sont *Automatique* (HTTPS sur le port 443, HTTP sinon), *Désactivé* (HTTP),
*Activé — HTTPS, vérifie le certificat* et *Activé — HTTPS, non vérifié*.

**Autre pilote ADBC** : la bibliothèque est le fichier de pilote que vous avez téléchargé ou compilé. Le point
d'entrée n'est nécessaire que pour une bibliothèque qui n'exporte pas `AdbcDriverInit` (pour la bibliothèque
ADBC de DuckDB, c'est `duckdb_adbc_init`). L'URI et les options sont ce que la documentation de ce pilote
indique. Un fichier absent, ou qui n'est pas un pilote, est signalé à la connexion.

## Privilèges

Tout ce que le compte ou le jeton a sur le service. Le navigateur lit les catalogues, schémas, tables et
colonnes avec l'appel de métadonnées d'ADBC.

## Ce qui fonctionne

- Les requêtes, les résultats, le navigateur et la saisie semi-automatique. Le résultat arrive en flux sous
  forme de lots Arrow.
- Annuler : l'application demande au pilote d'annuler ; les pilotes écrits en Go n'annulent que leurs propres
  appels, donc une instruction qui ne s'arrête pas est abandonnée avec sa session, et l'application en ouvre
  une nouvelle.
- Le DDL d'objet sur **Trino** seulement (`SHOW CREATE`). Snowflake, BigQuery, Databricks et les autres
  pilotes n'affichent pas de DDL.
- Les clés étrangères qui pointent vers une table ne sont pas listées (il faudrait lire les clés de chaque
  table).
- Les exports, et les imports de fichiers (les imports typés sont testés sur Trino).

## Ce qui ne fonctionne pas

- **Pas de commit manuel** : chaque instruction valide d'elle-même, et désactiver l'autocommit dit que le
  commit manuel avec un pilote ADBC n'est pas pris en charge.
- **Pas d'explain**, et pas d'onglets d'administration.
- **Pas de modification des données dans la grille** : un enregistrement vérifie chaque modification dans une
  transaction, et ces connexions n'en ont pas. Les résultats restent en lecture seule ; utilisez le SQL pour
  les modifications.
- Le mode terminal n'est pas disponible.

## Agents

`run_query`, `sample_rows` et les autres outils fonctionnent, dans le cadre des vérifications. Sans session en
lecture seule côté serveur, les agents en lecture seule reposent sur la seule vérification de l'instruction.
Les instructions qui agissent en dehors de la base sont soumises à une question avant de s'exécuter : le
`query()` de Trino et les fonctions `SYSTEM$` de Snowflake. Voir [Agents et la CLI](../../AGENTS.md).

## Limites connues

- Une connexion à un entrepôt cloud autre que les champs ci-dessus (paire de clés, authentification unique)
  n'est pas dans le formulaire ; le pilote générique accepte les options propres au pilote.
- Snowflake, BigQuery et Databricks sont dans le code et partagent la couche de pilote avec Trino, mais n'ont
  aucun test sur les vrais services.

## Testé sur

La suite de conformité fait tourner **Trino 484** par son pilote téléchargé (types, longs résultats,
annulation, métadonnées, erreurs, instructions qui se terminent par un terminateur, imports typés, et lignes
et UUID) et le **pilote ADBC générique** sur le pilote ADBC d'Apache pour SQLite et sur celui de DuckDB.
Snowflake, BigQuery et Databricks ne sont pas testés.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
