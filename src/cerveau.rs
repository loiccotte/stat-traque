//! Le "cerveau" : reçoit TOUS les événements et décide quoi faire.
//! Un seul thread possède l'état de l'app -> pas de verrous, pas de course.
//!
//! En champ select : analyse des compos, picks conseillés, runes + sorts (stats)
//! appliqués au verrouillage, rangs des alliés si le client les donne.
//! En partie : rangs des 10 joueurs, tableau de la partie (console).

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use crate::champselect::ChampSelect;
use crate::ddragon::DonneesJeu;
use crate::evenement::Evenement;
use crate::gameflow::Phase;
use crate::lcu::Lcu;
use crate::live::{self, InstantanePartie, SuiviPartie};
use crate::rangs::InfoJoueur;
use crate::riot_api::RiotApi;
use crate::{compo, log, stats};

/// Comment l'app tourne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Icône dans la barre des tâches (normal).
    Tray,
    /// Messages et tableau de la partie dans une console.
    Console,
    /// Rejeu d'un enregistrement : console, mais on n'agit pas sur un vrai client.
    Rejeu,
}

/// Ce qu'on a déjà fait pendant ce champ select (pour ne rien faire deux fois).
#[derive(Default)]
struct SuiviChampSelect {
    dernier: Option<ChampSelect>,
    champion_annonce: u16,
    runes_pour: u16,
    compo_affichee: (Vec<u16>, Vec<u16>),
    rangs_demandes: bool,
}

struct Etat {
    phase: Phase,
    mode: Mode,
    donnees: Option<Arc<DonneesJeu>>,
    lcu: Option<Lcu>,
    api: Option<Arc<RiotApi>>,
    mon_puuid: Option<Box<str>>,
    reglages: crate::stockage::Reglages,
    cs: SuiviChampSelect,
    /// Champ select reçu avant que Data Dragon soit prêt : traité après.
    cs_en_attente: Option<ChampSelect>,
    partie_scoutee: bool,
    /// Existe seulement pendant une partie (le lâcher arrête le thread).
    suivi: Option<SuiviPartie>,
}

pub fn boucle(rx: Receiver<Evenement>, tx: Sender<Evenement>, mode: Mode) {
    let reglages = crate::stockage::Reglages::charger();
    // Sans clé API : pas de rangs, mais tout le reste marche.
    let api = (mode != Mode::Rejeu && crate::stockage::lire_cle().is_some())
        .then(|| Arc::new(RiotApi::depuis_reglages(&reglages)));
    let mut etat = Etat {
        phase: Phase::Aucune,
        mode,
        donnees: None,
        lcu: None,
        api,
        mon_puuid: None,
        reglages,
        cs: SuiviChampSelect::default(),
        cs_en_attente: None,
        partie_scoutee: false,
        suivi: None,
    };
    crate::tray::definir_etat("En attente du client LoL");
    // `recv()` bloque sans consommer de CPU jusqu'au prochain événement.
    while let Ok(ev) = rx.recv() {
        match ev {
            Evenement::ClientConnecte(lcu) => {
                log!("Client LoL connecté.");
                etat.lcu = lcu;
                crate::tray::definir_etat("Client LoL connecté");
            }
            Evenement::ClientPerdu => {
                log!("Client LoL fermé, en attente...");
                // On NE coupe PAS le suivi de partie : le jeu continue sans le client
                // (beaucoup ferment le client pendant la partie). Il s'arrête seul à la fin.
                etat.lcu = None;
                crate::tray::definir_etat("En attente du client LoL");
            }
            Evenement::Moi { puuid } => etat.mon_puuid = Some(puuid),
            Evenement::Phase(p) => changer_phase(&mut etat, p, &tx),
            Evenement::ChampSelect(cs) => match etat.donnees.clone() {
                Some(d) => champ_select(&mut etat, &d, cs, &tx),
                None => etat.cs_en_attente = Some(cs),
            },
            Evenement::DonneesPretes(d) => {
                let patch = stats::patch_court(&d.version);
                if stats::trouver_fichier(&patch).is_none() {
                    std::thread::spawn(move || stats::mettre_a_jour(&patch));
                }
                etat.donnees = Some(d.clone());
                if let Some(cs) = etat.cs_en_attente.take() {
                    champ_select(&mut etat, &d, cs, &tx);
                }
            }
            Evenement::Rangs(joueurs) => {
                if let Some(d) = &etat.donnees {
                    afficher_rangs(d, &joueurs);
                }
            }
            Evenement::Partie(p) => {
                if etat.mode != Mode::Tray {
                    afficher_partie(&p);
                }
            }
            Evenement::Quitter => break,
        }
    }
}

fn changer_phase(etat: &mut Etat, p: Phase, tx: &Sender<Evenement>) {
    if p == etat.phase {
        return;
    }
    log!("Phase : {}", p.libelle());
    crate::tray::definir_etat(p.libelle());
    if p == Phase::ChampSelect {
        etat.cs = SuiviChampSelect::default();
        etat.partie_scoutee = false;
    }
    // Rangs des 10 joueurs, une fois par partie.
    if p.en_partie() && !etat.partie_scoutee {
        if let (Some(api), Some(puuid)) = (&etat.api, &etat.mon_puuid) {
            etat.partie_scoutee = true;
            log!("Recherche des rangs des joueurs...");
            crate::rangs::scouter_partie(api.clone(), puuid.to_string(), tx.clone());
        }
    }
    // Le suivi de partie (port 2999) ne tourne QUE pendant la partie, et seulement
    // si quelqu'un s'en sert : la console, l'enregistreur (overlay plus tard).
    // En rejeu, les instantanés viennent du fichier, pas du jeu.
    let utile = etat.mode == Mode::Console || (etat.mode == Mode::Tray && crate::enregistreur::actif());
    let vivant = etat.suivi.as_ref().is_some_and(SuiviPartie::vivant);
    match (p.en_partie() && utile, vivant) {
        (true, false) => etat.suivi = Some(live::suivre(tx.clone())),
        (false, _) if !p.en_partie() => etat.suivi = None,
        _ => {}
    }
    etat.phase = p;
}

/// Id Data Dragon ("MonkeyKing") d'un champion.
fn id_dd(d: &DonneesJeu, champion: u16) -> Option<&str> {
    d.champions.get(&champion).map(|c| &*c.id)
}

fn champ_select(etat: &mut Etat, d: &DonneesJeu, cs: ChampSelect, tx: &Sender<Evenement>) {
    let role = cs.role.as_deref();
    // 1. Mon champion.
    if cs.champion != 0 && cs.champion != etat.cs.champion_annonce {
        etat.cs.champion_annonce = cs.champion;
        let r = role.map(|r| format!(" ({r})")).unwrap_or_default();
        log!("\nChampion : {}{r}", d.nom_champion(cs.champion));
    }

    // 2. Runes + sorts : au verrouillage, une fois par champion.
    if cs.verrouille && cs.champion != 0 && etat.cs.runes_pour != cs.champion {
        etat.cs.runes_pour = cs.champion;
        runes(etat, d, cs.champion, role);
    }

    // 3. Compos (seulement quand un pick change).
    let allies: Vec<u16> = cs.champions_allies().collect();
    let cle = (allies.clone(), cs.ennemis.clone());
    if cle != etat.cs.compo_affichee {
        etat.cs.compo_affichee = cle;
        afficher_compos(d, &allies, &cs.ennemis, role, cs.verrouille);
    }

    // 4. Rangs des alliés : seulement si le client donne leur identité (hors classé).
    if !etat.cs.rangs_demandes {
        if let Some(api) = &etat.api {
            let connus: Vec<(String, u16, bool)> =
                cs.allies.iter().filter_map(|a| a.puuid.as_ref().map(|p| (p.to_string(), a.champion, a.moi))).collect();
            if connus.len() > 1 {
                etat.cs.rangs_demandes = true;
                crate::rangs::scouter_allies(api.clone(), connus, tx.clone());
            }
        }
    }
    etat.cs.dernier = Some(cs);
}

fn afficher_compos(d: &DonneesJeu, allies: &[u16], ennemis: &[u16], role: Option<&str>, verrouille: bool) {
    let nom = |id: &str| d.champions.values().find(|c| &*c.id == id).map_or(id.to_string(), |c| c.nom.to_string());
    let a: Vec<&str> = allies.iter().filter_map(|&c| id_dd(d, c)).collect();
    let e: Vec<&str> = ennemis.iter().filter_map(|&c| id_dd(d, c)).collect();
    log!("--- Compo alliée ({}/5) ---\n{}", a.len(), compo::analyser(&a).texte(&nom));
    if !e.is_empty() {
        log!("--- Compo ennemie ({}/5) ---\n{}", e.len(), compo::analyser(&e).texte(&nom));
        for conseil in compo::comparer(&a, &e) {
            log!("  > {conseil}");
        }
    }
    // Picks conseillés tant que je n'ai pas verrouillé.
    if let (Some(r), false) = (role, verrouille) {
        let s = compo::suggestions(&a, &e, r, 5);
        if !s.is_empty() {
            let liste: Vec<String> = s.iter().map(|x| format!("{} ({})", nom(x.id), x.raison)).collect();
            log!("  Picks qui complètent la compo : {}", liste.join(" · "));
        }
    }
}

fn nom_page(d: &DonneesJeu, page: &stats::PageRunes) -> String {
    let n = |id: u16| d.runes.get(&id).map_or(id.to_string(), |s| s.to_string());
    format!("{} + {} [{}]", n(page.perks[0]), n(page.secondaire), page.perks[1..6].iter().map(|&p| n(p)).collect::<Vec<_>>().join(", "))
}

fn runes(etat: &Etat, d: &DonneesJeu, champion: u16, role: Option<&str>) {
    let patch = stats::patch_court(&d.version);
    let Some(fichier) = stats::trouver_fichier(&patch) else {
        log!("Runes : pas encore de stats pour le patch {patch} (lance `stat-traque collecter 30`).");
        return;
    };
    let Some(r) = stats::recommander_depuis_fichier(&fichier, champion, role) else {
        log!("Runes : pas assez de parties pour {} sur ce patch.", d.nom_champion(champion));
        return;
    };
    let portee = r.role.as_deref().map_or("tous rôles".to_string(), String::from);
    log!(
        "Runes conseillées : {}  — {:.1} % de victoires sur {} parties (choisie dans {:.0} % des parties, {} parties analysées, {portee})",
        nom_page(d, &r.page), r.stats.winrate(), r.stats.parties, r.pick_rate, r.parties_champion
    );
    if r.plus_jouee.0 != r.page {
        log!("  Page la plus jouée : {}  — {:.1} % sur {} parties", nom_page(d, &r.plus_jouee.0), r.plus_jouee.1.winrate(), r.plus_jouee.1.parties);
    }
    let sorts = r.sorts.map(|((a, b), c)| {
        let n = |id: u16| d.sorts.get(&id).map_or(id.to_string(), |s| s.to_string());
        log!("Sorts conseillés : {} + {}  — {:.1} % sur {} parties", n(a), n(b), c.winrate(), c.parties);
        (a, b)
    });

    // Application dans le client (pas en rejeu ; désactivable dans reglages.txt).
    let Some(lcu) = etat.lcu.clone().filter(|_| etat.mode != Mode::Rejeu) else { return };
    let runes_auto = etat.reglages.get("runes") != Some("non");
    let sorts_auto = etat.reglages.get("sorts") != Some("non");
    let flash_sur_f = etat.reglages.get("flash").is_some_and(|f| f.eq_ignore_ascii_case("f"));
    let page = r.page;
    let nom = format!("{} {}", d.nom_champion(champion), r.role.as_deref().unwrap_or(""));
    std::thread::spawn(move || {
        if runes_auto {
            match crate::lcu::actions::appliquer_runes(&lcu, nom.trim(), &page) {
                Ok(()) => log!("Page de runes appliquée."),
                Err(e) => log!("Runes non appliquées : {e}"),
            }
        }
        if let (true, Some(s)) = (sorts_auto, sorts) {
            match crate::lcu::actions::appliquer_sorts(&lcu, s, flash_sur_f) {
                Ok(()) => log!("Sorts appliqués."),
                Err(e) => log!("Sorts non appliqués : {e}"),
            }
        }
    });
}

fn afficher_rangs(d: &DonneesJeu, joueurs: &[InfoJoueur]) {
    let mut joueurs = joueurs.to_vec();
    joueurs.sort_by_key(|j| !j.bleu);
    log!("--- Rangs ---");
    for j in &joueurs {
        let equipe = if j.bleu { "BLEU " } else { "ROUGE" };
        let solo = j.solo.as_ref().map_or("non classé".into(), |r| {
            format!("{r} ({:.0} % sur {}){}", r.winrate(), r.victoires + r.defaites, if r.serie { " en série" } else { "" })
        });
        let flex = j.flex.as_ref().map_or(String::new(), |r| format!(" · flex {r}"));
        let meilleur = match (&j.meilleur, &j.solo) {
            (Some(m), Some(s)) if m.valeur() > s.valeur() => format!(" · meilleur vu {m}"),
            _ => String::new(),
        };
        let maitrise = j.maitrise.map_or(String::new(), |(n, p)| format!(" · maîtrise {n} ({} k pts)", p / 1000));
        let moi = if j.moi { " (toi)" } else { "" };
        log!("{equipe} {:<13} {solo}{flex}{meilleur}{maitrise}  {}{moi}", d.nom_champion(j.champion), j.nom);
    }
}

fn afficher_partie(p: &InstantanePartie) {
    println!("\n--- Partie {} | {}:{:02} ---", p.mode, p.secondes / 60, p.secondes % 60);
    for j in &p.joueurs {
        let equipe = if j.bleu { "BLEU" } else { "ROUGE" };
        println!(
            "{equipe:<6} {:<14} niv {:>2}  {}/{}/{}  {:>3} cs  {}",
            j.champion, j.niveau, j.kills, j.morts, j.assists, j.cs, j.pseudo
        );
    }
}
