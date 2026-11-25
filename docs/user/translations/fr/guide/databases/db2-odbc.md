<!-- source: 1206da228de0 -->
[← Bases de données](README.md)

# IBM Db2 et ODBC (Windows)

Db2, et toute base de données dotée d'un pilote ODBC ou d'une source de données (DSN), sont atteints par le
gestionnaire de pilotes ODBC du système. Cela ne fonctionne **que sous Windows** pour l'instant : sous macOS
et Linux, une connexion Db2 ou ODBC dit que les pilotes ODBC ne sont atteints que sous Windows.

## Première utilisation : le pilote ODBC d'IBM (Db2)

Quand vous choisissez **IBM Db2** pour une nouvelle connexion et qu'aucun pilote ODBC d'IBM n'est enregistré
sur cette machine, une fenêtre s'ouvre au-dessus du formulaire : *IBM Db2 nécessite IBM Data Server Driver
for ODBC and CLI*. Elle montre le paquet (version 12.1.4), sa taille, les conditions d'IBM et d'où il vient,
et vérifie le téléchargement par un SHA-256. **Télécharger et installer** le place dans le dossier de données
de l'application et l'enregistre avec `db2cli install -setup` ; **Windows demande la permission
d'administrateur (Contrôle de compte d'utilisateur)** pour cela. **Annuler** laisse le formulaire utilisable,
et la proposition revient à *Tester la connexion* et à la première ouverture.

Un pilote déjà présent dans le système compte : l'application en cherche un nommé `IBM DB2 ODBC DRIVER…` ou
`IBM DATA SERVER…`, que son enregistrement vienne de l'application ou d'un client IBM complet, et prend le
plus récent par son nom. L'option `driver` de la connexion remplace ce choix. *Paramètres › Pilotes* liste et
supprime le pilote téléchargé, et montre les pilotes ODBC et les sources de données du système.

## Connexion

**IBM Db2** : hôte, port (50000 par défaut), **Base de données** (l'hôte et la base de données sont tous deux
obligatoires), utilisateur et mot de passe. L'application construit elle-même la chaîne de connexion
(`DATABASE`, `HOSTNAME`, `PORT`, `PROTOCOL=TCPIP`, `UID`, `PWD`). TLS : les modes `disable` et `prefer` se
connectent sans lui ; `require` et au-delà ajoutent `Security=SSL`, et `sslrootcert` devient
`SSLServerCertificate`. Les serveurs Db2 écoutent TLS sur un autre port que celui en clair, donc indiquez le
port. Par un tunnel SSH, c'est l'extrémité locale du tunnel qui est utilisée.

**Plateforme** choisit le type de serveur Db2 :

- *Db2 for Linux, UNIX and Windows (LUW)*, par défaut, aussi pour les vieux serveurs 8.x.
- *Db2 for z/OS* : le même pilote IBM, port 446 par défaut, et la **Base de données** est le nom de
  l'emplacement. Le pilote d'IBM n'atteint z/OS qu'avec une licence Db2 Connect sur cette machine.
- *Db2 for IBM i* (AS/400) : le propre *IBM i Access ODBC Driver* d'IBM (IBM i Access Client Solutions,
  installé depuis IBM, pas par l'application ; l'ancien pilote iSeries Access fonctionne aussi). L'hôte est
  le nom du système, la base de données est facultative et définit la bibliothèque par défaut, il n'y a pas
  de port, et TLS ajoute `SSL=1`. Il ne passe pas par un tunnel SSH, car le pilote ouvre plusieurs ports du
  serveur.

**Autre pilote ODBC** (le choix générique) : donnez l'un de ces éléments

- une **Source de données (DSN)** que le système connaît, saisie ou prise dans les suggestions ;
- un nom de **Pilote** (suggéré à partir des pilotes ODBC du système) avec une **Chaîne de connexion**
  contenant le reste, comme `Server=host;Database=sales` ;
- une **Chaîne de connexion** seule, utilisée telle quelle (elle peut nommer son propre `DRIVER=`).

Les champs utilisateur et mot de passe sont ajoutés comme `UID` et `PWD`, sauf si la chaîne les contient déjà.
Une chaîne ADO.NET ou ODBC collée avec `Driver={…}` est lue dans le formulaire correspondant quand le pilote
est SQL Server, PostgreSQL, MySQL/MariaDB ou ClickHouse ; tout autre pilote ou un `DSN=` devient une connexion
ODBC générique avec la chaîne telle quelle.

## Privilèges

Tout ce que le login de la base de données a. Le navigateur utilise les appels de catalogue d'ODBC
(`SQLTables`, `SQLColumns`, `SQLPrimaryKeys`, `SQLForeignKeys`). Il n'y a pas d'onglets d'administration.

## Ce qui fonctionne

- Les requêtes et les résultats, avec les types que le pilote rapporte.
- Le commit et le rollback manuels.
- Annuler : l'appel d'annulation d'ODBC n'est pas accessible, donc une instruction qui n'a pas encore démarré
  est abandonnée avec sa session (l'application en ouvre une nouvelle) ; une instruction qui renvoie des
  lignes s'arrête quand son flux est abandonné.
- Le navigateur, à partir des appels de catalogue. Le SQL pour Db2 est écrit avec les guillemets de Db2 (les
  noms nus passent en majuscules, les lignes sont plafonnées avec `FETCH FIRST`) ; le pilote générique écrit
  du SQL standard.

## Ce qui ne fonctionne pas

- Pas d'explain, pas de DDL d'objet, pas d'onglets d'administration, pas de chargement en masse (les imports
  utilisent `INSERT`).
- **Les valeurs de texte et binaires de plus de 64 Kio sont une erreur**, plutôt qu'un énorme tampon.
- **La modification des données dans la grille** n'est pas testée sur ces bases de données. La grille ne la
  bloque pas, mais sans savepoints, un enregistrement qui échoue en mode de commit manuel annule toute la
  transaction de la session et le dit.
- Le mode terminal n'est pas disponible.

## Agents

Les agents en lecture seule reposent sur la seule vérification de l'instruction. Voir
[Agents et la CLI](../../AGENTS.md).

## Limites connues

- Windows seulement.
- Db2 n'a pas de conteneur dans la configuration de test, donc la chaîne de connexion Db2 et la détection du
  pilote ne sont vérifiées que par des tests unitaires.

## Testé sur

Le pilote ODBC générique tourne sur SQL Server 2022 par le pilote `SQL Server` propre à Windows : types,
longs résultats, erreurs avec leur SQLSTATE, métadonnées, commit et rollback, annulation d'une instruction en
attente, littéraux et types propres à SQL Server, et imports. IBM Db2 n'est pas testé sur un serveur.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
