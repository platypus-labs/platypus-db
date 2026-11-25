<!-- source: 925ae39e7303 -->
[← Guide utilisateur](README.md)

# Import et export

Les actions ci-dessous sont dans le menu contextuel du navigateur et dans la palette de commandes
(`Ctrl+K`). La liste complète des formats et des options est dans
[FEATURES.md](../FEATURES.md#import-export-and-transfer).

## Importer un fichier dans une table

![L'aperçu d'un fichier à importer](../../screenshots/guide/import-preview.png)

1. Faites un clic droit sur la table dans le navigateur, puis **Données** › **Importer des données…**.
2. Choisissez le fichier : CSV, TSV, JSON, NDJSON, Parquet ou Excel.
3. Vérifiez l'aperçu et la correspondance des colonnes.
4. Choisissez **Ajouter les lignes**, **Remplacer toutes les lignes** ou **Mettre à jour les lignes avec la
   même clé, ajouter les autres**, puis cliquez sur **Importer**. Le dernier choix demande une clé primaire
   sur la table, et n'est pas proposé sur ClickHouse.

**Importer des données…** est proposé sur les tables, pas sur les vues. Pour une nouvelle table, faites un
clic droit sur le schéma et choisissez **Importer des données dans une nouvelle table…** : les types sont
ceux que DuckDB déduit du fichier, et il n'y a pas de choix de mode. **Coller un CSV comme nouvelle
table…** (même menu) importe le contenu du presse-papiers.

L'import n'est pas proposé sur une connexion en lecture seule, sur une connexion qui interdit la
modification des données, ni sur les bases de données NoSQL.

## Exporter des tables vers un fichier

1. Faites un clic droit sur une table, ou sélectionnez-en plusieurs, puis **Données** › **Exporter les
   données…**. Pour toutes les tables d'un schéma, faites un clic droit sur le schéma et choisissez
   **Exporter toutes les tables…**.
2. Choisissez le **Formater** (CSV, JSON, NDJSON, Parquet, Excel, SQL `INSERT`, Markdown, HTML ou XML)
   et cliquez sur **Exporter vers un dossier…** : même pour une seule table, l'application demande un
   dossier, et écrit un fichier par table, nommé d'après elle.
3. Réglez si vous le souhaitez la **Compression** (gzip ou zstd ; pas pour Parquet et Excel, qui sont déjà
   compressés) et **Envoyer aussi vers…** un bucket (S3, GCS, R2 ou MinIO).

Quand vous exportez à la place le résultat d'une requête (l'export de la grille de résultats), la boîte de
dialogue demande un seul fichier, et l'envoi peut aussi aller vers une URL signée (S3 ou GCS).

Les cellules CSV qu'un tableur exécuterait comme une formule reçoivent un `'` au début, sauf si vous
désactivez **Formules en texte, pour les tableurs**. Une connexion peut interdire les exports (page
**Apparence et sécurité**) ; l'action n'est alors pas proposée. Voir [Sécurité](safety.md).

Pour sortir le résultat d'une requête, copiez-le depuis la grille en CSV, JSON, Markdown, HTML, SQL
`INSERT` ou liste `IN`.

## Copier une table ou un schéma vers une autre base de données

Faites un clic droit sur une table et choisissez **Données** › **Copier vers une base de données…** (elle
permet de faire correspondre les colonnes), sélectionnez plusieurs tables d'un schéma et choisissez
**Données** › **Copier les tables vers une base de données…**, ou faites un clic droit sur le schéma et
choisissez **Copier le schéma vers une base de données…**. Les types sont convertis. La cible est une
connexion SQL qui n'est pas en lecture seule et qui autorise la modification des données. Copier plusieurs
tables ou un schéma est réservé aux sources SQL ; une collection MongoDB ou un type de clé Redis peut être
copié vers une base de données SQL. Aucune de ces actions n'est proposée quand la connexion interdit les
exports.

## Exécuter un fichier SQL, ou exporter vers un fichier

Faites un clic droit sur une connexion ou une base de données et choisissez **Exécuter un fichier SQL…** :
cela exécute un fichier `.sql` (ou `.sql.gz`, `.sql.zst`) de toute taille instruction par instruction,
avec progression et Arrêter, par défaut dans une transaction qu'un échec annule. **Exporter dans un fichier
SQL…** (sur un schéma, ou sur des tables sous **Données**) écrit dans un seul fichier le DDL d'un schéma ou
des objets choisis, et si on le veut leurs lignes.

## Enregistrer un transfert pour plus tard

Chaque boîte de dialogue d'export, d'import et de copie a **Enregistrer comme profil…**. **Profils de
transfert…** (palette de commandes) les exécute, les renomme, les supprime et les planifie. La clé secrète
d'un bucket n'est jamais enregistrée dans le fichier de profil.

## Interroger un fichier de données sans l'importer

Déposez un fichier CSV, JSON, Parquet ou Excel sur la fenêtre, ou cliquez sur **Ouvrir un fichier de
données** sur l'écran d'accueil : il s'ouvre comme une base de données (chaque feuille Excel est une
table). La première fois, l'application propose de télécharger la bibliothèque de DuckDB.

## Importer des connexions

Sur l'écran d'accueil, **Importer des connexions** lit DBeaver, DataGrip et pgAdmin (et
`pg_service.conf`) ; les connexions Db2 de DBeaver et DataGrip, et les URL `jdbc:db2:`, sont lues aussi.
**Exporter les connexions** écrit un fichier `.platypusconn` pour un autre ordinateur : voir
[Projets et partage](projects-sharing.md).
