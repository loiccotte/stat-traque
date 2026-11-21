# stat-traque

Alternative légère à Blitz pour League of Legends, écrite en Rust.
Objectif : peu de RAM, un seul `.exe`, utilisable par des amis.

Projet personnel, gratuit, sans publicité ni monétisation.

Mesures actuelles : exe de 0,9 Mo, ~9 Mo de RAM au repos, 0 requête au client LoL
quand rien ne se passe (le client prévient l'app par WebSocket).

## Fonctions
- Champ select : analyse des deux compos (style, AP/AD, frontline, contrôle, early/late,
  alertes, rôle de chaque champion), plan de jeu, picks qui complètent la compo
- Runes et sorts conseillés avec leurs stats (winrate, nombre de parties), appliqués au verrouillage
- Rangs solo/flex, maîtrise et meilleur rang vu des 10 joueurs en partie (clé API nécessaire)
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
stat-traque collecter 30           # 30 min de collecte de parties Master+ (stats de runes)
stat-traque demarrage oui          # lance stat-traque avec Windows (non = arrêter)
```
Tout est rangé dans `%APPDATA%\stat-traque\` : clé, réglages, cache, journal de la dernière session.

### Réglages
Fichier `%APPDATA%\stat-traque\reglages.txt`, une ligne `nom=valeur` par réglage
(relu au lancement de l'app) :

| Réglage | Effet | Par défaut |
|---|---|---|
| `flash=f` | Flash sur la touche F | D |
| `runes=non` | ne pas changer ma page de runes | appliquées |
| `sorts=non` | ne pas changer mes sorts | appliqués |
| `accepter=oui` | accepter la partie automatiquement (« Partie trouvée », après 1,5 s ; dans le client, jamais en jeu) | **désactivé** |
| `maj=non` | pas de mise à jour automatique | une vérification par jour |
| `demarrage=oui` | lancement avec Windows : utiliser `stat-traque demarrage oui` / `non` (écrit aussi le registre) | non |
| `enregistrer=non` | ne pas enregistrer les sessions | enregistrées |
| `region=kr` | serveur (`stat-traque region kr`) | euw1 |

Changer de compte dans le client LoL est détecté : ce qui concernait l'ancien compte est oublié.

### Mise à jour automatique
Au démarrage, au plus une fois par jour, l'app regarde la dernière release GitHub. Si elle
est plus récente, `stat-traque.exe` est téléchargé à côté de l'exe actuel et remplace
celui-ci ; la nouvelle version est active au prochain lancement (`maj=non` pour désactiver).

## Publier une version
1. Monter `version` dans `Cargo.toml` (ex : `0.3.0`) et commiter.
2. `git tag v0.3.0 && git push origin v0.3.0` (le tag doit être `v` + la version du Cargo.toml).
3. Le workflow `.github/workflows/release.yml` compile sous Windows, teste, vérifie les
   budgets, puis crée la release avec `stat-traque.exe` et `stat-traque.zip`.
   Les apps déjà installées se mettent à jour d'elles-mêmes dans les 24 h.

## Stats de runes automatiques
Le workflow `.github/workflows/stats.yml` collecte chaque jour (~5 h) des parties Master+
et publie `stats-<patch>.tsv` dans la release `stats` (pré-version), que l'app télécharge.
Pour l'activer : ajouter le secret **`RIOT_API_KEY`** dans *Settings > Secrets and variables >
Actions*. Sans secret, le workflow s'arrête proprement avec un avertissement.
Une clé de **développement** expire au bout de 24 h et est très limitée en débit :
demander une clé de **production** sur le portail développeur Riot est recommandé.
Lancement manuel possible (onglet *Actions* > *stats* > *Run workflow*, durée au choix).

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
