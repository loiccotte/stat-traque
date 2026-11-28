# stat-traque — règles du projet

Alternative à Blitz pour League of Legends, en Rust, **très optimisée**.
Méthode de travail multi-agents : voir `docs/METHODE.md`. Features : `docs/FEATURES.md`.
Faits observés sur le client, le jeu et l'API Riot : `docs/FONCTIONNEMENT.md`.

## Budgets de performance (non négociables)
Priorité : **RAM et CPU** (fluidité, rien qui ralentit le jeu). La taille de l'exe est secondaire.
| Mesure | Budget | Blitz (ordre de grandeur) |
|---|---|---|
| RAM au repos (tray) | < 15 Mo | 300–600 Mo (Electron) |
| RAM en partie (overlay ouvert) | < 40 Mo | 500 Mo+ |
| CPU au repos | ~0 % (aucun polling actif) | 1–5 % |
| Taille du .exe (secondaire) | < 20 Mo, un seul fichier | ~150 Mo installés |
| Démarrage à froid | < 300 ms jusqu'à l'icône tray | plusieurs secondes |
| Requêtes Riot API | toujours via cache + rate limiter | — |

Une tâche qui dépasse un budget n'est pas terminée.

## Règles de code
- Commentaires en français, pédagogiques (le code doit rester lisible pour un débutant Rust).
- Pas de tokio/async runtime complet sans justification mesurée : préférer threads + canaux.
- Événements plutôt que polling : WebSocket LCU (`OnJsonApiEvent`) dès qu'il existe.
- Toute dépendance ajoutée doit indiquer son coût (taille exe / RAM) dans le commit.
- La clé API ne quitte jamais `%APPDATA%\stat-traque\`.

## Conformité Riot (obligatoire)
- Aucune automatisation d'action en jeu, aucun timer d'objectif/sort ennemi calculé.
- Pas de dé-anonymisation des joueurs en champ select classé.
- Afficher uniquement des infos disponibles via le client et les API officielles.
