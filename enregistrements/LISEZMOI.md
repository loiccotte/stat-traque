# Enregistrements de sessions

Sessions réelles (anonymisées : pseudos remplacés par `J-xxxx`) envoyées avec
`scripts\envoyer.ps1`. `cargo test` les rejoue toutes : chaque correction est
vérifiée sur de vraies parties, sans relancer LoL.

Rejouer une session à la main :  `stat-traque --rejouer enregistrements\session-XXXX.jsonl`

`exemple-faux-client.jsonl` vient d'un faux client LoL (test de base).
