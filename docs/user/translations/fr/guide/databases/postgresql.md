<!-- source: a2b0bfe9784d -->
[← Bases de données](README.md)

# PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB et les autres

Un seul pilote, construit sur `tokio-postgres`, parle le protocole PostgreSQL. Il ouvre PostgreSQL 12 et les
versions suivantes, ainsi que les serveurs et services qui parlent le même protocole. Rien n'est
téléchargé : le pilote fait partie de l'application.

## Connexion

Choisissez **PostgreSQL** dans la grille des bases de données, ou collez une URL : le formulaire se remplit
tout seul.

| Vous collez | Interprété comme |
|---|---|
| `postgres://user:pass@host:5432/db?sslmode=require` (ou `postgresql://`) | la forme URL ; le port par défaut est 5432 |
| `jdbc:postgresql://host/db?sslmode=require` | une URL JDBC, comme DBeaver et DataGrip les gardent |
| `host=db port=5432 dbname=shop user=app sslmode=require` | une chaîne de connexion libpq |
| `Host=db;Database=shop;Username=app;Password=…` | une chaîne de connexion Npgsql |

Paramètres d'URL que l'application connaît : `sslmode`, `sslrootcert`, `sslcert`, `sslkey`,
`application_name`, `connect_timeout` et `pgbouncer=true`. Tout autre paramètre est écarté, et le formulaire
dit lequel. Une liste d'hôtes de basculement (`h1:5432,h2:5432`) garde le premier hôte.

**TLS** suit le `sslmode` de libpq :

| Mode | Ce qui se passe |
|---|---|
| `disable` | pas de TLS |
| `prefer` (par défaut) | TLS si le serveur l'offre, sinon connexion en clair ; le certificat n'est pas vérifié |
| `require` | TLS, certificat non vérifié |
| `verify-ca` | TLS, la chaîne de certificats est vérifiée, pas le nom d'hôte |
| `verify-full` | TLS, la chaîne et le nom d'hôte sont vérifiés |

La chaîne est vérifiée avec le fichier de `sslrootcert` s'il y en a un, sinon avec les certificats de
confiance du système. Un certificat client (`sslcert` et `sslkey`) est envoyé dans tous les modes qui
utilisent TLS.

**La connexion** se fait par mot de passe, ou par un jeton que l'outil en ligne de commande d'un cloud crée à
chaque connexion : AWS IAM (`aws`), Google Cloud IAM (`gcloud`) et Microsoft Entra ID (`az`) sont dans le
choix *Authentification* de la connexion. L'outil doit être installé et connecté sur votre machine. Ou par
**Kerberos**, ci-dessous.

**Kerberos (GSSAPI).** Choisissez *Ticket Kerberos (GSSAPI)* sous *Password from*. Le champ du mot de passe
disparaît : aucun mot de passe n'est demandé, enregistré ni envoyé. Si le serveur en demande un, ou vous laisse
entrer sans demander Kerberos, la connexion est refusée et le dit.

- **Le nom du service** est `postgres` sauf si vous en indiquez un (le `krb_srvname` du serveur). Le ticket
  demandé est `service/host`, dans votre realm par défaut : l'hôte doit être le nom du principal du service,
  pas une adresse IP ni un alias, car l'application ne le résout pas. Par un tunnel SSH ou une redirection de
  port, c'est toujours le nom du serveur.
- **L'utilisateur** est le rôle PostgreSQL, que le serveur déduit de votre principal (`include_realm` et
  `pg_ident.conf`). Laissé vide, l'application utilise votre nom d'utilisateur système.
- **Le ticket** sous Linux vient de `kinit`, dans le cache que nomme `KRB5CCNAME` (ou le cache par défaut), ou
  d'un keytab : avec `KRB5_CLIENT_KTNAME` défini, Kerberos obtient le ticket lui-même. Ces variables doivent
  être définies là où l'application démarre. Sous Windows, c'est le ticket de votre session Windows.
- **Sans ticket**, le test dit quoi lancer : `kinit`, les deux variables et `klist`, suivis du texte propre de
  Kerberos. Il en va de même pour un ticket expiré, un service inconnu et un KDC injoignable.
- **`psql`, `pg_dump` et `pg_restore`** se connectent de la même façon (`PGKRBSRVNAME`, `PGREQUIREAUTH=gss`,
  et pas de `PGPASSWORD`). Leur copie de libpq doit avoir été compilée avec GSSAPI.
- **Quelles versions l'ont.** Les paquets Windows (SSPI) et Linux (GSSAPI, MIT Kerberos). Ceux de macOS non :
  aucun Mac ne l'a encore compilé, et le formulaire le dit. La fonctionnalité Cargo `kerberos` de
  `platypus-driver-pg` l'active ; sans elle, rien de Kerberos n'est lié ni exécuté.
- **Le chiffrement GSS** (`hostgssenc`, `gssencmode`) n'est pas utilisé : utilisez TLS pour le chiffrement,
  avec des lignes `hostssl` ou `host`. Un `krbsrvname` dans une URL collée est écarté, comme tout autre
  paramètre que l'application ne connaît pas.

**Poolers.** Derrière PgBouncer, Supavisor ou le pooler de Neon en transaction mode, choisissez *Transaction
mode* dans le formulaire (ou ajoutez `pgbouncer=true` à l'URL). Les instructions s'exécutent alors sans nom,
car la transaction suivante peut tomber sur une autre session du serveur. Le formulaire le suggère pour les
hôtes qui se terminent par `.pooler.supabase.com` et pour les hôtes Neon qui ont `-pooler` dans leur nom. Dans
ce mode, le débogueur PL/pgSQL est désactivé, et les sessions en lecture seule des agents reposent sur la
seule vérification de l'instruction.

Les autres serveurs ont leurs propres tuiles, qui ne font que définir des valeurs par défaut :

| Tuile | Valeurs par défaut qu'elle définit |
|---|---|
| CockroachDB | port 26257 |
| YugabyteDB | port 5433, utilisateur et base de données `yugabyte` |
| TimescaleDB | aucune |
| Supabase | utilisateur et base de données `postgres`, TLS `require` |
| Neon, AlloyDB | TLS `require` |

## Privilèges

Parcourir ne demande que ce que le serveur montre au login. Les onglets d'administration lisent ceci :

- **Sessions** et **Verrous** : `pg_stat_activity` et `pg_locks`. L'application liste les sessions client que
  le serveur renvoie ; ce que le serveur montre au login des sessions et des instructions des autres
  utilisateurs est sa propre règle, que l'application ne vérifie pas. *Annuler l'instruction* et *Terminer la
  session* appellent `pg_cancel_backend` et `pg_terminate_backend` ; le serveur décide si le login peut le
  faire.
- **Instructions les plus coûteuses** : l'extension `pg_stat_statements`. Sans elle, l'onglet dit comment
  l'activer (l'ajouter à `shared_preload_libraries`, redémarrer, `CREATE EXTENSION`).
- **Tables et index** : `pg_stat_user_tables` et `pg_stat_user_indexes`.
- **Paramètres** et **Tableau de bord** : `pg_settings`, `pg_stat_database`, `pg_stat_activity` et les vues de
  réplication ; les standbys ne sont listés qu'à un membre de `pg_monitor`. Un chiffre que le login ne peut
  pas lire affiche « — » et pourquoi.
- **Utilisateurs et rôles** : `pg_roles` et `pg_auth_members` ; les modifier demande les droits que les
  instructions exigent.
- **Maintenance** : `VACUUM`, `ANALYZE` et `REINDEX` sur la table.
- **Sauvegarder… et Restaurer…** lancent `pg_dump`, `pg_restore` et `psql`, qui doivent être dans le PATH.
- **Déboguer…** demande l'extension `pldebugger` sur le serveur.

## Ce qui fonctionne

- La modification des données dans la grille, par clé primaire (ou clé unique sur des colonnes non nulles, ou
  clé virtuelle que vous définissez). Les tableaux et les valeurs composites se modifient un élément ou un
  champ par ligne.
- Les transactions avec commit manuel, et les instructions annulées côté serveur.
- Explain et Explain analyze ; le plan est lu depuis le JSON ou le texte de PostgreSQL.
- Plusieurs bases de données par connexion, les partitions, les géométries PostGIS, les tableaux et les types
  composites.
- Les imports et copies vers PostgreSQL utilisent `COPY`.
- Tous les onglets d'administration ci-dessus, le débogueur PL/pgSQL, et les outils de schéma (éditeur de
  tables, diagramme ER, comparaison).

**CockroachDB et YugabyteDB** s'ouvrent avec le même pilote, mais l'application détecte de quel serveur il
s'agit, et les outils serveur qui s'appuient sur les vues propres à PostgreSQL sont désactivés : pas
d'onglets sessions, verrous, tableau de bord, instructions, tables et index, paramètres, utilisateurs ou
maintenance, et pas de débogueur. CockroachDB a son propre format de plan (Explain et Explain analyze
fonctionnent) et les imports passent par des instructions `INSERT` au lieu de `COPY`. Une table CockroachDB
sans clé se modifie par son `rowid` caché. TimescaleDB (testé), Supabase, Neon et AlloyDB (non testés sur les
services) sont du PostgreSQL pour l'application, avec tout ce qui précède ; ce qu'un service hébergé restreint
pour votre rôle relève de sa propre règle.

## Agents

Les agents en lecture seule et les connexions en lecture seule ouvrent la session avec
`default_transaction_read_only` (PostgreSQL, CockroachDB et YugabyteDB), si bien que le serveur lui-même
refuse les écritures ; derrière un pooler en transaction mode, seule la vérification de l'instruction s'applique.
`server_activity` et `top_queries` fonctionnent sur PostgreSQL proprement dit, pas sur CockroachDB ni
YugabyteDB. Voir [Agents et la CLI](../../../../AGENTS.md).

## Limites connues

- Plus ancien que 12 : l'application se connecte et avertit que certaines fonctionnalités peuvent ne pas
  marcher.
- Le débogueur est désactivé derrière un pooler en transaction mode.
- Le mode terminal demande `psql` dans le PATH.

## Testé sur

La suite de conformité tourne sur PostgreSQL 12, 14, 16 et 17, TimescaleDB 2.30 sur PostgreSQL 17,
CockroachDB 26.2 et YugabyteDB 2025.2, plus PgBouncer en transaction mode, un certificat client sur TLS
mutuel, et le débogueur sur un serveur avec `pldebugger`. Supabase, Neon et AlloyDB ne sont pas testés : c'est
le pilote PostgreSQL avec des valeurs par défaut.

Kerberos est testé avec un vrai ticket d'un vrai KDC (MIT Kerberos, dans le profil `kerberos` de
`docker/compose.yml`) sur PostgreSQL 17, depuis un client Linux avec GSSAPI : un ticket de `kinit`, un keytab,
TLS, `psql`, pas de ticket, un ticket expiré et un mauvais nom de service. **La connexion par le SSPI de
Windows n'a pas été essayée avec un vrai ticket :** une machine Windows hors domaine ne peut pas joindre le
KDC de test sans modifier des réglages système (`ksetup`). Sous Windows, seul ce qui se passe sans ticket est
testé. Une ligne `sspi` du serveur (PostgreSQL sous Windows) n'a pas non plus été essayée.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
