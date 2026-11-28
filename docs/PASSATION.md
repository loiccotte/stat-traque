# Passation : travailler sur le PC de test (Windows + LoL)

Ce document sert à l'agent Claude Code qui tourne sur le PC annexe, avec LoL
ouvert (outil d'entraînement). À lire après CLAUDE.md, docs/METHODE.md,
docs/FEATURES.md et docs/FONCTIONNEMENT.md.

## Méthode de travail (inchangée)
- **Branche d'intégration** : `claude/sharp-hypatia-pj24j6`, toujours compilable, tests verts.
- **Une branche par feature** : `feature/<nom>`, partie de la branche d'intégration.
- **Une tâche = un commit**, message en français, avec le **coût RAM/CPU** (budgets de CLAUDE.md)
  et le coût de toute dépendance ajoutée.
- **Agents** (`.claude/agents/`) : `architecte` (découpe), `dev-lcu`, `dev-riot-api`,
  `dev-donnees`, `dev-ui` (développement), `perf` (mesures, lecture seule),
  `qualite` (relecture, lecture seule). Les tâches indépendantes tournent en parallèle
  dans des worktrees isolés.
- **Avant chaque fusion** : `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  puis relecture par l'agent `qualite` (et `perf` si la RAM ou le CPU bougent), corrections,
  nouvelle relecture si les corrections sont importantes, puis fusion `--no-ff`.
- **Jamais dans le dépôt** : clé API, puuid, pseudo réel. Les enregistrements sont anonymisés
  (`J-xxxx`) ; les vérifier avant de les commiter.
- **Conformité Riot** : rien d'automatisé en jeu, aucun timer, aucune dé-anonymisation
  (champ select classé, mode streamer).

## Ce qui n'a JAMAIS tourné sur un vrai Windows / vrai client
Tout a été écrit et testé sous Linux (tests unitaires, rejeu, captures Xvfb). À vérifier :
1. `stat-traque essai-fenetre` : RAM privée du processus de l'icône < 15 Mo à chaque étape,
   plus de processus fenêtre après fermeture, CPU ~0 %.
2. Icône : clic gauche ouvre la fenêtre (2e `stat-traque.exe` dans le Gestionnaire), la croix
   la ferme, plusieurs fois ; fenêtre réduite puis clic = retour au premier plan ; menu clic droit.
3. Overlay (LoL en « Sans bordure ») : Ctrl+Maj+Tab l'affiche / le masque, clics traversants,
   le jeu garde le focus, texte net à 125/150 %, disparaît en fin de partie.
4. Champ select (l'outil d'entraînement en a un) : survol puis verrouillage détectés ;
   page de runes « stat-traque … » créée (pas de doublon), sorts appliqués (Flash côté D/F),
   item set visible en boutique. Il faut un fichier de stats (`collecter`, voir plus bas).
5. Onglet Partie : tableau en direct (port 2999) ; en outil d'entraînement, spectator
   répond 404 (partie personnalisée) : pas de rangs, c'est normal.
6. Onglet Fiche : « Ma fiche » ; « Copier pour Discord » puis Ctrl+V dans un éditeur.
7. Onglet Réglages : Enregistrer modifie `%APPDATA%\stat-traque\reglages.txt` et la valeur
   n'est pas écrasée à la reconnexion du client.
8. `--console` : les accents (« é ») s'affichent correctement.
9. Mise à jour auto, démarrage avec Windows, acceptation auto : pas encore testés.

Rangs, historique, récap, fiche d'un autre joueur et Arena demandent une vraie partie
(normale ou Arena) : spectator et match-v5 ne connaissent pas l'outil d'entraînement.

## Comment tester sans casser
- Compiler : `cargo build --release` (cible MSVC native, pas besoin de mingw sur Windows).
- Mesurer : `powershell -File scripts\mesure.ps1` (LoL fermé) et `stat-traque essai-fenetre`.
- Garder des traces : l'enregistreur note chaque session dans
  `%APPDATA%\stat-traque\enregistrements\` (anonymisé). `scripts\envoyer.ps1` les copie dans
  `enregistrements/` du dépôt ; `cargo test` les rejoue ; `stat-traque --rejouer <fichier>`
  aussi. Relire un fichier avant de le commiter (pas de pseudo, pas de puuid en clair).
- Nouveau fait observé sur le client / le jeu / l'API : l'ajouter à docs/FONCTIONNEMENT.md
  (court, factuel, sans donnée personnelle).
- Ne jamais lire, afficher ni commiter `%APPDATA%\stat-traque\cle.txt`.

## Travail restant (par priorité)
1. Corriger ce que les tests réels ci-dessus révèlent (une branche par problème).
2. `stats.yml` publie le gros fichier cumulatif sous `stats-<patch>.tsv` au lieu de la version
   allégée (`cache\stats\publication\`) ; à corriger avant d'activer la collecte automatique
   (qui demande aussi le secret `RIOT_API_KEY` et le workflow sur la branche par défaut).
3. Tests du cerveau pour `puuid_pour_api` / `MonPuuidRiot` / `apres_identification`
   (injecter une fausse source dans `Etat`).
4. Points mineurs de revue de l'onglet Fiche : anti-rafale global des demandes, délai de la
   démo dans le thread de lecture, propriétaire du presse-papiers, position des graduations.
5. Fiches OTP (F35-F40) : il faut l'export CSV du Google Sheet du propriétaire.
6. Audit de conformité Riot (T24) avant de partager plus largement, puis release v0.3.0.
