//! Rangs des joueurs (alliés et ennemis) via l'API Riot.
//!
//! Quand ?
//!  - En partie (dès le chargement) : `spectator-v5` donne les 10 joueurs avec leur
//!    puuid -> rang solo/flex + maîtrise du champion joué. ~21 requêtes par partie.
//!  - En champ select NON classé : le client donne le puuid des alliés -> leurs rangs.
//!    En classé il le cache : on n'essaie pas de le retrouver (règle Riot).
//!
//! « Rang all time » : l'API Riot ne donne PAS les rangs des saisons passées.
//! On affiche donc la maîtrise (points cumulés depuis toujours sur le champion) et
//! le MEILLEUR rang que stat-traque a déjà vu pour ce joueur (mémorisé en local).
//!
//! Badges (calculés à partir des réponses déjà demandées, sauf les groupes) :
//!  - « main » : beaucoup de points de maîtrise sur le champion joué ;
//!  - « 1re fois sur ce champion » : maîtrise niveau 0 ou 1 ;
//!  - « en série de victoires » : `hotStreak` de league-v4 (3 victoires classées d'affilée) ;
//!  - « en série de défaites » : seulement pour MOI (base locale, 0 requête). Pour les
//!    autres il faudrait télécharger leurs dernières parties (~30 requêtes de plus) : non.
//!  - « groupe N » (duos / premades) : 1 requête de plus par joueur (ids de ses 20
//!    dernières parties) ; deux joueurs de la même équipe qui ont au moins 2 parties
//!    récentes en commun jouent sans doute ensemble.
//!
//! Coût d'une partie : 10 x (rang + maîtrise + ids) + 1 (spectator) = 31 requêtes,
//! toutes en cache (10 min pour les rangs, 6 h maîtrise, 2 min ids) : relancer l'app
//! pendant la partie ne coûte presque rien. La clé dev en permet 100 / 2 min.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::evenement::Evenement;
use crate::riot_api::{ErreurApi, RiotApi};

const PALIERS: [&str; 10] = [
    "IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND", "MASTER", "GRANDMASTER", "CHALLENGER",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rang {
    pub palier: Box<str>,
    /// "I" à "IV" (vide pour Master et plus).
    pub division: Box<str>,
    pub lp: u16,
    pub victoires: u16,
    pub defaites: u16,
    pub serie: bool,
}

impl Rang {
    pub fn depuis_json(e: &Value) -> Option<Self> {
        let nombre = |k: &str| e[k].as_u64().unwrap_or(0) as u16;
        Some(Rang {
            palier: e["tier"].as_str()?.into(),
            division: e["rank"].as_str().unwrap_or("").into(),
            lp: nombre("leaguePoints"),
            victoires: nombre("wins"),
            defaites: nombre("losses"),
            serie: e["hotStreak"] == true,
        })
    }

    /// Valeur pour comparer deux rangs (plus grand = meilleur).
    pub fn valeur(&self) -> u32 {
        let palier = PALIERS.iter().position(|p| **p == *self.palier).unwrap_or(0) as u32;
        let division = match &*self.division {
            "IV" => 0,
            "III" => 1,
            "II" => 2,
            "I" => 3,
            _ => 0,
        };
        palier * 10_000 + division * 1_000 + self.lp as u32
    }

    pub fn winrate(&self) -> f32 {
        let total = self.victoires + self.defaites;
        if total == 0 { 0.0 } else { self.victoires as f32 * 100.0 / total as f32 }
    }
}

impl std::fmt::Display for Rang {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // "Emerald II 45 LP" ; affichage court du palier.
        let p = &self.palier;
        let court = format!("{}{}", &p[..1], p[1..].to_lowercase());
        if self.division.is_empty() {
            write!(f, "{court} {} LP", self.lp)
        } else {
            write!(f, "{court} {} {} LP", self.division, self.lp)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoJoueur {
    pub nom: Box<str>,
    pub champion: u16,
    /// Équipe bleue (100) ?
    pub bleu: bool,
    pub moi: bool,
    pub solo: Option<Rang>,
    pub flex: Option<Rang>,
    /// Meilleur rang solo déjà vu par stat-traque pour ce joueur.
    pub meilleur: Option<Rang>,
    /// (niveau, points) de maîtrise sur le champion joué.
    pub maitrise: Option<(u8, u32)>,
    /// Badges à afficher à côté du joueur (voir `Badge`).
    pub badges: Vec<Badge>,
}

/// Points de maîtrise à partir desquels on considère le joueur comme un « main ».
const POINTS_MAIN: u32 = 100_000;
/// Parties récentes en commun (même équipe) à partir desquelles on parle de groupe.
const PARTIES_COMMUNES_GROUPE: usize = 2;
/// Parties récentes regardées pour détecter les groupes.
const PARTIES_GROUPE: u8 = 20;
/// Victoires / défaites d'affilée (dans MA base locale) pour les badges de série.
const SERIE_MIN: i32 = 3;

/// Petit indice affiché à côté d'un joueur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Main,
    PremiereFois,
    SerieVictoires,
    SerieDefaites,
    /// Joue avec les autres joueurs du même numéro de groupe (duo, trio...).
    Groupe(u8),
}

impl Badge {
    /// Libellé court (sans le numéro pour `Groupe`).
    pub fn libelle(self) -> &'static str {
        match self {
            Badge::Main => "main",
            Badge::PremiereFois => "1re fois sur ce champion",
            Badge::SerieVictoires => "en série de victoires",
            Badge::SerieDefaites => "en série de défaites",
            Badge::Groupe(_) => "groupe",
        }
    }
}

impl std::fmt::Display for Badge {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Badge::Groupe(n) => write!(f, "groupe {n}"),
            b => f.write_str(b.libelle()),
        }
    }
}

/// Badges d'un joueur. `serie_perso` : ma série d'après la base locale (+ victoires,
/// - défaites), seulement pour moi (0 pour les autres).
fn badges_de(j: &InfoJoueur, serie_perso: i32) -> Vec<Badge> {
    let mut b = Vec::new();
    match j.maitrise {
        Some((_, points)) if points >= POINTS_MAIN => b.push(Badge::Main),
        Some((niveau, _)) if niveau <= 1 => b.push(Badge::PremiereFois),
        _ => {}
    }
    let chaud = [&j.solo, &j.flex].into_iter().flatten().any(|r| r.serie);
    if chaud || serie_perso >= SERIE_MIN {
        b.push(Badge::SerieVictoires);
    }
    if serie_perso <= -SERIE_MIN {
        b.push(Badge::SerieDefaites);
    }
    b
}

/// Groupes (duos, premades) : pour chaque joueur, son numéro de groupe ou None.
/// `joueurs[i]` = (équipe bleue ?, ids de ses dernières parties).
/// Deux joueurs de la MÊME équipe avec au moins `PARTIES_COMMUNES_GROUPE` parties en
/// commun sont reliés ; un groupe = des joueurs reliés entre eux (même indirectement).
fn groupes(joueurs: &[(bool, &[String])]) -> Vec<Option<u8>> {
    // `chef[i]` : un représentant du groupe de i (méthode « union-find » simplifiée).
    let mut chef: Vec<usize> = (0..joueurs.len()).collect();
    fn racine(chef: &[usize], mut i: usize) -> usize {
        while chef[i] != i {
            i = chef[i];
        }
        i
    }
    for i in 0..joueurs.len() {
        for j in i + 1..joueurs.len() {
            let (bleu_i, ids_i) = joueurs[i];
            let (bleu_j, ids_j) = joueurs[j];
            if bleu_i != bleu_j {
                continue;
            }
            let communes = ids_i.iter().filter(|id| ids_j.contains(id)).count();
            if communes >= PARTIES_COMMUNES_GROUPE {
                let (a, b) = (racine(&chef, i), racine(&chef, j));
                chef[b] = a;
            }
        }
    }
    // Numérote les groupes d'au moins 2 joueurs : 1, 2, 3... dans l'ordre d'apparition.
    let racines: Vec<usize> = (0..joueurs.len()).map(|i| racine(&chef, i)).collect();
    let mut numeros: Vec<(usize, u8)> = Vec::new();
    racines
        .iter()
        .map(|&r| {
            if racines.iter().filter(|&&x| x == r).count() < 2 {
                return None;
            }
            if let Some(&(_, n)) = numeros.iter().find(|(x, _)| *x == r) {
                return Some(n);
            }
            let n = numeros.len() as u8 + 1;
            numeros.push((r, n));
            Some(n)
        })
        .collect()
}

/// Sépare les entrées league-v4 en (solo, flex).
pub(crate) fn lire_rangs(entrees: &Value) -> (Option<Rang>, Option<Rang>) {
    let mut solo = None;
    let mut flex = None;
    for e in entrees.as_array().into_iter().flatten() {
        match e["queueType"].as_str() {
            Some("RANKED_SOLO_5x5") => solo = Rang::depuis_json(e),
            Some("RANKED_FLEX_SR") => flex = Rang::depuis_json(e),
            _ => {}
        }
    }
    (solo, flex)
}

// ---------- Meilleurs rangs vus (fichier local) ----------
// cache/meilleurs-rangs.tsv : puuid<TAB>palier<TAB>division<TAB>lp

static MEILLEURS: Mutex<Option<HashMap<Box<str>, Rang>>> = Mutex::new(None);

fn chemin_meilleurs() -> std::path::PathBuf {
    crate::stockage::dossier_cache("rangs").join("meilleurs-rangs.tsv")
}

/// Met à jour le meilleur rang connu du joueur et le renvoie.
fn meilleur_rang(puuid: &str, actuel: Option<&Rang>) -> Option<Rang> {
    let mut garde = MEILLEURS.lock().unwrap();
    let table = garde.get_or_insert_with(|| {
        let texte = std::fs::read_to_string(chemin_meilleurs()).unwrap_or_default();
        texte
            .lines()
            .filter_map(|l| {
                let c: Vec<&str> = l.split('\t').collect();
                let rang = Rang {
                    palier: (*c.get(1)?).into(),
                    division: (*c.get(2)?).into(),
                    lp: c.get(3)?.parse().ok()?,
                    victoires: 0,
                    defaites: 0,
                    serie: false,
                };
                Some((Box::from(c[0]), rang))
            })
            .collect()
    });
    if let Some(a) = actuel {
        let mieux = table.get(puuid).is_none_or(|m| a.valeur() > m.valeur());
        if mieux {
            table.insert(puuid.into(), a.clone());
            let texte: String = table
                .iter()
                .map(|(p, r)| format!("{p}\t{}\t{}\t{}\n", r.palier, r.division, r.lp))
                .collect();
            let _ = std::fs::write(chemin_meilleurs(), texte);
        }
    }
    table.get(puuid).cloned()
}

/// Infos d'un joueur : rangs + maîtrise (2 requêtes, en cache ensuite).
/// `avec_parties` : + les ids de ses dernières parties (1 requête), pour les groupes.
fn infos(api: &RiotApi, joueur: &Joueur, avec_parties: bool) -> (InfoJoueur, Vec<String>) {
    let (puuid, nom, champion, bleu, moi) = joueur;
    let (champion, bleu, moi) = (*champion, *bleu, *moi);
    let (solo, flex) = api.rangs(puuid).map(|v| lire_rangs(&v)).unwrap_or((None, None));
    let maitrise = if champion == 0 {
        None
    } else {
        match api.maitrise(puuid, champion) {
            Ok(m) => Some((
                m["championLevel"].as_u64().unwrap_or(0) as u8,
                m["championPoints"].as_u64().unwrap_or(0) as u32,
            )),
            // 404 = aucun point sur ce champion : jamais joué (badge « 1re fois »).
            Err(ErreurApi::Introuvable) => Some((0, 0)),
            Err(_) => None,
        }
    };
    let meilleur = meilleur_rang(puuid, solo.as_ref());
    let mut j = InfoJoueur {
        nom: nom.as_str().into(),
        champion,
        bleu,
        moi,
        solo,
        flex,
        meilleur,
        maitrise,
        badges: Vec::new(),
    };
    // Ma propre série vient de la base locale (0 requête).
    let serie = if moi { crate::historique::serie_locale(puuid) } else { 0 };
    j.badges = badges_de(&j, serie);
    let ids = if avec_parties {
        api.ids_parties(puuid, PARTIES_GROUPE)
            .map(|v| v.as_array().into_iter().flatten().filter_map(|i| i.as_str().map(String::from)).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    (j, ids)
}

/// (puuid, nom, champion, équipe bleue ?, moi ?)
type Joueur = (String, String, u16, bool, bool);

/// Récupère les infos de plusieurs joueurs EN PARALLÈLE (le limiteur global
/// garantit qu'on ne dépasse pas les limites de la clé).
/// `joueurs` : (puuid, nom, champion, bleu, moi)
fn infos_paralleles(api: &RiotApi, joueurs: &[Joueur], avec_parties: bool) -> Vec<(InfoJoueur, Vec<String>)> {
    std::thread::scope(|s| {
        let fils: Vec<_> = joueurs.iter().map(|j| s.spawn(move || infos(api, j, avec_parties))).collect();
        fils.into_iter().filter_map(|f| f.join().ok()).collect()
    })
}

/// En partie : les 10 joueurs via spectator-v5. Lancé dans un thread à part.
pub fn scouter_partie(api: Arc<RiotApi>, mon_puuid: String, tx: Sender<Evenement>) {
    std::thread::spawn(move || {
        let avant = crate::riot_api::Compteurs::maintenant();
        let partie = match api.partie_en_cours(&mon_puuid) {
            Ok(p) => p,
            Err(ErreurApi::Introuvable) => return, // partie personnalisée / entraînement
            Err(e) => {
                crate::log!("Rangs indisponibles : {e}");
                return;
            }
        };
        let joueurs: Vec<_> = partie["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let puuid = p["puuid"].as_str()?.to_string();
                let nom = p["riotId"].as_str().unwrap_or("?").to_string();
                let moi = puuid == mon_puuid;
                Some((puuid, nom, p["championId"].as_u64().unwrap_or(0) as u16, p["teamId"] == 100, moi))
            })
            .collect();
        let resultats = infos_paralleles(&api, &joueurs, true);
        // Groupes (duos / premades) à partir des parties récentes en commun.
        let entrees: Vec<(bool, &[String])> = resultats.iter().map(|(j, ids)| (j.bleu, ids.as_slice())).collect();
        let numeros = groupes(&entrees);
        let infos: Vec<InfoJoueur> = resultats
            .into_iter()
            .zip(numeros)
            .map(|((mut j, _), groupe)| {
                if let Some(n) = groupe {
                    j.badges.push(Badge::Groupe(n));
                }
                j
            })
            .collect();
        crate::log!("Rangs : {} (mesuré pour cette partie).", crate::riot_api::Compteurs::depuis(avant));
        let _ = tx.send(Evenement::Rangs(infos));
    });
}

/// Champ select non classé : rangs des alliés dont le client donne le puuid.
pub fn scouter_allies(api: Arc<RiotApi>, allies: Vec<(String, u16, bool)>, tx: Sender<Evenement>) {
    std::thread::spawn(move || {
        let joueurs: Vec<_> = allies.into_iter().map(|(p, c, moi)| (p, String::from("allié"), c, true, moi)).collect();
        let infos = infos_paralleles(&api, &joueurs, false).into_iter().map(|(j, _)| j).collect();
        let _ = tx.send(Evenement::Rangs(infos));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_et_compare_les_rangs() {
        let v: Value = serde_json::from_str(
            r#"[{"queueType":"RANKED_FLEX_SR","tier":"GOLD","rank":"I","leaguePoints":10,"wins":5,"losses":5},
                {"queueType":"RANKED_SOLO_5x5","tier":"EMERALD","rank":"II","leaguePoints":45,"wins":60,"losses":40,"hotStreak":true}]"#,
        )
        .unwrap();
        let (solo, flex) = lire_rangs(&v);
        let solo = solo.unwrap();
        assert_eq!(solo.to_string(), "Emerald II 45 LP");
        assert_eq!(solo.winrate(), 60.0);
        assert!(solo.serie);
        assert!(solo.valeur() > flex.unwrap().valeur());
        let master = Rang { palier: "MASTER".into(), division: "I".into(), lp: 0, victoires: 0, defaites: 0, serie: false };
        assert!(master.valeur() > solo.valeur());
    }

    fn joueur(maitrise: Option<(u8, u32)>, serie: bool) -> InfoJoueur {
        let solo = Rang { palier: "GOLD".into(), division: "II".into(), lp: 0, victoires: 1, defaites: 1, serie };
        InfoJoueur {
            nom: "x".into(),
            champion: 1,
            bleu: true,
            moi: false,
            solo: Some(solo),
            flex: None,
            meilleur: None,
            maitrise,
            badges: Vec::new(),
        }
    }

    #[test]
    fn badges_main_premiere_fois_et_series() {
        assert_eq!(badges_de(&joueur(Some((12, 350_000)), true), 0), vec![Badge::Main, Badge::SerieVictoires]);
        assert_eq!(badges_de(&joueur(Some((0, 0)), false), 0), vec![Badge::PremiereFois]);
        assert_eq!(badges_de(&joueur(Some((5, 30_000)), false), 0), vec![]);
        // Maîtrise inconnue (erreur réseau) : aucun badge de maîtrise.
        assert_eq!(badges_de(&joueur(None, false), -4), vec![Badge::SerieDefaites]);
        assert_eq!(badges_de(&joueur(None, false), 3), vec![Badge::SerieVictoires]);
        assert_eq!(Badge::Groupe(2).to_string(), "groupe 2");
    }

    #[test]
    fn detecte_les_groupes_de_la_meme_equipe() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let a = ids(&["m1", "m2", "m3"]);
        let b = ids(&["m2", "m3", "m9"]); // 2 parties avec A : duo
        let c = ids(&["m3", "m7"]); // 1 seule en commun : pas assez
        let d = ids(&["m2", "m3"]); // équipe adverse : ignoré
        let e = ids(&["x1", "x2"]);
        let f = ids(&["x1", "x2", "x3"]);
        let entrees: Vec<(bool, &[String])> =
            vec![(true, &a), (true, &b), (true, &c), (false, &d), (false, &e), (false, &f)];
        assert_eq!(groupes(&entrees), vec![Some(1), Some(1), None, None, Some(2), Some(2)]);
        assert_eq!(groupes(&[]), Vec::<Option<u8>>::new());
    }
}
