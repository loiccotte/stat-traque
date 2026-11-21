# stat-traque

Alternative légère à Blitz pour League of Legends, écrite en Rust.
Objectif : peu de RAM, un seul `.exe`, utilisable par des amis.

Projet personnel, gratuit, sans publicité ni monétisation.

Mesures actuelles : exe de 0,9 Mo, ~9 Mo de RAM au repos, 0 requête au client LoL
quand rien ne se passe (le client prévient l'app par WebSocket).

## Fonctions
- Icône dans la barre des tâches (pas de fenêtre console)
- Suivi des phases du client (salon, champ select, partie, fin) par événements
- Détection du champion survolé / verrouillé et du rôle assigné en champ select
- Tableau des joueurs pendant une partie : champion, niveau, KDA, CS, objets
- Données du patch (Data Dragon) en cache : fonctionne hors ligne
- Client API Riot avec limiteur de débit et cache (prêt pour les rangs et l'historique)
- Gestion de la clé API Riot : stockage local, détection d'expiration

Liste complète et feuille de route : [docs/FEATURES.md](docs/FEATURES.md),
méthode de travail : [docs/METHODE.md](docs/METHODE.md).

## Utilisation
```
stat-traque                        # lance l'app (icône près de l'horloge)
stat-traque --console              # lance l'app avec les messages dans une console
stat-traque cle RGAPI-xxxxxxxx     # enregistre ta clé API Riot (stockée en local)
stat-traque region euw1            # change de serveur (euw1, eun1, na1, kr...)
```
Tout est rangé dans `%APPDATA%\stat-traque\` : clé, réglages, cache, journal de la dernière session.

### Tester sans effort
L'app enregistre chaque session (pseudos anonymisés, 5 sessions max) dans
`%APPDATA%\stat-traque\enregistrements\`. Joue normalement, puis de temps en temps :
`powershell -File scripts\envoyer.ps1` (copie + commit + push). Les sessions envoyées
sont rejouées par `cargo test`. Désactiver : `enregistrer=non` dans `reglages.txt`.

Développement : `cargo build --release`, puis `powershell -File scripts\mesure.ps1` pour
vérifier les budgets de performance (taille, RAM, CPU, démarrage).
La clé n'est jamais incluse dans le programme ni dans ce dépôt.

## Mention légale
stat-traque n'est ni approuvé ni affilié à Riot Games et ne reflète pas les opinions de
Riot Games ni de personne impliquée dans la production ou la gestion de League of Legends.
League of Legends et Riot Games sont des marques ou marques déposées de Riot Games, Inc.
Le programme n'automatise aucune action en jeu et n'affiche que des informations déjà
disponibles via le client et les API officielles.
