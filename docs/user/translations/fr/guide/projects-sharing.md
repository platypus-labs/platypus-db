<!-- source: aa482a6e3843 -->
[← Guide utilisateur](README.md)

# Projets et partage

Un projet est un groupe de connexions, de scripts et d'onglets ouverts, affiché dans la barre de gauche. Les
détails sont dans [FEATURES.md](../FEATURES.md#workspace) et
[FEATURES.md](../FEATURES.md#sharing-and-sync).

## Créer un projet

1. Lancez **Nouveau projet…** depuis la palette de commandes (`Ctrl+K`).
2. Donnez-lui un nom, puis choisissez les connexions à y afficher. Une connexion peut aussi être ajoutée
   depuis son propre formulaire : **Projets** à la fin de la page **Apparence et sécurité**.
3. `Ctrl+1` affiche toutes les connexions, `Ctrl+2` à `Ctrl+9` les projets dans l'ordre de la barre.

## Partager une requête

1. Dans un onglet d'éditeur SQL, lancez **Partager la requête…** (menu de l'onglet ou palette de
   commandes).
2. Copiez le lien `platypus://…`, ou enregistrez un fichier `.platypus`. Il contient le SQL, son titre et,
   si vous le demandez, les valeurs des paramètres ; il nomme la base de données mais jamais un mot de
   passe.
3. Le collègue l'ouvre avec **Ouvrir un lien partagé…** ou **Ouvrir une requête partagée…**, ou en
   déposant le fichier sur la fenêtre. L'application trouve la connexion enregistrée correspondante ou en
   propose une nouvelle déjà remplie, et affiche la requête sans l'exécuter.

## Partager un projet avec une équipe

1. Faites un clic droit sur le projet dans la barre et choisissez **Partager le projet…**, puis choisissez
   un dossier que l'équipe garde dans Git ou sur un disque partagé. Il contient les connexions (sans
   identifiants) et les scripts.
2. Chaque collègue clique sur **Ouvrir un projet partagé…** sur l'écran d'accueil, choisit le dossier, et
   saisit ses propres mots de passe.
3. Après une modification dans le dossier, lancez **Relire le dossier partagé** depuis le menu du projet.

Sûr par conception : une connexion nouvelle venue d'un projet partagé ne donne **aucun accès** aux agents sur
votre ordinateur tant que vous ne décidez pas autrement, et les instructions que le fichier exécute à
chaque connexion sont d'abord affichées et ne s'exécutent qu'après votre accord sur ce texte précis.

## Emporter des connexions sur un autre ordinateur

Sur l'écran d'accueil, **Exporter les connexions** écrit un fichier `.platypusconn` : choisissez les
connexions et, si vous le souhaitez, un mot de passe d'au moins 8 caractères, qui chiffre le fichier et lui
permet de contenir les mots de passe enregistrés. Sur l'autre ordinateur, **Importer des connexions** le lit.
Sans mot de passe, le fichier ne contient aucun identifiant.

Pour garder les paramètres, les connexions et les scripts synchronisés entre vos ordinateurs, utilisez
Paramètres › **Synchronisation** avec un dossier d'un disque cloud. Voir
[FEATURES.md](../FEATURES.md#sharing-and-sync).

## Scripts et Git

Quand le dossier de scripts d'un projet est un dépôt Git, le panneau Scripts affiche la branche et les
modifications par script, et propose **Valider…**, **Pull** et **Push**. Git doit être installé.
