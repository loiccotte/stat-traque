# stat-traque

Alternative légère à Blitz pour League of Legends, écrite en Rust.
Objectif : peu de RAM, un seul `.exe`, utilisable par des amis.

Projet personnel, gratuit, sans publicité ni monétisation.

## Fonctions
- Détection du champion sélectionné en champ select (API du client LoL, en local)
- Tableau des joueurs pendant une partie : champions, KDA (API Live Client Data, en local)
- Gestion de la clé API Riot : stockage local, détection d'expiration

## Fonctions prévues
- Runes et sorts automatiques en champ select
- Rangs et historique de parties (API Riot)
- Items et niveaux des joueurs

## Utilisation
```
cargo run                          # lance l'app (le client LoL doit être ouvert)
stat-traque cle RGAPI-xxxxxxxx     # enregistre ta clé API Riot (stockée en local)
```
La clé n'est jamais incluse dans le programme ni dans ce dépôt.

## Mention légale
stat-traque n'est ni approuvé ni affilié à Riot Games et ne reflète pas les opinions de
Riot Games ni de personne impliquée dans la production ou la gestion de League of Legends.
League of Legends et Riot Games sont des marques ou marques déposées de Riot Games, Inc.
Le programme n'automatise aucune action en jeu et n'affiche que des informations déjà
disponibles via le client et les API officielles.
