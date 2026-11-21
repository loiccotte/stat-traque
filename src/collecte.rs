//! Collecte de statistiques : `stat-traque collecter 60` (durée en minutes).
//!
//! Deux temps, le budget de requêtes (= le temps, la clé dev étant le goulot) est
//! partagé moitié / moitié :
//!  1. Master+ (Challenger, Grandmaster puis Master) : runes, sorts, matchups de lane,
//!     objets légendaires / bottes et bans (stats.rs), + références du palier Master+.
//!  2. Paliers Fer à Diamant (league-v4 `entries/{palier}/{division}?page=N`) : SEULEMENT
//!     les références de la fiche joueur (moyenne / écart-type des mesures par rôle,
//!     voir metriques.rs). Le temps restant est réparti à parts égales entre les 7
//!     paliers. La division tourne d'un joueur à l'autre, la page chaque jour.
//!
//! Seules les parties du patch actuel sont gardées. Pas de JSON gardé : seulement les
//! comptes, et la liste des parties déjà vues (pour ne jamais compter deux fois la même).
//!
//! Débit : avec une clé de développement (100 requêtes / 2 min), environ 45 parties
//! par minute. Sous Master, 1 partie = 10 joueurs pour les références : ~3 parties
//! par joueur échantillonné suffisent (plus de joueurs différents = meilleur échantillon).

use std::collections::HashSet;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::log;
use crate::metriques::{indice_palier, PALIERS_REFERENCE};
use crate::riot_api::RiotApi;
use crate::stats::{self, Ajout, Stats};

const LIGUES: [&str; 3] = ["challengerleagues", "grandmasterleagues", "masterleagues"];
const PARTIES_PAR_JOUEUR: u8 = 20;
/// Parties lues par joueur sous Master (références seulement).
const PARTIES_PAR_JOUEUR_PALIER: u8 = 3;
/// Paliers échantillonnés pour les références (Master+ a déjà les siennes).
const PALIERS_BAS: [&str; 7] = ["IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND"];
const DIVISIONS: [&str; 4] = ["I", "II", "III", "IV"];
/// Part du temps donnée à Master+ (le reste va aux autres paliers).
const PART_MASTER: f64 = 0.5;

/// Que faire d'une partie lue.
#[derive(Clone, Copy)]
enum Usage {
    /// Master+ : tout compter (runes, objets...) + références Master+.
    Complet,
    /// Autre palier : seulement les références de ce palier (indice).
    References(usize),
}

pub fn collecter(minutes: u64) {
    let debut = Instant::now();
    let fin = debut + Duration::from_secs(minutes * 60);
    let fin_master = debut + Duration::from_secs_f64(minutes as f64 * 60.0 * PART_MASTER);
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
    // `traiter` : lit les dernières parties classées d'un joueur et les compte.
    // Renvoie false quand le temps `limite` est écoulé.
    let mut traiter = |s: &mut Stats, vus: &mut HashSet<String>, puuid: &str, nombre: u8, usage: Usage, limite: Instant| {
        let Ok(ids) = api.ids_parties_classees(puuid, nombre) else { return Instant::now() < limite };
        for id in ids.as_array().into_iter().flatten().filter_map(|i| i.as_str()) {
            if Instant::now() > limite {
                return false;
            }
            if !vus.insert(id.to_string()) {
                continue;
            }
            let p = match api.partie_sans_cache(id) {
                Ok(p) => p,
                Err(e) => {
                    log!("Partie {id} : {e}");
                    continue;
                }
            };
            let ajout = match usage {
                Usage::Complet => {
                    let a = s.ajouter_partie(&p, &|id| donnees.genre_objet(id));
                    if a == Ajout::Comptee {
                        s.ajouter_references(&p, PALIERS_REFERENCE.len() - 1);
                    }
                    a
                }
                Usage::References(palier) => s.ajouter_references(&p, palier),
            };
            match ajout {
                Ajout::Comptee => {
                    ajoutees += 1;
                    if ajoutees.is_multiple_of(50) {
                        sauver(s, vus);
                        log!("{ajoutees} parties ajoutées...");
                    }
                }
                // Remake / reddition précoce : partie suivante.
                Ajout::Ignoree => {}
                // Partie d'un ancien patch : les suivantes de ce joueur aussi -> joueur suivant.
                Ajout::AutrePatch => break,
            }
        }
        true
    };

    // 1. Master+ (runes, objets... et références Master+).
    'ligues: for ligue in LIGUES {
        let Ok(l) = api.ligue_haut_niveau(ligue) else { continue };
        let joueurs: Vec<String> =
            l["entries"].as_array().into_iter().flatten().filter_map(|e| e["puuid"].as_str().map(String::from)).collect();
        log!("{ligue} : {} joueurs", joueurs.len());
        for puuid in joueurs {
            if !traiter(&mut s, &mut vus, &puuid, PARTIES_PAR_JOUEUR, Usage::Complet, fin_master) {
                break 'ligues;
            }
        }
    }

    // 2. Paliers sous Master : références seulement, temps restant partagé à parts égales.
    // La page tourne chaque jour ; la DIVISION tourne d'un joueur à l'autre (I, II,
    // III, IV, I...) : chaque palier est représenté en entier, pas par une seule division.
    let jour = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400) as usize;
    let page = 1 + jour % 5;
    for (i, palier) in PALIERS_BAS.iter().enumerate() {
        let reste = fin.saturating_duration_since(Instant::now());
        let limite = Instant::now() + reste / (PALIERS_BAS.len() - i) as u32;
        let Some(indice) = indice_palier(palier) else { continue };
        // Une page par division (4 requêtes, en cache 1 h).
        let listes: Vec<Vec<String>> = DIVISIONS
            .iter()
            .map(|division| {
                let entrees = api.entrees_ligue(palier, division, page as u32).unwrap_or_default();
                entrees.as_array().into_iter().flatten().filter_map(|e| e["puuid"].as_str().map(String::from)).collect()
            })
            .collect();
        let joueurs = alterner(&listes);
        log!("{palier} (page {page}, 4 divisions) : {} joueurs, {} s", joueurs.len(), limite.saturating_duration_since(Instant::now()).as_secs());
        for puuid in joueurs {
            if !traiter(&mut s, &mut vus, &puuid, PARTIES_PAR_JOUEUR_PALIER, Usage::References(indice), limite) {
                break;
            }
        }
    }
    sauver(&s, &vus);
    log!("Collecte terminée : {ajoutees} parties ajoutées, {} au total. Fichier : {}", s.nombre_parties_vues(), chemin.display());
    log!("Fichier à publier (allégé) : {}", chemin_publication.display());
}

/// Joueur k = le (k / 4)-ième de la division k % 4 : I, II, III, IV, I, II...
/// Une division plus courte que les autres est simplement sautée quand elle est vide.
fn alterner(listes: &[Vec<String>]) -> Vec<String> {
    let plus_longue = listes.iter().map(Vec::len).max().unwrap_or(0);
    (0..plus_longue * listes.len())
        .filter_map(|k| listes[k % listes.len()].get(k / listes.len()).cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_division_tourne_par_joueur() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let listes = vec![l(&["a1", "a2"]), l(&["b1"]), l(&[]), l(&["d1", "d2", "d3"])];
        assert_eq!(alterner(&listes), l(&["a1", "b1", "d1", "a2", "d2", "d3"]));
        assert!(alterner(&[]).is_empty());
    }
}
