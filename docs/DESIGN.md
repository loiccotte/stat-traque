# Design : thème « D, étoilé minimal, version LoL »

Maquette de référence validée par le propriétaire (6 écrans à reproduire : voir plus bas).
Le thème remplace `src/ui/theme.rs` (bleu nuit + or du client) par une version plus sobre.
Règle : **rien ne doit coûter de CPU au repos** (pas d'animation, redessin seulement sur événement)
et la RAM de la fenêtre reste sous 40 Mo.

## Idée
Un ciel presque noir, des filets or fins, de grands espaces. Les notes des joueurs
(0 à 100, lettre D à S) sont des **étoiles qui s'allument**. Une seule couleur vive : l'or clair des S.

## Jetons de couleur
| Rôle | Valeur | Remarque |
|---|---|---|
| Fond fenêtre | `#05060C` | dégradé radial bleu nuit `#10213C` en haut (dessiné une fois) |
| Texte | `#ECE8DC` | contraste ≥ 7:1 sur le fond |
| Texte secondaire | `#A39F94` | contraste ≥ 4,5:1 minimum, jamais plus sombre |
| Accent (or clair, notes S) | `#F0C870` | |
| Or (filets, titres, portraits) | `#C8AA6E` | |
| Ligne / séparateur | `#1D2238` ; ligne fine de rangée `#151A2E` | |
| Victoire | `#7FD8A8` ; Défaite `#E88A7A` | |
| Équipe bleue / rouge | `#8FB4FF` / `#FF9D7A` | seulement pour les équipes, jamais pour les notes |

## Typographie
- Titres de section et nom de l'app : **Cinzel** (capitales, espacement large), couleur or. À embarquer
  seulement si le coût est acceptable (~100 Ko d'exe) ; repli : police egui par défaut en capitales espacées.
- Texte courant : la police par défaut d'egui (proche d'Inter).
- Les chiffres alignés en colonnes utilisent des chiffres tabulaires.

## Formes
- **Aucune carte arrondie.** Les panneaux sont des rectangles à **angles droits**, fond `#0B1428` à 67 %,
  filet or `#C8AA6E` à 19 %, et **deux petits losanges or (7 px)** posés sur le bord haut (gauche et droite).
- Onglet actif : texte or, soulignement or 2 px. Onglets inactifs : texte secondaire.
- Barre de progression : dégradé `#785A28` → `#C8AA6E`, piste sombre (pas blanche).
- Séparateur de rangée : ligne fine `#151A2E`.
- **Portrait de champion** : carré 38 px (66 px sur une fiche), bordure or 1 px (2 px pour 66 px),
  **niveau** en petit dans un cadre or en bas à droite.
- **Emblème de rang** : losange à double contour or avec l'initiale du rang, 58 px (accueil).

## La note (le signe distinctif)
Cinq étoiles alignées reliées par une fine ligne ; `ceil(note / 20)` étoiles allumées (or), le reste éteint.
Le nombre est écrit à droite. À partir de 90 (lettre S) : étoiles en or clair `#F0C870`.
Ne jamais colorer une note autrement qu'avec ces deux ors (pas de rouge/vert pour les notes).

## Écrans (4 + 2)
1. **Accueil** : mon compte (emblème de rang, LP, victoires, barre, dernière partie avec portrait),
   mes champions (portrait + victoires), cinq dernières parties (V/D, portrait, KDA, note).
2. **Fiche d'un joueur** : en-tête (portrait de son principal, rang, MMR estimé), radar, spécificités (puces),
   ses champions, histogramme des notes des 10 dernières parties, bouton « Copier pour Discord ».
3. **Partie** : deux équipes de 5 ; par ligne : portrait + niveau, pseudo, champion · rôle, rang, victoires,
   KDA, note en étoiles. Ma ligne : fond or à 8 %.
4. **En jeu, petite fenêtre** (overlay natif GDI, pas egui) : état **réduit** (une ligne : cs/min · or · niveau)
   et **ouvert** (290 px : CS/min vs attendu, or contre adversaire direct *estimé*, niveau vs attendu, forme).
   ⚠ « or contre adversaire » = estimation à valider par l'audit de conformité Riot (T24) : ne pas l'activer
   sans feu vert. Aucun timer d'objectif ni de sort ennemi.
5. Champ select, Journal, Réglages : mêmes jetons, aucune nouvelle structure.

## Portraits de champions : contraintes
- Source : icônes officielles Data Dragon (`/cdn/<patch>/img/champion/<Id>.png`, 120 px, ~27 Ko).
- Téléchargement **à la demande**, une fois par patch, stocké dans `%APPDATA%\stat-traque\cache\` ;
  jamais pendant que la fenêtre est fermée ; au plus ~10 icônes visibles en RAM à la fois (réduites à 48 px).
- Coût de toute dépendance de décodage d'image à indiquer dans le commit (taille exe, RAM).

## Défauts de lisibilité connus à corriger au passage (vus sur Windows)
- Texte `.strong()` des cadres Champions et Rôles quasi invisible (fiche.rs:394 et :411).
- Onglet actif : orange sur bleu clair. Barres de rôles : piste blanche.
- « 1 parties » (pluriel) et « — » isolé dans l'en-tête de statut.
- Fenêtre coupée par la barre des tâches (taille par défaut).
