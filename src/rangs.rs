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
}

/// Sépare les entrées league-v4 en (solo, flex).
fn lire_rangs(entrees: &Value) -> (Option<Rang>, Option<Rang>) {
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
fn infos(api: &RiotApi, puuid: &str, nom: &str, champion: u16, bleu: bool, moi: bool) -> InfoJoueur {
    let (solo, flex) = api.rangs(puuid).map(|v| lire_rangs(&v)).unwrap_or((None, None));
    let maitrise = if champion == 0 {
        None
    } else {
        api.maitrise(puuid, champion).ok().map(|m| {
            (m["championLevel"].as_u64().unwrap_or(0) as u8, m["championPoints"].as_u64().unwrap_or(0) as u32)
        })
    };
    let meilleur = meilleur_rang(puuid, solo.as_ref());
    InfoJoueur { nom: nom.into(), champion, bleu, moi, solo, flex, meilleur, maitrise }
}

/// Récupère les infos de plusieurs joueurs EN PARALLÈLE (le limiteur global
/// garantit qu'on ne dépasse pas les limites de la clé).
/// `joueurs` : (puuid, nom, champion, bleu, moi)
fn infos_paralleles(api: &RiotApi, joueurs: &[(String, String, u16, bool, bool)]) -> Vec<InfoJoueur> {
    std::thread::scope(|s| {
        let fils: Vec<_> = joueurs
            .iter()
            .map(|(p, n, c, b, m)| s.spawn(move || infos(api, p, n, *c, *b, *m)))
            .collect();
        fils.into_iter().filter_map(|f| f.join().ok()).collect()
    })
}

/// En partie : les 10 joueurs via spectator-v5. Lancé dans un thread à part.
pub fn scouter_partie(api: Arc<RiotApi>, mon_puuid: String, tx: Sender<Evenement>) {
    std::thread::spawn(move || {
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
        let _ = tx.send(Evenement::Rangs(infos_paralleles(&api, &joueurs)));
    });
}

/// Champ select non classé : rangs des alliés dont le client donne le puuid.
pub fn scouter_allies(api: Arc<RiotApi>, allies: Vec<(String, u16, bool)>, tx: Sender<Evenement>) {
    std::thread::spawn(move || {
        let joueurs: Vec<_> = allies.into_iter().map(|(p, c, moi)| (p, String::from("allié"), c, true, moi)).collect();
        let _ = tx.send(Evenement::Rangs(infos_paralleles(&api, &joueurs)));
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
}
