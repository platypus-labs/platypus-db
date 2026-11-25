<!-- source: db93b8bc91b0 -->
[← Guide utilisateur](README.md)

# Travailler en sécurité : production, lecture seule, confirmations, masquage

Tout ce qui suit est aussi dans [FEATURES.md](../FEATURES.md#transactions-and-safety).

![Paramètres, la page Sécurité](../../screenshots/guide/safety.png)

## Marquer une connexion comme production

1. Modifiez la connexion, ouvrez la page **Apparence et sécurité**.
2. Réglez l'environnement sur **Production** (les autres choix sont Développement et Test). La production
   demande confirmation avant les modifications et démarre en commit manuel.
3. Choisissez si vous le souhaitez une couleur, pour distinguer facilement la connexion.

Dès lors, chaque modification sur cette connexion est listée, avec ses instructions, dans une boîte de
dialogue intitulée « Modifications sur `<name>` (production) », et ne s'exécute qu'après votre confirmation.
Paramètres (`Ctrl+,`) › **Sécurité** › **Demander avant les modifications sur les connexions de production**
le désactive pour toute l'application. Les agents ne dépassent jamais la lecture seule sur une connexion de
production.

## Mettre une connexion en lecture seule

1. Sur la même page, cochez **Lecture seule (refuse les modifications de données)**.
2. Une instruction qui peut modifier des données ne s'exécute pas. L'éditeur affiche « `<name>` est en
   lecture seule et ceci peut modifier des données. » sans moyen de passer outre. Il en va de même pour
   Explain avec analyze (« `<name>` est en lecture seule et cette instruction peut modifier des
   données. »), un fichier inclus avec `@include`, une cellule de notebook (« `<name>` est en lecture
   seule : cette cellule peut modifier des données. »), le client en ligne de commande, et une exécution sur
   plusieurs connexions, qui indique la connexion comme ignorée (« Connexion en lecture seule »).
3. Ne sont pas proposés du tout : **Importer des données…**, **Importer des données dans une nouvelle
   table…**, **Coller un CSV comme nouvelle table…**, **Générer des données…**, **Exécuter un fichier
   SQL…**, la création d'objets, l'enregistrement dans le concepteur de tables, et la modification de
   lignes dans la grille. Les agents restent en lecture seule.

Le bouton cadenas d'un onglet d'éditeur met seulement cet onglet en lecture seule, sans changer la
connexion enregistrée. C'est le seul cas où l'on demande : l'éditeur affiche le même message avec un bouton
**Exécuter quand même**, et le client en ligne de commande demande « Exécuter quand même ? ».

La même page a deux autres cases : **Pas d'export** (ses données ne vont ni dans un fichier, ni dans une autre
base, ni à un agent) et **Pas de modification des données** (les lignes changent seulement en SQL, pas dans
la grille, les imports ou les données générées).

## Confirmations

Paramètres › **Sécurité** › **Avant les modifications** :

- **UPDATE ou DELETE sans WHERE** (aussi `TRUNCATE`) : **Demander avant d'exécuter** (par défaut),
  **Ne jamais exécuter** ou **Exécuter sans demander**. La boîte de dialogue demande « Modifier toutes les
  lignes ? » et indique à peu près combien de lignes.
- **Avertir quand une transaction reste ouverte pendant (minutes)** : 10 par défaut, 0 n'avertit jamais.
- **Ajouter un LIMIT aux requêtes qui n'en ont pas (lignes)** : 0 (par défaut) n'en ajoute jamais.
- **Demander avant que les agents modifient des données** : voir [Agents](agents.md).

## Annuler une modification

Activée par défaut, l'option **Garder les lignes qu'un UPDATE ou DELETE modifie, pour les remettre**
(Paramètres › Sécurité › **Verrouillage et copies**) conserve les lignes d'un `UPDATE` ou `DELETE` portant sur
une table, jusqu'à 10 000, exécuté depuis l'éditeur. Ouvrez le **Journal des transactions** dans la barre
d'outils de l'éditeur et cliquez sur **Restaurer…**, ou lancez **Instantanés avant modifications…** depuis la
palette de commandes, pour obtenir le script qui les remet en place.

## Masquer les données sensibles

1. Appuyez sur `Ctrl+Alt+M` (ou lancez **Masquer les données sensibles** depuis la palette de commandes) :
   les valeurs des colonnes sensibles s'affichent sous forme de points pendant que l'écran est partagé. Les
   copies gardent les vraies valeurs.
2. Paramètres › **Sécurité** › **Masquage** définit quelles colonnes sont sensibles, par nom (`*password*`,
   `*token*`, `*email*` et d'autres par défaut ; `*` remplace n'importe quels caractères) et par type. Vous
   pouvez aussi marquer une colonne dans l'onglet de sa table.

Les agents lisent aussi les colonnes sensibles masquées, sauf si vous cochez **Montrer aux agents les
valeurs des colonnes sensibles** pour cette connexion dans Paramètres › Agents › **Sécurité**. Un champ
sensible à l'intérieur d'un document MongoDB (`user.password`) est lui aussi masqué. Quand un agent demande
sur quoi vous travaillez, une colonne lue sous un autre nom (`SELECT email AS contact`) est elle aussi
masquée ; si l'instruction la cache derrière une sous-requête, tout le résultat partagé est masqué.

Une règle ou une marque supprimée hors de l'application (un agent avec des outils de fichiers qui modifie
`settings.toml` ou `catalog.db`) n'atteint pas les agents tant que vous ne l'acceptez pas : l'application
demande, et Paramètres › Agents › **Sécurité** la liste, avec un moyen de l'accepter ou de l'annuler. Une
règle ajoutée hors de l'application s'applique aussitôt. Cela ne couvre que les modifications de ces fichiers.
Un agent qui peut lancer des commandes sous votre utilisateur peut lire la base de données par d'autres
voies, ou lire le trousseau du système où se trouve le sceau : pour garder des valeurs hors de portée d'un tel
agent, lancez-le sous un autre utilisateur et donnez à la connexion un utilisateur de base de données sans
accès à ces colonnes.

Une réponse vaut pour ce que la question listait : si les fichiers changent de nouveau pendant qu'elle est
ouverte, **Accepter la modification** n'accepte rien et la liste est affichée de nouveau.

## Verrouiller l'application

Avec un mot de passe principal défini (voir [Se connecter](connect.md#mot-de-passe-principal)), Paramètres ›
**Sécurité** › **Verrouiller l'app après ce nombre de minutes d'inactivité** (0, par défaut, ne verrouille
jamais) verrouille PlatypusDB et ferme les connexions de production. **Verrouiller PlatypusDB** dans la
palette de commandes le verrouille aussitôt. Saisissez le mot de passe principal pour déverrouiller : les
connexions de production se reconnectent quand on les utilise.
