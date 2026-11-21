//! Collecte de statistiques : `stat-traque collecter 60` (durée en minutes).
//!
//! Lit des parties classées solo/duo de joueurs Challenger, Grandmaster puis Master,
//! garde UNIQUEMENT celles du patch actuel et compte runes, sorts, matchups de lane,
//! objets légendaires / bottes et bans (stats.rs).
//! Ne garde pas le JSON des parties : seulement les comptes, et la liste des parties
//! déjà vues (pour ne jamais compter deux fois la même).
//!
//! Débit : avec une clé de développement (100 requêtes / 2 min), environ 45 parties
//! par minute. Une clé de production Riot permet beaucoup plus.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::log;
use crate::riot_api::RiotApi;
use crate::stats::{self, Ajout, Stats};

const LIGUES: [&str; 3] = ["challengerleagues", "grandmasterleagues", "masterleagues"];
const PARTIES_PAR_JOUEUR: u8 = 20;

pub fn collecter(minutes: u64) {
    let fin = Instant::now() + Duration::from_secs(minutes * 60);
    let api = RiotApi::depuis_reglages(&crate::stockage::Reglages::charger());
    if let Err(e) = api.verifier_cle() {
        log!("Collecte impossible : {e}");
        return;
    }
    let Some(donnees) = crate::ddragon::charger() else {
        log!("Collecte impossible : version du patch inconnue (Data Dragon).");
        return;
    };
    if donnees.genres_objets.is_empty() {
        // Cache Data Dragon à l'ancien format et pas d'internet : on ne sait pas
        // quels objets sont légendaires. On s'arrête plutôt que de marquer des
        // parties « vues » sans leurs objets (elles ne seraient jamais recomptées).
        log!("Collecte impossible : genres des objets inconnus (cache Data Dragon ancien, connecte-toi à internet).");
        return;
    }
    let patch = stats::patch_court(&donnees.version);
    let chemin = stats::chemin_patch(&patch);
    let chemin_publication = stats::chemin_publication(&patch);
    let chemin_vus = stats::dossier().join(format!("vus-{patch}-{}.txt", api.plateforme()));

    // On reprend là où on s'était arrêté.
    let mut s = Stats::charger_tout(&chemin).unwrap_or_default();
    let mut vus: HashSet<String> =
        std::fs::read_to_string(&chemin_vus).unwrap_or_default().lines().map(String::from).collect();
    // Autre patch, ou fichier d'un ancien format (sans matchups ni objets) : on repart
    // de zéro, liste des parties vues comprise (sinon elles ne seraient jamais recomptées).
    if s.patch != patch || !s.format_actuel() {
        s = Stats::nouveau(&patch);
        vus.clear();
    }
    log!("Collecte patch {patch} sur {} pendant {minutes} min ({} parties déjà comptées).", api.plateforme(), s.nombre_parties_vues());

    let sauver = |s: &Stats, vus: &HashSet<String>| {
        // Fichier de collecte : TOUT (pour continuer à cumuler) ; fichier à publier : allégé.
        let _ = std::fs::write(&chemin, s.vers_texte(1));
        let _ = std::fs::write(&chemin_publication, s.vers_texte_publication());
        let _ = std::fs::write(&chemin_vus, vus.iter().cloned().collect::<Vec<_>>().join("\n"));
    };
    let mut ajoutees = 0u32;
    'ligues: for ligue in LIGUES {
        let Ok(l) = api.ligue_haut_niveau(ligue) else { continue };
        let joueurs: Vec<String> =
            l["entries"].as_array().into_iter().flatten().filter_map(|e| e["puuid"].as_str().map(String::from)).collect();
        log!("{ligue} : {} joueurs", joueurs.len());
        for puuid in joueurs {
            let Ok(ids) = api.ids_parties_classees(&puuid, PARTIES_PAR_JOUEUR) else { continue };
            for id in ids.as_array().into_iter().flatten().filter_map(|i| i.as_str()) {
                if Instant::now() > fin {
                    break 'ligues;
                }
                if !vus.insert(id.to_string()) {
                    continue;
                }
                match api.partie_sans_cache(id) {
                    Ok(p) => match s.ajouter_partie(&p, &|id| donnees.genre_objet(id)) {
                        Ajout::Comptee => {
                            ajoutees += 1;
                            if ajoutees.is_multiple_of(50) {
                                sauver(&s, &vus);
                                log!("{ajoutees} parties ajoutées...");
                            }
                        }
                        // Remake / reddition précoce : partie suivante.
                        Ajout::Ignoree => {}
                        // Partie d'un ancien patch : les suivantes de ce joueur aussi -> joueur suivant.
                        Ajout::AutrePatch => break,
                    },
                    Err(e) => log!("Partie {id} : {e}"),
                }
            }
        }
    }
    sauver(&s, &vus);
    log!("Collecte terminée : {ajoutees} parties ajoutées, {} au total. Fichier : {}", s.nombre_parties_vues(), chemin.display());
    log!("Fichier à publier (allégé) : {}", chemin_publication.display());
}
