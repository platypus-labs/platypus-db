<!-- source: 7b39ba524d58 -->
[← Bases de données](README.md)

# libSQL et Turso

Le pilote parle le protocole **Hrana 3** sur HTTP(S) à un serveur libSQL (`sqld`) ou à Turso. Le SQL est
celui de SQLite, donc le navigateur, les types et les plans sont aussi ceux de SQLite. Le pilote fait partie
de l'application ; rien n'est téléchargé.

## Connexion

| Vous collez | Interprété comme |
|---|---|
| `libsql://db-acme.turso.io?authToken=…` | HTTPS vers cet hôte, le jeton comme mot de passe |
| `libsql://:token@localhost:8080?sslmode=disable` | HTTP simple, le jeton à la place du mot de passe |
| `libsql://localhost:8080?tls=0&authToken=t` | `tls=0` est du HTTP simple, `tls=1` du HTTPS |
| `wss://…`, `ws://…` | le même serveur en HTTP(S) : `wss` est TLS, `ws` ne l'est pas |
| `https://host` ou `http://host` dans un formulaire libSQL | l'adresse HTTP du serveur |

Sans port, la valeur par défaut du schéma est utilisée : 443, ou 80 quand TLS est désactivé.

- **Token** : le champ du mot de passe, ou `authToken` dans l'URL.
- **TLS** : une URL `libsql://` est en HTTPS avec le certificat et le nom d'hôte vérifiés (`verify-full`),
  parce que l'URL porte le jeton. La tuile Turso du formulaire démarre aussi à `verify-full`. Pour un `sqld`
  local sans TLS, choisissez `disable`, ou utilisez `tls=0` dans l'URL. `sslrootcert` désigne le fichier de
  votre CA ; sinon les certificats de confiance du système sont utilisés.

## Privilèges

Tout ce que le jeton autorise : l'application envoie les instructions que vous exécutez, et les requêtes de
catalogue SQLite pour le navigateur.

## Ce qui fonctionne

- La modification des données dans la grille, par clé primaire (ou clé unique sur des colonnes non nulles, ou
  clé virtuelle).
- Le commit et le rollback manuels. Une instruction s'exécute sur un flux que le serveur garde ouvert entre
  les requêtes (un `baton`), si bien qu'une transaction et les tables temporaires durent. Si le serveur coupe
  le flux en dehors d'une transaction, l'application en ouvre un autre et continue ; dans une transaction, la
  connexion se déclare perdue, et la transaction avec elle.
- Explain montre le plan de requête de SQLite ; il n'y a pas d'Explain analyze.
- La recherche de métadonnées et le DDL des objets, et les outils de schéma comme pour SQLite.
- Pas d'onglets sessions, verrous, tableau de bord, utilisateurs ou paramètres.

## Ce qui ne fonctionne pas

- **Annuler** : HTTP n'a aucun moyen d'arrêter une instruction en cours, donc Annuler répond que l'annulation
  des instructions libSQL n'est pas prise en charge.
- Les résultats arrivent entiers du serveur, pas ligne par ligne.
- Le mode terminal n'est pas disponible : les serveurs libSQL n'ont pas de client en ligne de commande pour
  exécuter des scripts.

## Agents

Les agents en lecture seule et les connexions en lecture seule reposent sur la seule vérification de
l'instruction ; le serveur n'est pas invité à refuser les écritures. Voir
[Agents et la CLI](../../AGENTS.md).

## Limites connues

- Une panne réseau après l'envoi d'une instruction peut laisser inconnu si elle s'est exécutée ; l'application
  ne la renvoie pas.

## Testé sur

La suite de conformité tourne sur `sqld` v0.24.33 (conteneur `ghcr.io/tursodatabase/libsql-server`), sans TLS.
Le cloud de Turso n'est pas testé.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
