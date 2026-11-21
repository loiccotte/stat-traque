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
| F04 | Item sets : départ, core, situationnels | 🔜 | P1 | fichier natif du client, rien en overlay | T13 |
| F05 | Détection du rôle assigné | 🔜 | P0 | — | T09 |
| F06 | Ordre de compétences | 🔜 | P1 | — | T14 |
| F07 | Builds perso sauvegardés, prioritaires sur l'auto | 🔜 | P1 | Blitz écrase tes pages | T10 |
| F08 | Mode « proposer » : rien n'est appliqué sans 1 clic | 🔜 | P1 | contrôle total | T14 |
| F09 | Contre-picks / matchup de lane | 💡 | P2 | — | T10 |
| F10 | Tier list par rôle et par patch | 💡 | P2 | — | T10 |
| F11 | Acceptation auto de la partie (option) | ⚠️ | P2 | désactivée par défaut | T09 |

## 2. Chargement et partie en cours
| ID | Feature | Statut | Prio | Mieux que Blitz | Tâche |
|---|---|---|---|---|---|
| F12 | Tableau des 10 joueurs : champion, KDA | ✅ | P0 | — | T17 |
| F13 | Rangs solo / flex des 10 joueurs | 🔜 | P0 | requêtes en cache, affichées en < 2 s | T15 |
| F14 | Winrate du joueur sur son champion, nb de parties | 🔜 | P0 | — | T16 |
| F15 | Badges : main, 1re partie sur le champ, série V/D | 🔜 | P1 | calculés en local | T16 |
| F16 | Items et niveaux en direct | 🔜 | P1 | API locale 2999 seulement en partie | T17 |
| F17 | Overlay click-through, ouvert/fermé par raccourci | 🔜 | P1 | fenêtre transparente, aucune injection | T18 |
| F18 | Détection des duos (premades) | 💡 | P2 | — | T16 |

## 3. Après la partie
| ID | Feature | Statut | Prio | Mieux que Blitz | Tâche |
|---|---|---|---|---|---|
| F19 | Récap : KDA, CS/min, dégâts, vision, or | 🔜 | P0 | — | T20 |
| F20 | Comparaison à **ta** moyenne sur ce champion | 🔜 | P1 | plus parlant qu'une moyenne de rang | T20 |
| F21 | Historique des parties avec filtres | 🔜 | P0 | cache permanent, consultable hors ligne | T22 |
| F22 | Stats perso par champion / rôle / patch | 🔜 | P1 | — | T21 |
| F23 | Suivi des LP et de la progression | 🔜 | P1 | — | T21 |
| F24 | Récap partageable (texte / image) pour les amis | 💡 | P2 | — | T22 |

## 4. Application
| ID | Feature | Statut | Prio | Mieux que Blitz | Tâche |
|---|---|---|---|---|---|
| F25 | Clé API : stockage local, détection d'expiration | ✅ | P0 | jamais dans le dépôt | T06 |
| F26 | Icône tray, fenêtre créée seulement à l'ouverture | 🔜 | P0 | RAM < 15 Mo au repos | T07 |
| F27 | Cache Data Dragon hors ligne, mis à jour par patch | 🔜 | P0 | démarrage instantané | T04 |
| F28 | Multi-région (EUW, EUNE, NA, KR…) | 🔜 | P1 | — | T06 |
| F29 | Multi-comptes (détecte le compte connecté) | 🔜 | P1 | aucun compte stat-traque | T09 |
| F30 | Lancement avec Windows (option) | 🔜 | P1 | — | T25 |
| F31 | Mise à jour automatique légère (GitHub Releases) | 🔜 | P1 | — | T25 |
| F32 | Mode console ultra léger conservé (`--console`) | ✅ | P2 | — | T07 |
| F33 | FR / EN, thème clair / sombre | 💡 | P2 | — | T07 |
| F34 | Zéro pub, zéro télémétrie, zéro compte | ✅ | P0 | c'est la différence principale | — |

## Exclu volontairement (politique Riot)
- Timers de jungle / d'objectifs ou de sorts ennemis calculés.
- Révélation des pseudos alliés en champ select classé.
- Toute action automatique **dans** la partie (macros, scripts).
