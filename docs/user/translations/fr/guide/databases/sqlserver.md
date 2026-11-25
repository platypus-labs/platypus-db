<!-- source: 3830a0c639a7 -->
[← Bases de données](README.md)

# SQL Server et Azure SQL

Le pilote parle lui-même TDS (un `tiberius` modifié), donc il n'y a rien à installer ni à télécharger. Il est
écrit pour SQL Server 2017 (14.0) et les versions suivantes, et pour Azure SQL (non testé sur le service :
voir la fin de cette page).

## Connexion

| Vous collez | Interprété comme |
|---|---|
| `sqlserver://sa:pass@host:1433/master?encrypt=true&trustServerCertificate=true` (ou `mssql://`) | la forme URL ; port par défaut 1433 |
| `jdbc:sqlserver://host:1433;databaseName=shop;encrypt=true` | une URL JDBC (le pilote de Microsoft, jTDS ou l'ancien `jdbc:microsoft:`) |
| `Server=tcp:db.example.com,1433;Database=shop;User Id=sa;Password=…;Encrypt=True` | une chaîne de connexion ADO.NET ou ODBC |

Dans une URL, `database` ou `databaseName` nomme la base de données, et `encrypt` et
`trustServerCertificate` sont lus comme les lisent les chaînes JDBC et ADO.NET (sauf si `sslmode` est aussi
donné).

Les **instances nommées** (`host\SQLEXPRESS`) ne sont pas recherchées par SQL Server Browser : indiquez le
port de l'instance. Une chaîne collée avec une instance et sans port le signale.

**TLS** : `disable` désactive le chiffrement. `prefer` (par défaut) chiffre sans vérifier le certificat (les
serveurs en ont souvent un auto-signé), et se rabat sur une connexion en clair si le serveur n'a pas de
chiffrement. `require` chiffre sans vérifier. `verify-ca` et `verify-full` vérifient tous deux la chaîne
**et** le nom d'hôte, parce que la bibliothèque TLS vérifie toujours le nom ; faites pointer `sslrootcert`
vers le fichier de votre CA, sinon les certificats de confiance du système sont utilisés.

**La connexion** :

- *SQL Server login* : utilisateur et mot de passe.
- *Windows sign-in* (`Integrated Security=SSPI` dans une chaîne collée) : seulement quand PlatypusDB tourne
  sous Windows ; ailleurs, c'est une erreur.
- *Microsoft Entra ID* : un jeton créé par l'outil `az`, qui doit être installé et connecté. C'est ainsi que
  l'on atteint la connexion Entra d'Azure SQL.

**Azure SQL** : la passerelle peut envoyer le client vers le nœud propre de la base de données ; l'application
suit une seule redirection de ce genre, sauf par un tunnel SSH, qui ne mène qu'à une seule adresse.

## Privilèges

Parcourir ne demande que ce que le login peut voir. Les onglets d'administration lisent :

- **Sessions** et **Verrous** : `sys.dm_exec_sessions`, `dm_exec_requests`, `dm_exec_connections` et
  `dm_tran_locks`. Pour voir ce que font les autres logins, le login a besoin de `VIEW SERVER STATE`.
- **Instructions les plus coûteuses** : `sys.dm_exec_query_stats`, le cache de plans.
- **Tables et index** : `sys.tables`, `dm_db_partition_stats`, `dm_db_index_usage_stats` et `sys.stats`.
- **Paramètres** : `sys.configurations` ; *Ouvrir dans l'éditeur* écrit une instruction `sp_configure`.
- **Tableau de bord** : les compteurs de performance, les tailles de `sys.databases` et `sys.master_files`, et
  les groupes de disponibilité quand il y en a ; un chiffre que le login ne peut pas lire affiche « — ».
- **Utilisateurs et rôles** : les logins, rôles et droits du serveur.
- **Maintenance** : `UPDATE STATISTICS`, `ALTER INDEX ALL … REBUILD` et `DBCC CHECKTABLE`.

## Ce qui fonctionne

- La modification des données dans la grille, par clé primaire (ou clé unique sur des colonnes non nulles, ou
  clé virtuelle).
- Le commit et le rollback manuels ; l'annulation d'une instruction en cours (une attention TDS) conserve la
  session, sa transaction et ses tables temporaires.
- Plusieurs jeux de résultats pour un seul lot, et les messages `PRINT`.
- Plusieurs bases de données par connexion : une connexion qui nomme sa base de données n'affiche que celle-là ;
  *Afficher toutes les bases de données* liste les autres.
- Explain montre le plan **estimé** (`SHOWPLAN`). Explain analyze est refusé.
- *Terminer la session* est un `KILL`. Annuler l'instruction d'une autre session n'est pas possible sur SQL
  Server, donc l'onglet ne propose que de terminer la session.
- Les imports utilisent la copie en bloc de SQL Server (des insertions `OPENJSON` pour les tables avec des
  colonnes `money`, `xml` ou héritées).
- Les outils de schéma (éditeur de tables, diagramme ER, comparaison), y compris pour les colonnes calculées
  et à collation, les index riches et les tables temporelles. Les index XML et spatiaux sont laissés de côté
  dans une comparaison.
- Le mode terminal exécute `sqlcmd`, s'il est dans le PATH.

## Agents

Sur SQL Server, la seule vérification de l'instruction empêche les écritures des agents en lecture seule
(pas de session en lecture seule côté serveur) ; la transaction d'un agent est annulée. `server_activity` et
`top_queries` fonctionnent. Voir [Agents et la CLI](../../../../AGENTS.md).

## Limites connues

- Plus ancien que 2017 : l'application se connecte et avertit que certaines fonctionnalités peuvent ne pas
  marcher.
- Pas d'Explain analyze et pas de recherche d'instance nommée.

## Testé sur

La suite de conformité tourne sur SQL Server 2017 et 2022 (conteneurs), et le pilote ODBC générique est essayé
sur le même conteneur 2022. Azure SQL n'est pas testé : ses règles de redirection, de connexion Entra et de
chiffrement sont dans le code mais n'ont pas été exécutées sur le service.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
