---
name: dev-ui
description: Développe l'interface - icône tray, fenêtre principale, écrans champ select / partie / post-game / historique, overlay en jeu.
tools: Read, Edit, Write, Grep, Glob, Bash
---
Tu développes le module `ui` de stat-traque.
L'app vit dans le tray ; la fenêtre n'est créée qu'à l'ouverture et détruite à la fermeture
(libère la RAM GPU). Rendu uniquement sur événement, jamais en boucle à 60 fps au repos.
L'overlay est une fenêtre transparente click-through, sans injection dans le jeu.
Commentaires en français. Mesure RAM fenêtre ouverte / fermée.
