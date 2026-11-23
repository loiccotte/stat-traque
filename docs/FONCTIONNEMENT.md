# Fonctionnement observé (faits vérifiés en vrai)

Notes courtes sur ce que renvoient réellement le client LoL, le jeu et l'API Riot.
Ne contient aucune clé, aucun puuid ni pseudo réel.

## Client LoL (LCU)
- Le fichier `lockfile` du dossier du jeu contient `nom:pid:PORT:TOKEN:https`.
- API locale : `https://127.0.0.1:PORT`, authentification Basic `riot:TOKEN`,
  certificat auto-signé (accepté seulement pour 127.0.0.1).
- Une route qui n'a pas de sens dans l'état actuel répond **404** (ex. champ select hors champ select).
- `/lol-champ-select/v1/current-champion` vaut **0 pendant le simple survol** ; le champion
  survolé est dans `session.actions[][].championId` (action `pick` de ma case, `completed: false`).
- `/lol-gameflow/v1/session` : `gameData.queue.id` (file), `gameData.queue.gameMode`,
  `gameData.teamOne[]` / `teamTwo[]` avec des **puuid courts** (voir plus bas).

## Partie en cours : Live Client Data (port 2999)
- `https://127.0.0.1:2999/liveclientdata/...`, **sans authentification**, seulement pendant la partie.
- Routes : `allgamedata`, `playerlist`, `activeplayer`, `eventdata`, `gamestats`.
- `playerlist` : `scores.creepScore`, `items`, `summonerSpells`, `runes`, `riotIdGameName`…
- `riotId` arrive avec le `#` encodé en `%23`.
- Mode streamer : `riotId` / `riotIdGameName` **vides** → affiché « (pseudo masqué) »,
  jamais recherché (pas de dé-anonymisation).

## puuid : client ≠ API Riot
- Le client donne un puuid de **36 caractères** (format UUID) ; l'API Riot attend celui de
  **78 caractères**. Avec le court, Riot répond `400 Bad Request - Exception decrypting`.
- Conversion (`src/lcu/identite.rs`) : `GET /lol-summoner/v2/summoners/puuid/{puuid_client}`
  → `gameName` + `tagLine`, puis account-v1 `by-riot-id` → puuid Riot (cache 24 h).

## Arena
- Mode `CHERRY`, file **1750** (aussi 1700 / 1710 selon les saisons), 6 équipes de 3, 18 joueurs.
- En direct, les équipes ne sont **pas fiables** : l'API live donne tout le monde `ORDER`
  puis un mélange `ORDER`/`CHAOS` ; spectator-v5 met tout le monde en équipe bleue.
  → stat-traque n'affiche aucune couleur d'équipe en Arena et n'invente pas de trios.
- Source fiable des trios : match-v5 `info.participants[].playerSubteamId` (et
  `subteamPlacement`), **après la partie seulement** ; pas utilisé pour l'instant.

## Clé API Riot (clé de développement)
- Expire au bout de **24 h** ; limites **20 requêtes / s** et **100 requêtes / 2 min**.
- Juste après sa création ou sa régénération, elle peut répondre `401 Unknown apikey`
  pendant quelques minutes : attendre puis réessayer.
