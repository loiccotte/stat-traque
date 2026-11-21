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

## Tâches
| ID | Tâche | Agent | Dépend de | Critère de fin | État |
|---|---|---|---|---|---|
| T01 | Découper `main.rs` en modules (lcu, live, riot_api, ddragon, stockage, ui) | architecte | — | compile, comportement identique | ✅ |
| T02 | Client HTTP léger : `ureq` au lieu de `reqwest`+tokio | dev-lcu | T01 | exe plus petit, RAM mesurée | ✅ |
| T03 | Stockage et réglages dans `%APPDATA%` | dev-donnees | T01 | clé + réglages + dossier cache | ✅ |
| T04 | Cache Data Dragon versionné (champions, runes, items, sorts) | dev-donnees | T02, T03 | 0 requête si patch inchangé | ✅ |
| T05 | WebSocket LCU événementiel (`OnJsonApiEvent`) | dev-lcu | T02 | plus aucun polling LCU | ✅ |
| T06 | Client Riot API : rate limiter, cache, régions | dev-riot-api | T02, T03 | respecte 20/s et 100/2 min | ✅ |
| T07 | Squelette UI : tray + fenêtre à la demande | dev-ui | T01 | < 15 Mo au repos | 🟡 tray fait, fenêtre à faire |
| T08 | Banc de mesure perf (RAM, taille, démarrage) | perf | T01 | script reproductible | ✅ |
| T09 | Machine d'états gameflow (lobby → champ select → partie → fin) | dev-lcu | T05 | transitions loguées | ✅ |
| T10 | Source des builds / runes (**décision à prendre**) | dev-donnees | T04, T06 | format interne défini |  |
| T11 | Import auto des runes | dev-lcu | T09, T10 | 1 page gérée, pas de doublon |  |
| T12 | Sorts d'invocateur auto | dev-lcu | T09, T10 | Flash du bon côté |  |
| T13 | Import des item sets | dev-lcu | T09, T10 | visible dans la boutique |  |
| T14 | Écran champ select | dev-ui | T07, T11 | mode proposer / appliquer |  |
| T15 | Rangs des 10 joueurs | dev-riot-api | T06, T09 | affichés en < 2 s |  |
| T16 | Profil rapide : winrate champion, badges | dev-riot-api | T15 | requêtes bornées et en cache |  |
| T17 | Écran partie en cours | dev-ui | T07, T16 | rafraîchi sur événement |  |
| T18 | Overlay en jeu click-through | dev-ui | T17 | < 40 Mo en partie |  |
| T19 | Base locale des parties | dev-donnees | T03, T06 | match-v5 en cache permanent |  |
| T20 | Écran post-game | dev-ui | T07, T19 | comparaison à ta moyenne |  |
| T21 | Stats perso et suivi LP | dev-donnees | T19 | calculs hors ligne |  |
| T22 | Écran historique / profil | dev-ui | T21 | filtres champion / rôle |  |
| T23 | Tests avec réponses LCU / Riot simulées | qualite | T05, T06 | `cargo test` vert | 🟡 15 tests unitaires |
| T24 | Audit conformité Riot | qualite | T13, T18 | checklist validée |  |
| T25 | Packaging : .exe unique, mise à jour, démarrage Windows | qualite | T08, T24 | release GitHub |  |
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
- **T07 – bibliothèque UI** : `egui`/`eframe` (simple, ~5 Mo) vs Win32 natif (le plus léger, plus long à écrire).
