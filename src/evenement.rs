//! Tous les messages échangés entre les threads de l'app.
//!
//! Architecture : chaque source (client LoL, jeu, Data Dragon, icône) tourne dans
//! son propre thread et ENVOIE des `Evenement` dans un canal. Un seul thread
//! (le "cerveau", voir `cerveau.rs`) les reçoit et décide quoi faire.
//! Quand rien ne se passe, tous les threads dorment : CPU à ~0 %.

use std::sync::Arc;

use crate::ddragon::DonneesJeu;
use crate::gameflow::Phase;
use crate::live::InstantanePartie;

#[derive(Debug)]
pub enum Evenement {
    /// Le client LoL vient d'être trouvé et le WebSocket est ouvert.
    ClientConnecte,
    /// Le client LoL a été fermé (ou la connexion perdue).
    ClientPerdu,
    /// Changement de phase : Lobby, ChampSelect, InProgress...
    Phase(Phase),
    /// Champ select : mon champion (survolé ou verrouillé) et mon rôle assigné.
    ChampSelect { champion: u16, role: Option<Box<str>> },
    /// Données du patch (noms des champions, runes, sorts) prêtes.
    DonneesPretes(Arc<DonneesJeu>),
    /// Nouvel état de la partie en cours (API du jeu, port 2999).
    Partie(InstantanePartie),
    /// Demande d'arrêt (menu "Quitter" de l'icône).
    #[cfg_attr(not(windows), allow(dead_code))]
    Quitter,
}
