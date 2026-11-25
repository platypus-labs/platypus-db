<!-- source: 284fda1cc34a -->
[← Guide utilisateur](README.md)

# Se connecter à une base de données

![Choix de la base de données pour une nouvelle connexion](../../../../screenshots/connections.png)

## À partir d'une URL

1. Cliquez sur **Nouvelle connexion** (ou sur le `+` du navigateur).
2. Collez l'URL sous les logos : `postgres://`, `mysql://`, `sqlserver://`, `clickhouse://`,
   `libsql://`, `mongodb://`, `redis://`, `rediss://`, `elasticsearch://`, `opensearch://`, une
   URL JDBC ou une chaîne ADO.NET / ODBC. Le formulaire s'ouvre déjà rempli pour cette base de données.
3. Mettez le mot de passe dans le champ **Mot de passe**, pas dans l'URL : le formulaire vous
   le signale si l'URL en contient un.
4. Cliquez sur **Tester la connexion**, puis sur **Enregistrer**.

Ou choisissez la base de données parmi les logos (la recherche en haut accepte `Enter` pour le
premier résultat). Ce choix fixe le pilote ; **Retour** ramène aux logos.

## Le formulaire

Le formulaire a quatre pages : **Général**, **Apparence et sécurité**, **Réseau** et **Initialisation**. Un
point après le nom d'une page indique qu'elle contient des réglages.

- **Général** : hôte, port, utilisateur, base de données et origine du mot de passe (voir plus bas).
- **Apparence et sécurité** : l'environnement (Développement, Test ou Production), une couleur, la
  case **Lecture seule (refuse les modifications de données)**, les cases qui bloquent les exports et la
  modification des données, et l'accès des agents. Voir [Sécurité](safety.md) et [Agents](agents.md).
- **Réseau** : tunnel SSH, mode SSL et certificats, proxy, redirection de port et commandes autour de la
  connexion.
- **Initialisation** : un fichier de variables, un schéma ou un fuseau horaire par défaut, et des instructions
  exécutées à chaque connexion.

## Mots de passe

Dans **Général**, le mot de passe vient de l'une de ces sources :

- **Enregistré dans le trousseau** : le trousseau du système. **Tester la connexion** ne l'envoie qu'au
  serveur pour lequel il a été enregistré : si vous changez l'hôte, le port ou les serveurs SSH
  intermédiaires, l'application vous demande de saisir le mot de passe pour ce test. Une autre base de
  données du même serveur PostgreSQL ou MySQL n'est pas un autre serveur.
- **Demander à chaque fois** : saisi à chaque connexion. Les agents ne peuvent pas ouvrir une telle
  connexion.
- **Fichier .pgpass** (PostgreSQL uniquement ; l'hôte, le port, la base de données et l'utilisateur de
  la connexion choisissent la ligne, comme le fait libpq).
- **Commande shell** : une commande qui affiche le mot de passe.

Tout champ peut lire une variable d'environnement écrite `${env:NAME}`. Les gestionnaires de mots de passe
(1Password, Bitwarden, HashiCorp Vault, AWS Secrets Manager) figurent dans la même liste pour toute
base de données dont le formulaire demande un mot de passe ou un jeton ; aucun n'est limité à une seule
base de données. Indiquez la référence du secret : l'application construit la commande shell à partir
de celle-ci (`op read`, `bw get password`, `vault kv get`, `aws secretsmanager get-secret-value`) et
l'exécute à chaque connexion. Leur outil en ligne de commande doit être installé et connecté. Rien de
tout cela n'a été essayé avec un vrai compte 1Password, Bitwarden, Vault ou AWS : seules les commandes
construites par l'application et la commande shell elle-même sont testées.

Certaines bases de données se connectent sans aucun mot de passe, et la même liste les propose : un jeton
obtenu avec l'outil en ligne de commande d'un cloud (AWS IAM et Google Cloud IAM pour PostgreSQL et MySQL,
Microsoft Entra ID pour ces deux bases et SQL Server), la connexion Windows pour SQL Server, et un
**Kerberos ticket** pour PostgreSQL sous Windows et Linux. Lancez d'abord `kinit`, ou définissez
`KRB5CCNAME` ou `KRB5_CLIENT_KTNAME` ; sous Windows, le ticket est celui de votre session. Le ticket est la
seule chose envoyée au serveur, et un test sans ticket indique ce qu'il faut lancer. La
[page PostgreSQL](databases/postgresql.md) donne les détails, et ce qui est testé ou non : un vrai ticket a
permis de se connecter depuis Linux, pas encore par le SSPI de Windows.

### Mot de passe principal

Un mot de passe principal garde les mots de passe enregistrés dans un fichier chiffré au lieu du trousseau.

1. Ouvrez Paramètres (`Ctrl+,`) › **Mot de passe principal**.
2. Cliquez sur **Définir un mot de passe principal…**, saisissez-le deux fois (8 caractères au minimum) et
   confirmez.
3. Pour revenir au trousseau, cliquez sur **Désactiver…** sur la même page. **Verrouiller maintenant**
   verrouille aussitôt.

Il est impossible de le récupérer : sans lui, les mots de passe enregistrés sont perdus. Les agents ne peuvent
pas le saisir (voir [Dépannage](troubleshooting.md#un-agent-ne-voit-pas-une-connexion)).

## TLS

Sur la page **Réseau**, **Mode SSL** prend les valeurs de libpq : `disable`, `prefer`, `require`,
`verify-ca` et `verify-full`. `prefer` et `require` chiffrent sans vérifier le certificat ;
`verify-ca` et `verify-full` le vérifient, donc un **Certificat CA (PEM)** peut être nécessaire. Pour les serveurs
qui authentifient les clients par certificat, indiquez le **Certificat client (PEM)** et la **Clé client
(PEM)** (PostgreSQL et MySQL).

## Tunnel SSH

![Un tunnel SSH sur la page Réseau](../../../../screenshots/guide/connect-network.png)

1. Sur la page **Réseau**, cochez **Se connecter via un tunnel SSH**.
2. Remplissez **Hôte SSH**, le port et **Utilisateur SSH**.
3. Authentifiez-vous avec un mot de passe, un **Fichier de clé privée** (et sa phrase secrète) ou l'**Agent SSH**.
   Pour un bastion, ajoutez un **Hôte de rebond** (**D'abord via un hôte de rebond**).
4. Cliquez sur **Tester la connexion**.

La clé du serveur est comparée à `~/.ssh/known_hosts` : un serveur inconnu est mémorisé la première
fois, et une clé qui a changé depuis est refusée. Un **Profil réseau** partage un même tunnel et les mêmes
réglages SSL entre plusieurs connexions.

## Proxy et redirection de port

Toujours sur **Réseau** : un proxy SOCKS5 ou HTTP (`socks5://user:password@host:1080`,
`http://host:3128`), ou une redirection de port via `kubectl` ou une session AWS Systems Manager (la
CLI doit être installée et connectée). Une redirection de port va seule : retirez le tunnel SSH ou le proxy.

## Et ensuite

Les détails de chaque base de données : [Bases de données](databases/README.md). La connexion au cloud, les
poolers, les imports depuis DBeaver, DataGrip et pgAdmin (connexions Db2 comprises) et l'export des connexions sont
dans [FEATURES.md](../../../FEATURES.md#connections). Si la connexion échoue, voir
[Dépannage](troubleshooting.md#erreurs-de-connexion).
