//! Données de DÉMONSTRATION (fausses, mais réalistes) pour voir tous les écrans
//! sans lancer LoL : commande `stat-traque essai-fenetre`. Servent aussi aux tests.

use std::sync::Arc;

use crate::affichage::{EtatAffichage, VueChampSelect, VueRunes};
use crate::compo;
use crate::ddragon::{Champion, DonneesJeu};
use crate::gameflow::Phase;
use crate::live::{InstantanePartie, JoueurLive};
use crate::rangs::{InfoJoueur, Rang};

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

fn donnees() -> DonneesJeu {
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
    EtatAffichage {
        phase: Some(phase),
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
        }),
        rangs,
        partie: Some(InstantanePartie { mode: "CLASSIC".into(), secondes: 14 * 60 + 7, joueurs }),
        messages: (1..=30).map(|i| format!("Message de démonstration n° {i}")).collect(),
        donnees: Some(Arc::new(donnees())),
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
