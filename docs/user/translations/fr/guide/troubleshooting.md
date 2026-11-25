<!-- source: 25c359215e93 -->
[← Guide utilisateur](README.md)

# Dépannage et FAQ

Commencez par **Tester la connexion** dans le formulaire de la connexion : elle signale l'échec avec le
message du serveur lui-même, et indique la version du serveur quand cela aide.

## Erreurs de connexion

### L'hôte est injoignable

Vérifiez d'abord l'hôte et le port (le formulaire dit « L'hôte est manquant. » ou « Le port doit être un
nombre de 1 à 65535. »). Regardez ensuite ce qui se trouve entre vous et le serveur : un tunnel SSH, un
proxy, une redirection de port, ou une commande autour de la connexion qui lance un VPN (page **Réseau**).
Chacun ajoute son propre message, plus bas.

### TLS

Sur la page **Réseau**, **Mode SSL** décide de ce qui est vérifié (voir [Se connecter](connect.md#tls)).

- Un serveur qui refuse une connexion non chiffrée demande `require` ou plus ; un serveur dont le certificat
  n'est pas reconnu par votre ordinateur demande `verify-ca` ou `verify-full` avec le **Certificat CA
  (PEM)**, ou `require` pour chiffrer sans vérifier.
- « invalid certificate » et « invalid root certificates » signalent un fichier PEM illisible : vérifiez
  que c'est bien le certificat, pas une clé.
- « a client certificate needs both the certificate and its key » : remplissez ensemble **Certificat client
  (PEM)** et **Clé client (PEM)**. « invalid client certificate or key » veut dire qu'ils ne
  correspondent pas ou ne sont pas au format PEM.
- Redis, Elasticsearch et Cassandra activent TLS à partir d'un mode SSL `require` ou plus.

### SSH

Les erreurs commencent par `SSH <user>@<host>:<port>:`.

- `no answer within 10 s` ou `cannot reach <host>:<port>` : le serveur SSH (ou le proxy placé devant)
  n'est pas joignable d'ici.
- `cannot read the key <path>` : le **Fichier de clé privée** est absent, illisible, ou demande sa
  phrase secrète.
- `no SSH agent is running` : démarrez votre agent SSH, ou utilisez un fichier de clé ou un mot de passe.
- Un mot de passe ou une clé refusés, c'est la réponse du serveur : vérifiez **Utilisateur SSH** et la méthode.
- Un serveur dont la clé a changé depuis la première connexion est refusé. Les clés sont celles de
  `~/.ssh/known_hosts`, partagé avec OpenSSH : supprimez-y l'ancienne ligne si le changement est attendu.

### Proxy

Les erreurs nomment le proxy, par exemple `SOCKS5 proxy host:1080: …` ou `HTTP proxy host:3128: …`.

- `no answer within 10 s` : le proxy ne répond pas.
- `the proxy asks for a user and password, or refused them` : écrivez-les dans l'URL
  (`http://user:password@host:3128`).
- `cannot reach <host>:<port>: …` : le proxy a répondu mais n'a pas pu joindre la base de données.
- Une URL non comprise est signalée par `proxy "<url>": …`. Une redirection de port va seule : retirez
  le tunnel SSH ou le proxy.

### Trousseau

- `keychain error: …`, ou « the system has no keychain to save the password in » : sous Linux, un
  Secret Service comme GNOME Keyring ou KWallet doit être en cours d'exécution. Sans lui, définissez un mot
  de passe principal (Paramètres › **Mot de passe principal**) ou choisissez **Demander à chaque fois**
  pour le mot de passe.
- « Aucun mot de passe n'est enregistré pour cette connexion : il sera enregistré dans le trousseau. »
  est un simple avis.
- Les mots de passe importés d'un autre outil sans leur mot de passe (DBeaver avec son propre mot de passe
  principal, par exemple) sont demandés à la première utilisation.

### Mot de passe principal

- « PlatypusDB est verrouillé » ou « Saisissez le mot de passe principal pour les utiliser » : saisissez-le
  et cliquez sur **Déverrouiller**. Le verrouillage après inactivité se règle dans Paramètres › **Sécurité** ;
  les connexions de production se reconnectent quand on les utilise.
- Il est impossible de le récupérer. S'il est perdu, les mots de passe enregistrés le sont aussi : saisissez
  de nouveau les mots de passe dans chaque connexion.
- Pour revenir au trousseau, Paramètres › **Mot de passe principal** › **Désactiver…** (le mot de passe
  actuel est demandé).

## Un téléchargement de pilote échoue

DuckDB, Snowflake, BigQuery, Databricks, Trino, les autres pilotes ADBC, Oracle et IBM Db2 téléchargent leur
bibliothèque à la première utilisation. La fenêtre dit « Téléchargement du pilote `<name>`… » avec un bouton
**Annuler**, et en cas d'échec un message s'affiche avec **Réessayer**.

- `the download failed (check the internet connection)` ou `cannot reach the driver registry` : pas de
  route vers le registre ; vérifiez le réseau, le proxy ou le pare-feu, puis **Réessayer**.
- `the downloaded driver is corrupted (its SHA-256 does not match)` : le fichier a été modifié ou coupé
  en route ; réessayez, et signalez-le si cela persiste.
- `no <name> driver for this platform`, ou « PlatypusDB télécharge DuckDB sous Windows, macOS et Linux,
  sur processeurs x64 et ARM ; pas sur ce système. » : il n'existe pas de version pour votre système.
- `the driver's license (…) does not allow downloading it here` : l'application ne télécharge pas les
  pilotes dont la licence l'interdit.
- IBM Db2 n'est accessible par ODBC que sous Windows pour l'instant ; l'enregistrement de son pilote demande
  la permission d'administrateur (Contrôle de compte d'utilisateur). Répondez oui, sinon le pilote n'est
  pas enregistré.
- **Annuler** laisse le formulaire utilisable ; la proposition revient à **Tester la connexion** ou à la
  première ouverture.

Paramètres › **Pilotes** liste ce qui est téléchargé, supprime un pilote, et montre les pilotes ODBC du
système. Une suppression que Windows a refusée se termine au prochain démarrage.

## Un agent ne voit pas une connexion

Parcourez cette liste dans l'ordre.

1. **L'accès est désactivé.** L'agent répond « `<name>` is not available to agents ». Dans Paramètres ›
   **Agents** › **Sécurité**, mettez la connexion sur **Lecture seule** ou **Lecture et écriture**. Une
   connexion venue d'un projet partagé démarre sur **Désactivé** sur votre ordinateur.
2. **Un mot de passe principal.** L'agent répond « the saved password of `<name>` is locked by
   PlatypusDB's master password, which agents cannot enter ». Donnez à cette connexion une commande de mot
   de passe ou `.pgpass`, ou désactivez le mot de passe principal.
3. **Le mot de passe est demandé à chaque fois.** L'agent répond « `<name>` asks for its password at each
   connection, and agents cannot type it ». Enregistrez-le dans le trousseau, ou utilisez `.pgpass` ou une
   commande de mot de passe.
4. **Écritures refusées.** Les connexions de production et en lecture seule ne dépassent jamais la lecture
   seule ; sur les autres, la modification attend votre réponse dans l'application ([Agents](agents.md)).
5. **Le client n'a pas été redémarré** après **Installer**, ou l'entrée manque : Paramètres › **Agents**
   › **Configuration** indique si PlatypusDB figure dans le fichier du client.
6. **Une copie portable ou un autre profil.** L'agent lit les connexions du profil depuis lequel il a été
   démarré ; un client configuré avec l'application installée ne voit pas le dossier `data` d'une copie
   portable.

## La copie portable

- C'est un exécutable avec un dossier `data` vide à côté. Si le dossier n'est pas là au démarrage,
  l'application utilise à la place les dossiers du système : un dossier `data` renommé ressemble donc à des
  connexions perdues ; renommez-le comme avant.
- Tout est dans `data`, journaux compris ; le dossier se déplace avec l'exécutable. Les mots de passe
  enregistrés restent dans le trousseau du système de l'ordinateur, sauf si un mot de passe principal les
  garde aussi dans le dossier : sur un autre ordinateur, définissez d'abord le mot de passe principal ou
  saisissez de nouveau les mots de passe.
- Elle ne se met pas à jour toute seule : remplacez son exécutable par le nouveau. L'écran d'accueil affiche
  `portable` à côté de la version.

## Où sont les données et les journaux

L'application garde les connexions et les paramètres (`connections.toml`, `settings.toml`, et
`secrets.vault` avec un mot de passe principal) dans son dossier de configuration, et l'historique, les
scripts, les pilotes téléchargés et les journaux dans son dossier de données, tous deux nommés
`dev.platypusdb.desktop` :

| Système | Dossier de configuration | Dossier de données |
|---|---|---|
| Windows | `%APPDATA%\dev.platypusdb.desktop` | `%APPDATA%\dev.platypusdb.desktop` |
| macOS | `~/Library/Application Support/dev.platypusdb.desktop` | le même |
| Linux | `~/.config/dev.platypusdb.desktop` | `~/.local/share/dev.platypusdb.desktop` |

Une copie portable garde tout dans son dossier `data`, et la variable d'environnement
`PLATYPUS_DATA_DIR` met tout dans le dossier qu'elle désigne.

![Paramètres, la page Diagnostic](../../../../screenshots/guide/diagnostics.png)

Le journal est `logs/platypus.log` dans le dossier de données : une ligne par démarrage, erreur et plantage,
avec rotation à 1 Mo et deux anciens fichiers (`platypus.1.log`, `platypus.2.log`). Paramètres ›
**Diagnostic** affiche le dossier (**Copier**) et **Enregistrer le paquet de diagnostic…**, un zip avec la
version, le système, les paramètres sans les mots de passe et les journaux, à joindre à un rapport de bug.
Rien n'est envoyé nulle part ; relisez les journaux avant de les partager, car une erreur peut nommer un
serveur ou une table. Après un plantage, le démarrage suivant propose le même paquet.

## Mises à jour

- Au démarrage, l'application cherche une version plus récente et affiche « PlatypusDB `<version>` est
  disponible » avec **Plus tard** et **Installer et redémarrer**.
- Une copie portable se contente de le dire : remplacez son exécutable. Une installation depuis un
  gestionnaire de paquets (Flatpak, l'AUR) le dit aussi et lui laisse la mise à jour.
- Les pilotes téléchargés se mettent à jour séparément : Paramètres › **Pilotes** › **Rechercher des mises à
  jour**, ou les réglages **Rechercher les mises à jour des pilotes au démarrage** (activé, une fois par
  jour au plus) et **Installer seul les mises à jour des pilotes** (désactivé). Les pilotes intégrés se
  mettent à jour avec l'application.

## FAQ

**Puis-je utiliser PlatypusDB hors ligne ?** Oui, avec une base de données locale. Les téléchargements
(pilotes, mises à jour) demandent le réseau.

**Où signaler un bug ?** Sur le [suivi des tickets](https://github.com/platypus-labs/platypus-db/issues),
avec le paquet de diagnostic si possible.

**Où sont les raccourcis ?** [SHORTCUTS.md](../../../SHORTCUTS.md), et Paramètres › Raccourcis clavier pour les
modifier.
