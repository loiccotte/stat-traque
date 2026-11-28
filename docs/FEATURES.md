# Features de stat-traque

Objectif : tout ce qu'apporte Blitz d'utile, en **10 à 30 fois moins de RAM**, sans pub,
sans compte, sans télémétrie, dans un seul `.exe`.

Statut : ✅ fait · 🔜 prévu · 💡 idée (P2) · ⚠️ à valider (conformité Riot)
Priorité : P0 indispensable · P1 important · P2 bonus

## 1. Champ select
| ID | Feature | Statut | Prio | Mieux que Blitz | Tâche |
|---|---|---|---|---|---|
| F01 | Détection du champion survolé / verrouillé | ✅ | P0 | instantané via événement, pas de polling | T05 |
| F02 | Import auto des runes (champion + rôle) | 🔜 | P0 | une seule page "stat-traque", jamais de doublons | T11 |
| F03 | Sorts d'invocateur auto (Flash côté D/F mémorisé) | 🔜 | P0 | respecte ta touche Flash habituelle | T12 |
| F04 | Item sets : core, bottes, situationnels (objets de fin de partie match-v5) | ✅ | P1 | un seul set « stat-traque <champion> » dans le client, remplacé (jamais de doublon), `objets=non` pour désactiver | T13 |
| F05 | Détection du rôle assigné | ✅ | P0 | — | T09 |
| F06 | ~~Ordre de compétences et objet de départ~~ | ❌ abandonné | — | jugé inutile | — |
| F07 | Builds perso sauvegardés, prioritaires sur l'auto | 🔜 | P1 | Blitz écrase tes pages | T10 |
| F08 | Mode « proposer » : rien n'est appliqué sans 1 clic | 🔜 | P1 | contrôle total | T14 |
| F09 | Contre-picks / matchup de lane (adversaire probable d'après les rôles habituels des ennemis visibles) | ✅ | P2 | lecture filtrée : seules les lignes des champions visibles sont lues (quelques dizaines de Ko) | T10 |
| F10 | Tier list par rôle et par patch : winrate, pick rate, ban rate, score lissé (`stat-traque tierlist [role]`) | ✅ | P2 | calculée en flux, ~40 Ko gardés | T10 |
| F11 | Acceptation auto de la partie (option `accepter=oui`) | ✅ | P2 | désactivée par défaut, dans le client seulement (avant la partie) | T09 |

## 2. Chargement et partie en cours
| ID | Feature | Statut | Prio | Mieux que Blitz | Tâche |
|---|---|---|---|---|---|
| F12 | Tableau des 10 joueurs : champion, KDA, CS, niveau | ✅ | P0 | — | T17 |
| F13 | Rangs solo / flex des 10 joueurs | ✅ | P0 | requêtes en cache, affichées en < 2 s | T15 |
| F14 | Winrate du joueur sur son champion, nb de parties | 🔜 | P0 | — | T16 |
| F15 | Badges : main, 1re partie sur le champ, série V/D | ✅ | P1 | calculés en local (série de défaites : seulement pour toi, base locale) | T16 |
| F16 | Items et niveaux en direct | 🔜 | P1 | API locale 2999 seulement en partie | T17 |
| F17 | Overlay click-through, ouvert/fermé par raccourci | 🟡 | P1 | fenêtre transparente, aucune injection | T18 |
| F18 | Détection des duos (premades) | ✅ | P2 | 1 requête par joueur : ≥ 3 parties communes dont la dernière de l’un, ou ≥ 3 parmi les 5 dernières | T16 |

## 3. Après la partie
| ID | Feature | Statut | Prio | Mieux que Blitz | Tâche |
|---|---|---|---|---|---|
| F19 | Récap : KDA, CS/min, dégâts, vision, or, LP +/- | ✅ | P0 | — | T20 |
| F20 | Comparaison à **ta** moyenne sur ce champion | ✅ | P1 | plus parlant qu'une moyenne de rang | T20 |
| F21 | Historique des parties avec filtres | ✅ | P0 | base locale TSV (~100 octets / partie), hors ligne ; 100 dernières parties au 1er lancement (pas de pagination) ; filtres = interface | T22 |
| F22 | Stats perso par champion / rôle / patch | ✅ | P1 | — | T21 |
| F23 | Suivi des LP et de la progression | ✅ | P1 | LP par partie, changements de division gérés | T21 |
| F24 | Récap partageable (texte prêt à coller sur Discord) pour les amis | ✅ | P1 | markdown court (résultat, KDA, écarts à ta moyenne, radar de la partie en ▰▱) ; `stat-traque partager`, 0 requête ; bouton « Copier » = interface | T30 |

## 3 bis. Fiche joueur
| ID | Feature | Statut | Prio | Mieux que Blitz | Tâche |
|---|---|---|---|---|---|
| F41 | Radar de jeu : combat, farm, vision, objectifs, lane, survie, agressivité, noté par rapport aux joueurs du même rang et du même rôle | ✅ | P1 | comparé à TON palier et TON rôle (références collectées Fer → Master+, repli documenté « référence approximative ») | T29 |
| F42 | MMR estimé (gains/pertes de LP, winrate, parties) — affiché comme une estimation | ✅ | P1 | rang équivalent (« ~Émeraude II ») + écart en LP + niveau de confiance | T29 |
| F43 | Spécificités : champion pool, rôles, style (farmer, roamer, teamfighter, early...), forme récente | ✅ | P1 | étiquettes déduites des métriques challenges match-v5 vs référence | T29 |
| F44 | Fiche des autres joueurs de la partie, à la demande (coût en requêtes borné, cache) | ✅ | P1 | ≤ 11 requêtes (rang + ids + 9 parties en cache permanent), une fiche à la fois, seulement les joueurs connus par spectator ou un Riot ID tapé ; `stat-traque fiche [Pseudo#TAG]` | T29 |


## 4. Savoir des OTP (fiches matchup)
L'idée : les one-tricks (OTP) tiennent des spreadsheets ou des sites (ex : fiches Zed de
Zedking / koala) avec, pour chaque adversaire, la difficulté, les runes, les sorts, l'objet
de départ et les conseils de lane. stat-traque affiche **la bonne fiche au bon moment**,
sans chercher dans un onglet pendant le champ select.

| ID | Feature | Statut | Prio | Mieux que Blitz | Tâche |
|---|---|---|---|---|---|
| F35 | Fiche du matchup affichée dès que l'adversaire de lane est connu | 🔜 | P1 | Blitz n'a que des stats, pas de conseils d'experts | T28 |
| F36 | Import d'un Google Sheet (export CSV) ou d'un CSV, colonnes configurables | 🔜 | P1 | — | T26 |
| F37 | Runes / sorts / objet de départ de la fiche appliqués en 1 clic | 🔜 | P1 | builds par matchup, pas génériques | T26 |
| F38 | Format de fiche ouvert, partageable entre amis, auteur crédité | 🔜 | P1 | — | T26 |
| F39 | Rappel de la fiche (3 conseils max) dans l'overlay au chargement | 💡 | P2 | — | T28 |
| F40 | Mes propres notes de matchup, ajoutées après la partie | 💡 | P2 | — | T22 |

Règles : les fiches sont importées par l'utilisateur lui-même, l'auteur est toujours affiché,
et on demande l'accord de l'auteur avant de distribuer une fiche avec l'app.

## 5. Application
| ID | Feature | Statut | Prio | Mieux que Blitz | Tâche |
|---|---|---|---|---|---|
| F25 | Clé API : stockage local, détection d'expiration | ✅ | P0 | jamais dans le dépôt | T06 |
| F26 | Icône tray (fait), fenêtre créée seulement à l'ouverture | 🟡 | P0 | RAM < 15 Mo au repos | T07 |
| F27 | Cache Data Dragon hors ligne, mis à jour par patch | ✅ | P0 | démarrage instantané | T04 |
| F28 | Multi-région (EUW, EUNE, NA, KR…) : `stat-traque region kr` | ✅ | P1 | — | T06 |
| F29 | Multi-comptes (détecte le compte connecté) | ✅ | P1 | aucun compte stat-traque | T09 |
| F30 | Lancement avec Windows (`stat-traque demarrage oui`) | ✅ | P1 | clé Run de l'utilisateur, aucun service | T25 |
| F31 | Mise à jour automatique légère (GitHub Releases) | ✅ | P1 | 1 vérification / jour, `maj=non` pour couper | T25 |
| F32 | Mode console ultra léger conservé (`--console`) | ✅ | P2 | — | T07 |
| F33 | ~~Version anglaise~~ (app locale, en français) ; thème sombre | ❌ abandonné | — | — | — |
| F34 | Zéro pub, zéro télémétrie, zéro compte | ✅ | P0 | c'est la différence principale | — |

## Exclu volontairement (politique Riot)
- Timers de jungle / d'objectifs ou de sorts ennemis calculés.
- Révélation des pseudos alliés en champ select classé.
- Toute action automatique **dans** la partie (macros, scripts).
