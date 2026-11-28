//! Rejeu d'un enregistrement (`stat-traque --rejouer fichier.jsonl`).
//! Les lignes enregistrées repassent par EXACTEMENT les mêmes fonctions qu'en
//! direct (`traduire`, `Filtre`, `InstantanePartie::depuis_json`) : ce qui marche
//! en rejeu marche en vrai, et on teste sans lancer LoL.

use std::sync::mpsc::Sender;

use serde_json::Value;

use crate::evenement::Evenement;
use crate::gameflow::Phase;
use crate::lcu::evenements::{lire_champ_select, traduire, Filtre};
use crate::live::InstantanePartie;

/// Transforme le contenu d'un enregistrement en suite d'événements. Fonction pure.
pub fn evenements(contenu: &str) -> Vec<Evenement> {
    let mut filtre = Filtre::default();
    let mut sortie = vec![Evenement::ClientConnecte(None)];
    for ligne in contenu.lines() {
        let Ok(v) = serde_json::from_str::<Value>(ligne) else { continue };
        let ev = match v["src"].as_str() {
            Some("ws") => traduire(&v["msg"]),
            Some("get") => match v["uri"].as_str() {
                Some("/lol-gameflow/v1/gameflow-phase") => v["data"].as_str().map(|p| Evenement::Phase(Phase::depuis_texte(p))),
                Some("/lol-champ-select/v1/session") => lire_champ_select(&v["data"]),
                Some("/lol-summoner/v1/current-summoner") => {
                    // summonerId anonymisé (0) dans les enregistrements -> None.
                    let summoner_id = v["data"]["summonerId"].as_u64().filter(|&id| id != 0);
                    v["data"]["puuid"].as_str().map(|p| Evenement::Moi { puuid: p.into(), summoner_id })
                }
                _ => None,
            },
            Some("live") => InstantanePartie::depuis_json(&v["joueurs"], &v["stats"]).map(Evenement::Partie),
            _ => None,
        };
        if let Some(ev) = ev.and_then(|e| filtre.filtrer(e)) {
            sortie.push(ev);
        }
    }
    sortie.push(Evenement::ClientPerdu);
    sortie
}

/// Envoie les événements du fichier au cerveau, puis demande l'arrêt.
pub fn lancer(chemin: &str, tx: Sender<Evenement>) {
    let contenu = match std::fs::read_to_string(chemin) {
        Ok(c) => c,
        Err(e) => {
            crate::log!("Impossible de lire {chemin} : {e}");
            let _ = tx.send(Evenement::Quitter);
            return;
        }
    };
    std::thread::spawn(move || {
        for ev in evenements(&contenu) {
            if tx.send(ev).is_err() {
                return;
            }
        }
        let _ = tx.send(Evenement::Quitter);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rejoue TOUS les enregistrements du dossier `enregistrements/` du dépôt
    /// (les vraies sessions envoyées avec scripts/envoyer.ps1) : aucune ne doit
    /// planter, et chacune doit produire au moins une phase.
    #[test]
    fn rejoue_les_enregistrements_du_depot() {
        let dossier = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("enregistrements");
        let mut nombre = 0;
        for entree in std::fs::read_dir(dossier).into_iter().flatten().flatten() {
            let chemin = entree.path();
            if chemin.extension().is_some_and(|e| e == "jsonl") {
                let evs = evenements(&std::fs::read_to_string(&chemin).unwrap());
                assert!(evs.iter().any(|e| matches!(e, Evenement::Phase(_))), "aucune phase dans {chemin:?}");
                nombre += 1;
            }
        }
        assert!(nombre > 0, "aucun enregistrement dans enregistrements/");
    }

    /// Multi-comptes : un ami se connecte sur le même PC pendant la session.
    /// Le compte est relu à la connexion (get) PUIS suivi en direct (ws).
    #[test]
    fn changer_de_compte_remet_a_zero() {
        let contenu = r#"{"src":"get","uri":"/lol-summoner/v1/current-summoner","data":{"puuid":"J-aaaa"},"t":0}
{"src":"get","uri":"/lol-gameflow/v1/gameflow-phase","data":"Lobby","t":1}
{"src":"ws","msg":[8,"OnJsonApiEvent",{"uri":"/lol-summoner/v1/current-summoner","eventType":"Update","data":{"puuid":"J-bbbb"}}],"t":50}"#;
        let evs = evenements(contenu);
        let comptes: Vec<&str> = evs
            .iter()
            .filter_map(|e| match e {
                Evenement::Moi { puuid, .. } => Some(&**puuid),
                _ => None,
            })
            .collect();
        assert_eq!(comptes, ["J-aaaa", "J-bbbb"]);

        // Le cerveau doit effacer ce qui appartenait à l'ancien compte (ici : les rangs affichés),
        // mais garder tout si c'est le même compte qui revient.
        let joueur = crate::rangs::InfoJoueur {
            nom: "J-cccc".into(),
            champion: 238,
            bleu: true,
            moi: false,
            solo: None,
            flex: None,
            meilleur: None,
            maitrise: None,
            badges: Vec::new(),
        };
        // `fin` : événements envoyés après le retour du client avec le compte `second`.
        let rangs_apres = |second: &str, fin: Vec<Evenement>| {
            let (tx, rx) = std::sync::mpsc::channel();
            let debut = [
                Evenement::ClientConnecte(None),
                Evenement::Moi { puuid: "J-aaaa".into(), summoner_id: Some(1) },
                Evenement::Rangs { session: 0, joueurs: vec![joueur.clone()] },
                Evenement::ClientPerdu,
                Evenement::ClientConnecte(None),
                Evenement::Moi { puuid: second.into(), summoner_id: Some(2) },
            ];
            for ev in debut.into_iter().chain(fin).chain([Evenement::Quitter]) {
                tx.send(ev).unwrap();
            }
            crate::cerveau::boucle(rx, tx, crate::cerveau::Mode::Rejeu);
            crate::affichage::lire().rangs.len()
        };
        assert_eq!(rangs_apres("J-aaaa", vec![]), 1, "même compte : on garde");
        assert_eq!(rangs_apres("J-bbbb", vec![]), 0, "autre compte : on efface");
        // Résultat d'une recherche lancée pour l'ANCIEN compte, arrivé trop tard : ignoré.
        let perime = Evenement::Rangs { session: 0, joueurs: vec![joueur.clone()] };
        assert_eq!(rangs_apres("J-bbbb", vec![perime]), 0, "résultat périmé ignoré");
        let frais = Evenement::Rangs { session: 1, joueurs: vec![joueur.clone()] };
        assert_eq!(rangs_apres("J-bbbb", vec![frais]), 1, "résultat du nouveau compte affiché");
    }

    #[test]
    fn rejoue_une_session_simple() {
        let contenu = r#"{"src":"get","uri":"/lol-gameflow/v1/gameflow-phase","data":"Lobby","t":0}
{"src":"ws","msg":[8,"OnJsonApiEvent",{"uri":"/lol-gameflow/v1/gameflow-phase","eventType":"Update","data":"ChampSelect"}],"t":10}
{"src":"ws","msg":[8,"OnJsonApiEvent",{"uri":"/lol-champ-select/v1/session","eventType":"Update","data":{"localPlayerCellId":0,"myTeam":[{"cellId":0,"championId":238}]}}],"t":20}
{"src":"ws","msg":[8,"OnJsonApiEvent",{"uri":"/lol-champ-select/v1/session","eventType":"Update","data":{"localPlayerCellId":0,"myTeam":[{"cellId":0,"championId":238}]}}],"t":30}
ligne abîmée
{"src":"live","joueurs":[{"championName":"Zed","team":"ORDER","scores":{}}],"stats":{"gameMode":"CLASSIC","gameTime":61.0},"t":40}"#;
        let evs = evenements(contenu);
        let champs = evs.iter().filter(|e| matches!(e, Evenement::ChampSelect(cs) if cs.champion == 238)).count();
        assert_eq!(champs, 1, "le doublon doit être filtré");
        assert!(evs.iter().any(|e| matches!(e, Evenement::Partie(p) if p.secondes == 61)));
        assert!(matches!(evs.last(), Some(Evenement::ClientPerdu)));
    }
}
