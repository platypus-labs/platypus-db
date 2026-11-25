<!-- source: d4ffd9bf1ac2 -->
[← Guide utilisateur](README.md)

# Exécuter des requêtes et lire les résultats

![Complétion de JOIN à partir des clés étrangères](../../screenshots/autocomplete.png)

## Exécuter une instruction

1. Double-cliquez sur une connexion (ou une table) pour ouvrir un éditeur SQL.
2. Saisissez une instruction. `Ctrl+Space` complète les mots-clés, les tables et les colonnes ; `JOIN `
   propose les tables liées d'après les clés étrangères.
3. Appuyez sur `Ctrl+Enter` pour exécuter l'instruction sous le curseur (ou la sélection), ou sur `Alt+X`
   pour exécuter tout le script. `Ctrl+\` exécute dans un nouvel onglet de résultat.
4. `Ctrl+.` annule une instruction en cours d'exécution.

Une instruction se termine à un `;`, ou à une ligne vide avant un mot qui en commence une nouvelle. Quand
une instruction échoue, l'erreur est signalée dans le texte et le panneau d'erreur propose **Aller à
l'erreur**. `Ctrl+Shift+F` formate le script ; `Ctrl+S` l'enregistre dans un fichier `.sql`.

## Paramètres et variables

Les `:id` et `?` d'une instruction, ainsi que les variables `${name}`, sont demandés dans une boîte de
dialogue avant l'exécution et conservés dans un panneau Variables. La valeur d'un `${name}` entre dans le SQL
telle quelle : une instruction avec des variables est donc vérifiée de nouveau une fois remplie (voir
[Sécurité](safety.md)).

## Lire les résultats

- La grille lit les lignes une page à la fois. Cliquez sur l'en-tête d'une colonne pour trier : cela écrit
  le `ORDER BY` dans l'éditeur et relance la requête ; un deuxième clic inverse l'ordre, un troisième remet
  celui de la requête.
- Ouvrez une table depuis le navigateur pour la parcourir ; un `WHERE` saisi dans l'éditeur la filtre.
- `F7` ouvre le panneau de valeur : texte long, JSON et XML mis en forme, hexadécimal et images des
  colonnes binaires.
- Une cellule qui contient un fichier (un PDF, une image, un ZIP et d'autres, reconnus à leurs octets)
  affiche son type et sa taille, par exemple `PDF · 2.3 MB`. Le menu de la grille propose **Enregistrer PDF…**
  (ou le type du fichier) et, sur plusieurs cellules d'une colonne, **Enregistrer N fichiers dans un
  dossier…**. Le panneau de valeur a le même bouton.
  Une cellule de texte qui contient un fichier (une URL `data:`, du base64 ou de l'hexadécimal) n'a pas
  d'étiquette dans la grille, car il faudrait lire toutes les cellules de texte pour le savoir : le menu de la
  grille et le panneau de valeur l'affichent, pour les cellules que vous sélectionnez.
- `Ctrl+F` cherche dans les lignes chargées ; `Ctrl+Shift+C` copie la sélection avec les en-têtes.
- Un résultat peut passer de la grille à un graphique depuis la barre d'état.

![Cellules contenant des fichiers, avec leur type et leur taille](../../screenshots/guide/file-cell.png)

## Comprendre une requête lente

Appuyez sur `Ctrl+Shift+E` sur une instruction pour voir son plan. Voir
[FEATURES.md](../FEATURES.md#query-plans-and-performance).

## Pour aller plus loin

Les scripts enregistrés, les snippets, l'historique, le mode vim, l'exécution d'un script sur plusieurs
connexions, le constructeur de requêtes et l'analyse de données sont dans
[FEATURES.md](../FEATURES.md#sql-editor). La grille de résultats est dans
[FEATURES.md](../FEATURES.md#results-and-the-data-viewer).
