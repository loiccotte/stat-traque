//! Le "cerveau" : reçoit TOUS les événements et décide quoi faire.
//! Un seul thread possède l'état de l'app -> pas de verrous, pas de course.
//!
//! C'est ici qu'on branchera la partie joueur (runes auto, rangs, fiches OTP...).

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use crate::ddragon::DonneesJeu;
use crate::evenement::Evenement;
use crate::gameflow::Phase;
use crate::live::{self, InstantanePartie, SuiviPartie};
use crate::log;

struct Etat {
    phase: Phase,
    donnees: Option<Arc<DonneesJeu>>,
    /// Champion reçu avant que Data Dragon soit prêt : on l'affichera après.
    champion_en_attente: Option<(u16, Option<Box<str>>)>,
    /// Existe seulement pendant une partie (le lâcher arrête le thread).
    suivi: Option<SuiviPartie>,
    /// En mode console on affiche le tableau des joueurs.
    console: bool,
}

pub fn boucle(rx: Receiver<Evenement>, tx: Sender<Evenement>, console: bool) {
    let mut etat = Etat { phase: Phase::Aucune, donnees: None, champion_en_attente: None, suivi: None, console };
    crate::tray::definir_etat("En attente du client LoL");
    // `recv()` bloque sans consommer de CPU jusqu'au prochain événement.
    while let Ok(ev) = rx.recv() {
        match ev {
            Evenement::ClientConnecte => {
                log!("Client LoL connecté.");
                crate::tray::definir_etat("Client LoL connecté");
            }
            Evenement::ClientPerdu => {
                log!("Client LoL fermé, en attente...");
                // On NE coupe PAS le suivi de partie : le jeu continue sans le client
                // (beaucoup ferment le client pendant la partie). Il s'arrête seul à la fin.
                etat.phase = Phase::Aucune;
                crate::tray::definir_etat("En attente du client LoL");
            }
            Evenement::Phase(p) => changer_phase(&mut etat, p, &tx),
            Evenement::ChampSelect { champion, role } => match &etat.donnees {
                Some(d) => annoncer_champion(d, champion, role.as_deref()),
                None => etat.champion_en_attente = Some((champion, role)),
            },
            Evenement::DonneesPretes(d) => {
                if let Some((c, r)) = etat.champion_en_attente.take() {
                    annoncer_champion(&d, c, r.as_deref());
                }
                etat.donnees = Some(d);
            }
            Evenement::Partie(p) => {
                if etat.console {
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
    // Le suivi de partie (port 2999) ne tourne QUE pendant la partie, et seulement
    // si quelqu'un affiche le résultat (console aujourd'hui, overlay plus tard).
    let besoin = p.en_partie() && etat.console;
    let vivant = etat.suivi.as_ref().is_some_and(SuiviPartie::vivant);
    match (besoin, vivant) {
        (true, false) => etat.suivi = Some(live::suivre(tx.clone())),
        (false, _) if !p.en_partie() => etat.suivi = None,
        _ => {}
    }
    etat.phase = p;
}

fn annoncer_champion(d: &DonneesJeu, champion: u16, role: Option<&str>) {
    if champion == 0 {
        return;
    }
    let role = role.map(|r| format!(" ({r})")).unwrap_or_default();
    log!("Champion sélectionné : {}{role}", d.nom_champion(champion));
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
