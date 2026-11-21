//! Tous les messages échangés entre les threads de l'app.
//!
//! Architecture : chaque source (client LoL, jeu, Data Dragon, icône) tourne dans
//! son propre thread et ENVOIE des `Evenement` dans un canal. Un seul thread
//! (le "cerveau", voir `cerveau.rs`) les reçoit et décide quoi faire.
//! Quand rien ne se passe, tous les threads dorment : CPU à ~0 %.

use std::sync::Arc;

use crate::champselect::ChampSelect;
use crate::ddragon::DonneesJeu;
use crate::gameflow::Phase;
use crate::live::InstantanePartie;

#[derive(Debug)]
pub enum Evenement {
    /// Le client LoL vient d'être trouvé et le WebSocket est ouvert.
    /// `None` en rejeu (pas de vrai client : on n'applique rien).
    ClientConnecte(Option<crate::lcu::Lcu>),
    /// Le client LoL a été fermé (ou la connexion perdue).
    ClientPerdu,
    /// Changement de phase : Lobby, ChampSelect, InProgress...
    Phase(Phase),
    /// Champ select : mon champion, mon rôle, les deux équipes (voir champselect.rs).
    ChampSelect(ChampSelect),
    /// Données du patch (noms des champions, runes, sorts) prêtes.
    DonneesPretes(Arc<DonneesJeu>),
    /// Nouvel état de la partie en cours (API du jeu, port 2999).
    Partie(InstantanePartie),
    /// Mon compte (lu au client à la connexion) : sert à trouver ma partie en cours.
    /// `summoner_id` : sert aux item sets du client (`/lol-item-sets/v1/item-sets/{id}/sets`).
    Moi { puuid: Box<str>, summoner_id: Option<u64> },
    /// Rangs des joueurs (alliés en champ select non classé, ou les 10 en partie).
    /// `session` : numéro du compte pour lequel la recherche a été lancée (voir
    /// `cerveau::changer_compte`) ; un résultat arrivé après un changement de compte est ignoré.
    Rangs { session: u32, joueurs: Vec<crate::rangs::InfoJoueur> },
    /// Synchro de mon historique terminée (parties, stats perso, LP, récap éventuel).
    /// `Box` : le contenu est gros, l'événement reste petit dans le canal.
    Historique(Box<crate::historique::MajHistorique>),
    /// L'interface demande la fiche d'un joueur (F44). Acceptée seulement pour moi ou
    /// un joueur de ma partie en cours connu par spectator (voir `fiche::autoriser`).
    /// Envoyée par le processus principal quand la fenêtre la demande (ui/parent.rs,
    /// après vérification : puuid présent dans `cibles_fiche`).
    DemandeFiche { puuid: Box<str> },
    /// L'interface demande MA fiche (bouton « Ma fiche ») : la fenêtre ne connaît pas
    /// mon puuid, le cerveau si (`mon_puuid`).
    DemandeMaFiche,
    /// Fiche calculée (fiche.rs). `Box` : contenu gros, l'événement reste petit.
    Fiche(Box<crate::fiche::Fiche>),
    /// La fenêtre principale vient de s'ouvrir ou de se fermer (voir ui/parent.rs) :
    /// le cerveau relance ou arrête le suivi de partie (onglet Partie).
    Fenetre,
    /// Demande d'arrêt (menu "Quitter" de l'icône).
    #[cfg_attr(not(windows), allow(dead_code))]
    Quitter,
}
