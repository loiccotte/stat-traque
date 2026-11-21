# Méthode de travail multi-agents

Les agents sont définis dans `.claude/agents/` ; les règles communes dans `CLAUDE.md`.

## Rôles
| Agent | Couleur (diagramme) | Responsabilité |
|---|---|---|
| `architecte` | violet | découpe, modules, interfaces, dépendances |
| `dev-lcu` | bleu | client LoL local : lockfile, WebSocket, champ select, port 2999 |
| `dev-riot-api` | rouge | Riot API, rate limiter, cache réseau |
| `dev-donnees` | vert | Data Dragon, stockage, stats, builds |
| `dev-ui` | orange | tray, fenêtres, overlay |
| `perf` | jaune | mesures et verdict budget (lecture seule) |
| `qualite` | gris | tests, clippy, conformité Riot, packaging |

## Cycle d'une tâche (un bloc du diagramme)
1. **architecte** précise la tâche : fichiers, interfaces, critère de fin.
2. **dev-xxx** code dans un worktree isolé (les tâches sans dépendance entre elles tournent en parallèle).
3. **perf** mesure → OK ou HORS BUDGET (retour à l'étape 2).
4. **qualite** relit, teste, vérifie la conformité.
5. Commit sur la branche de travail, la tâche passe à « fait » ici.

## Branches
- `claude/sharp-hypatia-pj24j6` : branche d'intégration (toujours compilable, tests verts).
- `feature/<nom>` : une branche par feature, développée par l'agent du domaine dans un
  worktree isolé, partie de la branche d'intégration.
- Avant fusion : revue par l'agent `qualite` (et `perf` si la RAM / la taille bougent),
  corrections, puis fusion dans la branche d'intégration.
- Contrat commun : `src/affichage.rs` (le cerveau remplit, l'interface lit).

## Tâches
| ID | Tâche | Agent | Dépend de | Critère de fin | État |
|---|---|---|---|---|---|
| T01 | Découper `main.rs` en modules (lcu, live, riot_api, ddragon, stockage, ui) | architecte | — | compile, comportement identique | ✅ |
| T02 | Client HTTP léger : `ureq` au lieu de `reqwest`+tokio | dev-lcu | T01 | exe plus petit, RAM mesurée | ✅ |
| T03 | Stockage et réglages dans `%APPDATA%` | dev-donnees | T01 | clé + réglages + dossier cache | ✅ |
| T04 | Cache Data Dragon versionné (champions, runes, items, sorts) | dev-donnees | T02, T03 | 0 requête si patch inchangé | ✅ |
| T05 | WebSocket LCU événementiel (`OnJsonApiEvent`) | dev-lcu | T02 | plus aucun polling LCU | ✅ |
| T06 | Client Riot API : rate limiter, cache, régions | dev-riot-api | T02, T03 | respecte 20/s et 100/2 min | ✅ |
| T07 | Squelette UI : tray + fenêtre à la demande | dev-ui | T01 | < 15 Mo au repos | 🟡 fait, RAM après fermeture à mesurer sous Windows (voir Mesures T07) |
| T08 | Banc de mesure perf (RAM, taille, démarrage) | perf | T01 | script reproductible | ✅ |
| T09 | Machine d'états gameflow (lobby → champ select → partie → fin) | dev-lcu | T05 | transitions loguées | ✅ |
| T10 | Source des builds / runes (**décision à prendre**) | dev-donnees | T04, T06 | format interne défini |  |
| T11 | Import auto des runes | dev-lcu | T09, T10 | 1 page gérée, pas de doublon |  |
| T12 | Sorts d'invocateur auto | dev-lcu | T09, T10 | Flash du bon côté |  |
| T13 | Import des item sets | dev-lcu | T09, T10 | visible dans la boutique |  |
| T14 | Écran champ select | dev-ui | T07, T11 | mode proposer / appliquer | 🟡 affichage fait (compos, picks, runes, matchups, objets) |
| T15 | Rangs des 10 joueurs | dev-riot-api | T06, T09 | affichés en < 2 s |  |
| T16 | Profil rapide : winrate champion, badges | dev-riot-api | T15 | requêtes bornées et en cache |  |
| T17 | Écran partie en cours | dev-ui | T07, T16 | rafraîchi sur événement | ✅ |
| T18 | Overlay en jeu click-through | dev-ui | T17 | < 40 Mo en partie | 🟡 fait, à vérifier en jeu sous Windows |
| T19 | Base locale des parties | dev-donnees | T03, T06 | résumé compact par partie (pas le JSON) | ✅ |
| T20 | Écran post-game | dev-ui | T07, T19 | comparaison à ta moyenne | ✅ (onglet Historique) |
| T21 | Stats perso et suivi LP | dev-donnees | T19 | calculs hors ligne | ✅ |
| T22 | Écran historique / profil | dev-ui | T21 | filtres champion / rôle | 🟡 liste + stats par champion/rôle, filtres à faire |
| T23 | Tests avec réponses LCU / Riot simulées | qualite | T05, T06 | `cargo test` vert | 🟡 15 tests unitaires |
| T24 | Audit conformité Riot | qualite | T13, T18 | checklist validée |  |
| T25 | Packaging : .exe unique, mise à jour, démarrage Windows | qualite | T08, T24 | release GitHub | 🟡 release, mise à jour, démarrage faits ; audit T24 à faire |
| T26 | Fiches matchup OTP : format + import CSV / Google Sheet | dev-donnees | T04 | une fiche Zed importée |  |
| T27 | Détection de l'adversaire de lane en champ select | dev-lcu | T09 | bon adversaire en draft |  |
| T28 | Affichage de la fiche en champ select et overlay | dev-ui | T14, T26, T27 | affichée en < 100 ms |  |

Parallélisable dès le départ après T01 : **T02, T03, T07, T08**.

## Mesures (squelette v0.2, T01–T09)
| Mesure | Avant (v0.1, reqwest) | Après (v0.2) | Budget |
|---|---|---|---|
| Taille .exe Windows | 1,30 Mo | 0,89 Mo | < 8 Mo |
| RAM au repos (mesurée sous Linux) | — | 9 Mo | < 15 Mo |
| RAM pic (champ select + partie simulés) | — | 12 Mo | < 40 Mo |
| Threads au repos | 1 + runtime tokio | 2 (cerveau + client LoL endormis) | — |
| Requêtes au client LoL au repos | 1 / seconde | 0 (WebSocket) | 0 |
| Démarrage jusqu'à prêt | — | < 1 ms (Data Dragon en fond) | < 300 ms |

| Recherche du client LoL fermé | PowerShell à chaque seconde | 1 lecture de fichier / 3 s, liste des processus native | ~0 % CPU |

La CI Windows (`.github/workflows/ci.yml`) relance `scripts/mesure.ps1` à chaque push.

### Revue `perf` + `qualite` (v0.2) : 15 points relevés, corrigés
- **Bloquant** : PowerShell lancé toutes les 30 s client fermé -> remplacé par la liste des processus Windows
  (~1 ms), et seulement si le dossier du jeu est inconnu ; plus de sondage de D:/E:/F:.
- Le suivi de partie survit à la fermeture du client LoL et s'arrête seul après la partie.
- Port 2999 : `playerlist` + `gamestats` au lieu de `allgamedata` ; seulement s'il y a un affichage.
- Identifiants d'objets en u32 (objets à 6 chiffres).
- Cache Riot : ménage au démarrage, écriture atomique, fichier abîmé redemandé, pas d'écriture inutile.
- Limiteur Riot unique pour toute l'app ; Data Dragon : un fichier en RAM à la fois, repli sur l'ancien patch.
- Icône remise si l'Explorateur redémarre ; une seule instance ; pas de boîte « aucun disque ».
- Journal : erreurs répétées écrites une seule fois, attente progressive 3 s -> 30 s.

Pistes plus tard : attendre le lockfile avec `FindFirstChangeNotificationW` (0 réveil),
résumé compact des parties au lieu du JSON match-v5 brut (T19).

## Décisions ouvertes
- **T10 – source des builds** : les fiches OTP (T26) couvrent les builds par matchup pour les champions qui en ont.
  Pour les autres : Blitz agrège des millions de parties sur ses serveurs.
  Options : (a) agréger nous-mêmes via match-v5 sur des joueurs haut classement (lent, limité par la clé dev),
  (b) builds perso + presets maintenus à la main, (c) source publique tierce (vérifier les conditions d'utilisation).
- **T07 – bibliothèque UI** : décidé **egui/eframe**, rendu OpenGL (`glow`), sans options par défaut.
  Si la RAM après fermeture dépasse le budget sous Windows (pilote OpenGL resté chargé),
  plan B : la fenêtre dans un processus enfant (`stat-traque --fenetre`) qui meurt à la fermeture.

## Mesures T07 (fenêtre egui + overlay)
Architecture : un thread « fenetre » créé au démarrage, endormi (Condvar) tant que rien n'est
demandé ; `eframe::run_native` (run_and_return, winit `with_any_thread`) crée fenêtre + contexte
OpenGL à l'ouverture et détruit tout à la fermeture. Une seule fenêtre à la fois (principale OU
overlay). Redessin uniquement sur changement d'état (`affichage::quand_ca_change`) ou action de
la souris ; animations coupées.

| Mesure | Valeur | Budget |
|---|---|---|
| Taille .exe Windows (avant -> après eframe) | 1,09 Mo -> 5,18 Mo (+4,1 Mo) ; 5,47 Mo avec toutes les features fusionnées | < 20 Mo |
| CPU fenêtre ouverte, rien ne change | 0,00 % | ~0 % |
| CPU overlay affiché, rien ne change | 0,00 % | ~0 % |
| RAM privée au repos, avant toute ouverture | 0,7 Mo (Linux) | < 15 Mo |
| RAM privée fenêtre ouverte | 37,8 Mo (Linux, Mesa llvmpipe = rendu logiciel) | — |
| RAM privée après fermeture | 34 Mo (Linux, llvmpipe) : le pilote OpenGL reste chargé | < 15 Mo ⚠ |
| RAM privée overlay | 50,6 Mo (Linux, llvmpipe) | < 40 Mo ⚠ |

Les chiffres Linux viennent de Xvfb + Mesa **llvmpipe** (rendu OpenGL logiciel, très gourmand :
compilateur LLVM en mémoire). egui lui-même pèse ~4 Mo (37,8 ouverte - 34 fermée). **La mesure
qui compte est sous Windows avec le vrai pilote graphique** : `stat-traque essai-fenetre`
(affiche RAM privée / totale et CPU à chaque étape, ~45 s, sans LoL). Si « Fenêtre fermée » ou
« Overlay » dépasse le budget : plan B processus enfant (voir Décisions ouvertes).
