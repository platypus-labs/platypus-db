<!-- source: fd0bf4386fb3 -->
[← Bases de données](README.md)

# Oracle

Le pilote utilise la bibliothèque cliente d'Oracle (ODPI-C, via le crate `oracle`), chargée à la connexion.
Cette bibliothèque est Oracle Instant Client, que l'application télécharge à la première utilisation.

## Première utilisation : Instant Client

Quand vous choisissez **Oracle** pour une nouvelle connexion et que le client n'est pas sur cette machine, une
fenêtre s'ouvre au-dessus du formulaire : *Oracle nécessite Oracle Instant Client*. Elle montre le paquet
(Basic Light 23.26.4), sa taille, sa licence (Oracle Free Use Terms and Conditions) et d'où il vient, et
vérifie le téléchargement par un SHA-256. **Télécharger** le place dans le dossier de données de
l'application ; **Annuler** laisse le formulaire utilisable, et la proposition revient à *Tester la
connexion* et à la première ouverture. *Paramètres › Pilotes* liste le client et le supprime.

- L'application le télécharge sous Windows (x64) et Linux (x64 et ARM64). Sous macOS, elle ne télécharge
  rien : installez Instant Client depuis Oracle et mettez son dossier dans le chemin des bibliothèques.
- Si l'application n'en a pas téléchargé, elle utilise un Instant Client ou un client complet que le système
  peut charger.
- Windows demande le Microsoft Visual C++ Redistributable, et Linux la bibliothèque `libaio` (`libaio1`,
  `libaio1t64` ou `libaio` selon la distribution) ; quand le client ne peut pas se charger, l'erreur nomme
  l'élément manquant.

## Connexion

Il n'y a pas d'URL `oracle://` ; collez plutôt une URL JDBC, comme `jdbc:oracle:thin:@host:1521/service` (les
formes thin et OCI sont lues), ou remplissez le formulaire :

- **Hôte** et **port** (1521 par défaut), et **Nom du service** dans le champ Base de données (par exemple
  `FREEPDB1`). Ils sont écrits sous forme de chaîne Easy Connect, `host:port/service`.
- **Sans hôte**, le champ Base de données est un nom de service réseau de `tnsnames.ora`, ou un descripteur de
  connexion complet, écrit tel quel.
- **Utilisateur et mot de passe.** Sans utilisateur, c'est le compte du système d'exploitation ou un wallet
  Oracle qui connecte. L'utilisateur peut s'écrire à la façon de SQL*Plus, `sys as sysdba`, ou vous pouvez
  choisir le **Rôle** (`SYSDBA`, `SYSOPER`, `SYSBACKUP`, `SYSDG`, `SYSKM`, `SYSASM`).
- **TLS** : les modes `disable` et `prefer` utilisent du TCP en clair ; `require` et au-delà utilisent
  `tcps://` avec les certificats de confiance du système. `require` ignore la vérification du nom du
  serveur ; `verify-ca` et `verify-full` la font. Le délai de connexion s'applique. Par un tunnel SSH,
  c'est l'extrémité locale du tunnel qui est utilisée.

## Privilèges

Parcourir lit les vues `ALL_*`, donc le login voit les objets sur lesquels il a des droits. Annuler envoie une
interruption ; si l'instruction tourne encore après quelques secondes, l'application l'annule depuis une
seconde session avec `ALTER SYSTEM CANCEL SQL`, ce qui demande ce privilège, et à défaut abandonne la session
(sa transaction est annulée et une nouvelle connexion est ouverte).

## Ce qui fonctionne

- La modification des données dans la grille, par clé primaire (ou clé unique sur des colonnes non nulles, ou
  clé virtuelle).
- Le commit et le rollback manuels.
- Les blocs PL/SQL : un script termine un bloc par une ligne `/`. Le texte de `DBMS_OUTPUT` revient sous forme
  de messages.
- Le navigateur (tables, vues, vues matérialisées, séquences, synonymes, fonctions, procédures, triggers,
  types et packages), le DDL des objets, et les outils de schéma.
- Explain montre le plan de `EXPLAIN PLAN` et `DBMS_XPLAN`, estimé seulement : il n'y a pas d'Explain
  analyze.
- Les erreurs de compilation, les nombres et les heures à la nanoseconde, le JSON, les vecteurs, les curseurs,
  les LOB longs et les colonnes de fichier sont relus tels que la base de données les contient.
- Pas d'onglets sessions, verrous, tableau de bord, instructions, utilisateurs ou paramètres.
- Le mode terminal n'est pas disponible.

## Agents

Les agents en lecture seule reposent sur la seule vérification de l'instruction : Oracle n'a pas ici de session
en lecture seule côté serveur. Une instruction qui appelle un package Oracle, comme `UTL_HTTP`, fait l'objet
d'une question avant de s'exécuter, comme une modification. Voir [Agents et la CLI](../../../../AGENTS.md).

## Limites connues

- L'application ne compare pas la version du serveur à un minimum ; le client qu'elle télécharge est 23.26.4.
- Pour un serveur plus ancien que 12, le navigateur lit le dictionnaire autrement (pas de
  `ORACLE_MAINTAINED`, pas de `FETCH FIRST`). Seul Oracle Free 23.26 tourne dans les tests, donc ce chemin
  n'est pas exercé. Autonomous Database et les autres services cloud d'Oracle ne sont pas testés non plus.
- Instant Client n'est pas téléchargé sous macOS.

## Testé sur

La suite de conformité tourne sur Oracle Free 23.26 (conteneur `gvenzl/oracle-free`), avec Instant Client
téléchargé ou celui du système : types, longs résultats, erreurs qui désignent la faute, PL/SQL, navigateur,
commit et rollback, enregistrement de nombreuses lignes modifiées, annulation (y compris d'un sleep PL/SQL),
plans, DDL qui repasse, connexion en `SYSDBA`, et comparaison de schémas. Les autres versions ne sont pas
testées.

Voir aussi : [Se connecter](../connect.md), [Dépannage](../troubleshooting.md).
