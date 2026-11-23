//! Données de DÉMONSTRATION (fausses, mais réalistes) pour voir tous les écrans
//! sans lancer LoL : commande `stat-traque essai-fenetre`. Servent aussi aux tests.

use std::sync::Arc;

use crate::affichage::{
    EtatAffichage, VueAxeRadar, VueChampSelect, VueChampionFiche, VueCibleFiche, VueFiche, VueMatchup, VueMmr, VueObjet,
    VueObjets, VueRoleFiche, VueRunes, VueTier, VueTierList,
};
use crate::compo;
use crate::ddragon::{Champion, DonneesJeu};
use crate::gameflow::Phase;
use crate::historique::{FileClassee, Historique, PointLp, ProgressionLp, Recap, ResumePartie};
use crate::live::{InstantanePartie, JoueurLive};
use crate::rangs::{Badge, InfoJoueur, Rang};

/// (id numérique, id Data Dragon, nom affiché)
const CHAMPIONS: [(u16, &str, &str); 10] = [
    (238, "Zed", "Zed"),
    (64, "LeeSin", "Lee Sin"),
    (86, "Garen", "Garen"),
    (222, "Jinx", "Jinx"),
    (89, "Leona", "Leona"),
    (103, "Ahri", "Ahri"),
    (62, "MonkeyKing", "Wukong"),
    (122, "Darius", "Darius"),
    (51, "Caitlyn", "Caitlyn"),
    (117, "Lulu", "Lulu"),
];

/// Noms de champions et d'objets de la démo.
pub fn donnees() -> DonneesJeu {
    let mut d = DonneesJeu { version: "15.1.1".into(), ..Default::default() };
    for (id, dd, nom) in CHAMPIONS {
        d.champions.insert(id, Champion { id: dd.into(), nom: nom.into() });
    }
    for (id, nom) in [(3142, "Lame spectrale de Youmuu"), (6692, "Éclipse"), (3111, "Bottes de Mercure"), (3006, "Jambières du berserker")] {
        d.objets.insert(id, nom.into());
    }
    d
}

fn rang(palier: &str, division: &str, lp: u16, v: u16, d: u16) -> Rang {
    Rang { palier: palier.into(), division: division.into(), lp, victoires: v, defaites: d, serie: v > d + 20 }
}

/// Un état complet : champ select fini + partie en cours avec les 10 rangs.
pub fn etat(phase: Phase) -> EtatAffichage {
    let ids: Vec<&str> = CHAMPIONS.iter().map(|c| c.1).collect();
    let (allies, ennemis) = ids.split_at(5);
    let noms = |l: &[(u16, &str, &str)]| l.iter().map(|c| c.2.to_string()).collect::<Vec<_>>();
    let paliers = ["EMERALD", "DIAMOND", "PLATINUM", "GOLD", "EMERALD", "MASTER", "DIAMOND", "EMERALD", "PLATINUM", "SILVER"];
    let rangs = CHAMPIONS
        .iter()
        .enumerate()
        .map(|(i, (id, _, _))| InfoJoueur {
            nom: format!("Joueur{} #EUW", i + 1).into(),
            champion: *id,
            bleu: i < 5,
            moi: i == 0,
            solo: (i != 9).then(|| rang(paliers[i], if paliers[i] == "MASTER" { "" } else { "II" }, 40 + i as u16 * 5, 60 + i as u16 * 7, 55)),
            flex: (i % 3 == 0).then(|| rang("GOLD", "I", 12, 20, 18)),
            meilleur: (i == 1).then(|| rang("MASTER", "", 80, 0, 0)),
            maitrise: Some((5 + (i % 3) as u8, 30_000 + i as u32 * 41_000)),
            badges: match i {
                0 => vec![Badge::Main],
                3 | 4 => vec![Badge::Groupe(1)],
                6 => vec![Badge::SerieDefaites],
                _ => Vec::new(),
            },
        })
        .collect();
    let joueurs = CHAMPIONS
        .iter()
        .enumerate()
        .map(|(i, c)| JoueurLive {
            pseudo: format!("Joueur{}", i + 1).into(),
            champion: c.2.into(),
            bleu: i < 5,
            niveau: 9 + (i % 4) as u8,
            kills: (i * 3 % 7) as u16,
            morts: (i % 4) as u16,
            assists: (i * 2 % 9) as u16,
            cs: 80 + i as u16 * 11,
            items: vec![3142, 3111, 6692],
        })
        .collect();
    let historique = historique();
    EtatAffichage {
        phase: Some(phase),
        sans_equipes: false,
        client_connecte: true,
        cle_api: true,
        champ_select: Some(VueChampSelect {
            mon_champion: Some("Zed".into()),
            role: Some("middle".into()),
            verrouille: true,
            allies: noms(&CHAMPIONS[..5]),
            ennemis: noms(&CHAMPIONS[5..]),
            analyse_allies: Some(compo::analyser(allies)),
            analyse_ennemis: Some(compo::analyser(ennemis)),
            plan_de_jeu: compo::comparer(allies, ennemis),
            picks_conseilles: vec![("Malphite".into(), "apporte de la frontline".into()), ("Orianna".into(), "dégâts AP".into())],
            runes: Some(VueRunes {
                page: "Électrocution + Inspiration [Impact soudain, Globes oculaires, Chasseur ultime, Livraison de biscuits, Perspicacité cosmique]".into(),
                winrate: 52.4,
                parties: 3400,
                pick_rate: 61.0,
                parties_champion: 5570,
                sorts: Some("Flash + Embrasement — 52,1 % sur 3 100 parties".into()),
                role: Some("MIDDLE".into()),
            }),
            adversaire_lane: Some("Ahri".into()),
            matchup: Some(duel("Ahri", 48.7, 812)),
            contre_picks: vec![duel("Kassadin", 53.1, 640), duel("Fizz", 52.2, 1210)],
            meilleurs_matchups: vec![duel("Lux", 55.0, 900), duel("Xerath", 54.1, 420)],
            pires_matchups: vec![duel("Malzahar", 44.2, 530), duel("Lissandra", 45.9, 380)],
            objets: Some(VueObjets {
                core: vec![objet(3142, "Lame spectrale de Youmuu", 71.0, 52.0), objet(6692, "Éclipse", 64.0, 51.2)],
                bottes: vec![objet(3111, "Bottes de Mercure", 40.0, 50.3)],
                situationnels: vec![objet(3006, "Jambières du berserker", 8.0, 49.0)],
                parties: 5570,
                role: Some("MIDDLE".into()),
                item_set_applique: true,
            }),
        }),
        rangs,
        partie: Some(InstantanePartie { mode: "CLASSIC".into(), secondes: 14 * 60 + 7, joueurs }),
        messages: (1..=30).map(|i| format!("Message de démonstration n° {i}")).collect(),
        donnees: Some(Arc::new(donnees())),
        tier_list: Some(Arc::new(tier_list())),
        historique: Arc::new(historique.parties.clone()),
        stats_perso: Some(Arc::new(historique.stats())),
        progression_lp: Some(Arc::new(progression())),
        recap: Some(Recap::calculer(&historique, &historique.parties[0])),
        // Fiche joueur : la mienne d'abord ; les boutons « Fiche » en demandent d'autres
        // (le processus principal répond avec `fiche()` en mode démo, voir parent.rs).
        fiche: Some(Arc::new(fiche(None))),
        fiche_en_cours: false,
        recap_partage: Some(crate::fiche::recap_discord(&Recap::calculer(&historique, &historique.parties[0]), "Zed", None)),
        cibles_fiche: CHAMPIONS
            .iter()
            .enumerate()
            .map(|(i, c)| VueCibleFiche { puuid: format!("demo-puuid-{i}"), nom: format!("Joueur{} #EUW", i + 1), champion: c.0 })
            .collect(),
    }
}

/// Fiche de démonstration : la mienne (`None`) ou celle d'un joueur de la partie.
/// Valeurs inventées mais variées d'un joueur à l'autre (calculées depuis le champion).
pub fn fiche(cible: Option<&VueCibleFiche>) -> VueFiche {
    let graine = cible.map_or(238, |c| c.champion as u32);
    let varie = |i: u32, base: u32| ((base + graine * (i + 3) * 7) % 70 + 20) as u8;
    let moi = cible.is_none();
    let principal = cible.and_then(|c| CHAMPIONS.iter().find(|x| x.0 == c.champion)).map_or("Zed", |x| x.2);
    let mut v = VueFiche {
        puuid: cible.map_or("demo-puuid-0".into(), |c| c.puuid.clone()),
        nom: cible.map_or("Moi".into(), |c| c.nom.clone()),
        moi,
        rang: Some(if moi { "Émeraude II 45 LP".into() } else { "Diamant IV 12 LP".into() }),
        palier_reference: if moi { "Émeraude".into() } else { "Diamant".into() },
        reference_approximative: !moi,
        radar: crate::fiche::AXES
            .iter()
            .enumerate()
            .map(|(i, a)| VueAxeRadar {
                nom: a.nom().into(),
                // Une case sans donnée chez les autres joueurs (parties sans métriques).
                score: if !moi && i == 4 { None } else { Some(if moi { [68, 74, 41, 55, 62, 47, 81][i] } else { varie(i as u32, 11) }) },
            })
            .collect(),
        mmr: Some(VueMmr {
            rang_equivalent: if moi { "~Émeraude I".into() } else { "~Diamant IV".into() },
            ecart_lp: if moi { 35 } else { -10 },
            confiance: if moi { "moyenne".into() } else { "faible".into() },
            raisons: vec!["gains moyens +22 LP, pertes -17 LP".into(), "adversaires moyens : Émeraude I".into()],
        }),
        champions: [(principal, 9, 6, 2.8), ("Ahri", 4, 2, 3.1), ("Lee Sin", 3, 1, 1.9), ("Jinx", 2, 2, 4.0), ("Lulu", 2, 1, 2.2), ("Garen", 1, 1, 3.0)]
            .iter()
            .enumerate()
            // Pas deux fois le même champion (si le principal est déjà dans la liste).
            .filter(|(i, c)| *i == 0 || c.0 != principal)
            .map(|(_, c)| c)
            .take(5)
            .map(|&(champion, parties, victoires, kda)| VueChampionFiche {
                champion: champion.into(),
                parties,
                victoires,
                winrate: victoires as f32 * 100.0 / parties as f32,
                part: parties as f32 * 100.0 / 20.0,
                kda,
            })
            .collect(),
        roles: vec![
            VueRoleFiche { role: "middle".into(), parties: 14, part: 70.0 },
            VueRoleFiche { role: "jungle".into(), parties: 4, part: 20.0 },
            VueRoleFiche { role: "bottom".into(), parties: 2, part: 10.0 },
        ],
        etiquettes: if moi { vec!["Agressif".into(), "Farmer".into(), "Main Zed".into()] } else { vec!["Vision".into(), "Constant".into()] },
        forme: if moi { "VVDVVDVDVV".into() } else { "DDVDVVDDVD".into() },
        serie: if moi { 2 } else { -1 },
        parties_analysees: 20,
        requetes: if moi { 0 } else { 11 },
        erreur: None,
        texte_partage: String::new(),
    };
    v.texte_partage = crate::fiche::fiche_discord(&v);
    v
}

fn duel(champion: &str, winrate: f32, parties: u32) -> VueMatchup {
    VueMatchup { champion: champion.into(), winrate, parties }
}

fn objet(id: u32, nom: &str, frequence: f32, winrate: f32) -> VueObjet {
    VueObjet { id, nom: nom.into(), frequence, winrate, parties: (frequence * 50.0) as u32 }
}

fn tier_list() -> VueTierList {
    let lignes = [("Garen", "top", 52.8), ("Darius", "top", 51.0), ("Lee Sin", "jungle", 50.4), ("Zed", "middle", 51.9), ("Ahri", "middle", 51.2), ("Jinx", "bottom", 52.0), ("Lulu", "utility", 51.5)]
        .iter()
        .map(|&(champion, role, winrate)| VueTier {
            id: 0,
            champion: champion.into(),
            role,
            winrate,
            pick_rate: 8.5,
            ban_rate: 4.0,
            score: winrate,
            parties: 2400,
        })
        .collect();
    VueTierList { patch: "15.1".into(), parties: 48_000, lignes }
}

/// 12 fausses parties (alternance victoires / défaites, Zed surtout).
fn historique() -> Historique {
    let mut h = Historique::default();
    for i in 0..12u32 {
        h.ajouter(ResumePartie {
            id: format!("EUW1_{}", 7_000_000 + i).into(),
            date: crate::ui::format::maintenant().saturating_sub(1800 + i as u64 * 7200),
            file: if i % 4 == 3 { 450 } else { 420 },
            champion: if i % 3 == 2 { 103 } else { 238 },
            role: (i % 4 != 3).then_some("middle"),
            victoire: i % 3 != 1,
            remake: false,
            kills: 4 + (i % 5) as u16,
            morts: 2 + (i % 3) as u16,
            assists: 3 + (i % 4) as u16,
            cs: 180 + i as u16 * 6,
            duree: 1500 + i * 60,
            degats: 18_000 + i * 900,
            vision: 14 + i as u16,
            or: 11_000 + i * 300,
            patch: "15.1".into(),
            lp: (i % 4 != 3).then_some(if i % 3 != 1 { 21 } else { -18 }),
            metriques: None,
        });
    }
    h
}

fn progression() -> ProgressionLp {
    let points = (0..10u64)
        .map(|i| PointLp { date: 1_763_000_000 + i * 86_400, file: FileClassee::Solo, rang: rang("EMERALD", if i < 5 { "III" } else { "II" }, (i as u16 * 23) % 100, 60, 55) })
        .collect();
    ProgressionLp {
        solo: Some(rang("EMERALD", "II", 45, 65, 55)),
        flex: Some(rang("GOLD", "I", 12, 20, 18)),
        meilleur_solo: Some(rang("DIAMOND", "IV", 10, 0, 0)),
        meilleur_flex: None,
        solo_7_jours: Some(57),
        flex_7_jours: None,
        points,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::format;

    #[test]
    fn la_demo_est_complete() {
        let e = etat(Phase::InProgress);
        assert_eq!(e.rangs.len(), 10);
        let (bleus, rouges) = format::par_equipe(&e.rangs);
        assert_eq!((bleus.len(), rouges.len()), (5, 5));
        let d = e.donnees.as_deref();
        assert_eq!(format::nom_champion(d, 62), "Wukong");
        assert_eq!(format::nom_depuis_id_dd(d, "MonkeyKing"), "Wukong");
        assert_eq!(format::noms_objets(d, &[3142, 1]).len(), 1);
        assert!(e.champ_select.unwrap().analyse_allies.unwrap().nombre > 0);
    }
}
