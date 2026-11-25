<!-- source: 2a42aace96dc -->
[← Bases de données](README.md)

# Elasticsearch et OpenSearch

Un seul pilote parle aux deux par leur API HTTP (`reqwest` avec rustls). Le serveur dit lequel il est à la
connexion, et la connexion l'affiche (`Connected: OpenSearch 2.17.1`). Le pilote fait partie de l'application ;
rien n'est téléchargé.

## Connexion

| Vous collez | Interprété comme |
|---|---|
| `elasticsearch://user:pass@host:9200/logs-*` | HTTP simple ; le chemin est un motif d'index |
| `elasticsearchs://user:pass@host:9200` | HTTPS (vérifié : `verify-full`) |
| `opensearch://…` et `opensearchs://…` | la même chose, mémorisée comme OpenSearch pour que l'URL soit réécrite ainsi |

Le port par défaut est 9200. Le champ **Base de données** est un motif d'index facultatif (`logs-*`, ou
plusieurs séparés par des virgules) qui restreint ce que liste le navigateur ; vide, il liste tous les index
qui ne sont pas cachés.

- **Connexion** : un utilisateur et un mot de passe utilisent l'authentification Basic. Avec l'utilisateur
  vide, le champ du mot de passe est une **API key** (la valeur `encoded`), envoyée en
  `Authorization: ApiKey …`.
- **TLS** : HTTPS pour les URL `elasticsearchs://` et `opensearchs://` et pour les modes SSL à partir de
  `require` ; `elasticsearch://` et `prefer` sont du HTTP simple. `require` ne vérifie pas le certificat ;
  `verify-ca` et `verify-full` le font. Un fichier CA et un certificat client vont dans les champs TLS.

## Privilèges

Ceux dont les requêtes ont besoin. Le navigateur lit les index, alias, data streams et mappings que le login
peut voir ; il n'y a pas d'onglets d'administration.

## Ce qui fonctionne

- **Des requêtes écrites comme la console Dev Tools de Kibana** : une ligne comme `GET /orders/_search` (la
  barre oblique est facultative ; méthodes `GET`, `POST`, `PUT`, `DELETE`, `HEAD`), puis son corps JSON.
  `_bulk` et `_msearch` prennent des corps NDJSON qui se terminent à une ligne vide ou à la requête suivante.
- **SQL** : tout le reste est du SQL (`_sql` sur Elasticsearch, `_plugins/_sql` sur OpenSearch), avec des
  colonnes typées et le curseur du serveur pour les longs résultats. Les erreurs gardent la raison du serveur
  et une erreur SQL désigne son emplacement dans l'instruction.
- Une recherche donne des documents comme les résultats de MongoDB : une colonne pour chaque champ de premier
  niveau, avec le document sélectionné sous forme d'arbre (`F7`). Les agrégations arrivent comme des
  résultats à part.
- Le navigateur liste les index comme des tables (santé, shards et réplicas dans l'infobulle) et les alias et
  data streams comme des vues. Un index s'ouvre comme une recherche de tous ses documents, lus 1 000 lignes à
  la fois avec un point in time et `search_after` (un scroll sur OpenSearch), si bien que la lecture ne
  s'arrête pas à 10 000 résultats. Un champ sans colonne mappée va dans une dernière colonne `_other` en JSON.
- **La grille modifie les documents** : une cellule modifiée est un `_update` par `_id`, une nouvelle ligne est
  un `_create` (un `_id` qui existe est refusé par le serveur) ou un document généré, une ligne supprimée est
  un `DELETE`. L'aperçu montre les requêtes, qui s'exécutent l'une après l'autre ; si le serveur en refuse une,
  les suivantes s'arrêtent et les précédentes restent. *Script d'annulation* remet l'ancien `_source` (un
  document avec un `_id` généré doit être supprimé à la main).
- `_delete_by_query` et `_update_by_query` sans requête (ou `match_all`), et `DELETE /_all`, demandent d'abord
  comme un `DELETE` sans `WHERE`. Les connexions en lecture seule n'exécutent que les requêtes et le SQL qui
  lisent.
- Annuler arrête une recherche sur le serveur. Le DDL d'objet d'un index est la requête qui le recrée.
- Les détails sont dans [FEATURES.md](../../FEATURES.md#elasticsearch-and-opensearch).

## Ce qui ne fonctionne pas

- Pas de transactions (pas de commit manuel) et pas d'explain.
- Pas d'outils SQL : pas d'éditeur de tables, de diagramme ER, de comparaison de schémas, de constructeur de
  requêtes, d'import SQL, d'instantanés avant modifications, ni de mode terminal.
- La visionneuse de données n'a pas de barre de filtre et de tri.

## Agents

`run_query` et `sample_rows` fonctionnent avec les requêtes de la console et le SQL, et une requête qui modifie
des données fait d'abord l'objet d'une question. Refusés sur Elasticsearch et OpenSearch : `explain`,
`get_er_diagram`, `propose_edits` et `query_across_databases`. Voir [Agents et la CLI](../../AGENTS.md).

## Limites connues

- L'application refuse Elasticsearch antérieur à 7.10 et OpenSearch antérieur à 1.0 : contrairement aux autres
  bases de données, la connexion échoue avec « older than … the oldest version supported ».
- Testé sur Elasticsearch 8 et OpenSearch 2, pas sur 7.x.

## Testé sur

La suite de conformité tourne sur Elasticsearch 8.15.3 (login `elastic`) et OpenSearch 2.17.1 : informations
du serveur, SQL avec types et pagination, recherches qui dépassent une page, le catalogue et les mappings,
enregistrement et annulation des modifications de la grille, annulation d'une recherche lente, et erreurs du
serveur.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
