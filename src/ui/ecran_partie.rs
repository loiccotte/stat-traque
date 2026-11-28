//! Écran Partie : rangs des joueurs et tableau « en direct ».
//! (Code déplacé tel quel depuis ecrans.rs.)

use eframe::egui::{self, RichText, Ui};

use super::ecrans::vide;
use super::format;
use super::protocole::Demande;
use super::theme;
use crate::affichage::{EtatAffichage, VueCibleFiche};
use crate::ddragon::DonneesJeu;
use crate::live::{InstantanePartie, JoueurLive};
use crate::rangs::InfoJoueur;

/// Onglet Partie. Renvoie true si le joueur a demandé une fiche (bouton « Fiche »).
pub(super) fn dessiner_partie(ui: &mut Ui, etat: &EtatAffichage, donnees: Option<&DonneesJeu>) -> bool {
    let mut demandee = false;
    if etat.rangs.is_empty() && etat.partie.is_none() {
        let texte = if etat.cle_api {
            "Pas de partie en cours. Les rangs des 10 joueurs s'afficheront ici au lancement de la partie."
        } else {
            "Pas de partie en cours. Pour voir les rangs, enregistre ta clé API : stat-traque cle RGAPI-..."
        };
        vide(ui, texte);
        return false;
    }
    if !etat.rangs.is_empty() {
        for (groupe, joueurs) in format::groupes(&etat.rangs, etat.sans_equipes, |j| j.bleu) {
            let (titre, couleur) = (groupe.titre(), couleur_groupe(groupe));
            theme::carte().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(titre).color(couleur).strong().size(16.0));
                demandee |= tableau_rangs(ui, titre, &joueurs, &etat.cibles_fiche, donnees);
            });
            ui.add_space(8.0);
        }
    }
    if let Some(p) = &etat.partie {
        theme::carte().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            theme::titre(ui, &format!("En direct · {} · {}", p.mode, format::duree(p.secondes)));
            tableau_live(ui, p, donnees, etat.sans_equipes);
        });
    }
    demandee
}

/// Tableau des rangs d'une équipe. Renvoie true si une fiche a été demandée.
fn tableau_rangs(ui: &mut Ui, id: &str, joueurs: &[&InfoJoueur], cibles: &[VueCibleFiche], donnees: Option<&DonneesJeu>) -> bool {
    let mut demandee = false;
    egui::Grid::new(("rangs", id)).num_columns(9).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
        for entete in ["Champion", "Joueur", "Solo", "Bilan solo", "Flex", "Maîtrise", "Meilleur vu", "Badges", ""] {
            ui.label(RichText::new(entete).color(theme::TEXTE_DOUX).small());
        }
        ui.end_row();
        for j in joueurs {
            let champion = RichText::new(format::nom_champion(donnees, j.champion)).strong();
            ui.label(if j.moi { champion.color(theme::OR) } else { champion });
            ui.label(if j.moi { format!("{} (toi)", j.nom) } else { j.nom.to_string() });
            match &j.solo {
                Some(r) => {
                    let texte = if r.serie { format!("{r}  série") } else { r.to_string() };
                    ui.label(RichText::new(texte).color(theme::couleur_palier(&r.palier)));
                    ui.label(format::bilan(r));
                }
                None => {
                    ui.label(RichText::new("Non classé").color(theme::TEXTE_DOUX));
                    ui.label("");
                }
            }
            ui.label(j.flex.as_ref().map_or(String::new(), |r| r.to_string()));
            ui.label(format::maitrise(j.maitrise));
            ui.label(format::meilleur_vu(j).unwrap_or_default());
            let badges: Vec<String> = j.badges.iter().map(|b| b.to_string()).collect();
            ui.label(RichText::new(badges.join(" · ")).color(theme::TURQUOISE).small());
            // Bouton « Fiche » : moi (base locale), ou un joueur connu par spectator.
            // Pas de bouton pour un joueur inconnu (le parent refuserait de toute façon).
            let demande = if j.moi {
                Some(Demande::MaFiche)
            } else {
                format::cible_du_joueur(cibles, &j.nom, j.champion).map(|c| Demande::Fiche { puuid: c.puuid.clone() })
            };
            match demande {
                Some(d) => {
                    let aide = if j.moi { "Ta fiche : 0 requête Riot" } else { "Radar, MMR estimé, champions : 11 requêtes au plus" };
                    if ui.small_button("Fiche").on_hover_text(aide).clicked() {
                        super::enfant::demander(&d);
                        demandee = true;
                    }
                }
                None => {
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
    demandee
}

/// Couleur du titre / des champions d'un groupe (neutre en Arena).
fn couleur_groupe(groupe: format::Groupe) -> egui::Color32 {
    match groupe {
        format::Groupe::Bleue => theme::EQUIPE_BLEUE,
        format::Groupe::Rouge => theme::EQUIPE_ROUGE,
        format::Groupe::Tous => theme::TEXTE,
    }
}

fn tableau_live(ui: &mut Ui, p: &InstantanePartie, donnees: Option<&DonneesJeu>, sans_equipes: bool) {
    let groupes = format::groupes(&p.joueurs, sans_equipes, |j: &JoueurLive| j.bleu);
    egui::Grid::new("live").num_columns(6).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
        for entete in ["Champion", "Joueur", "Niv.", "KDA", "CS", "Objets"] {
            ui.label(RichText::new(entete).color(theme::TEXTE_DOUX).small());
        }
        ui.end_row();
        for (groupe, joueurs) in groupes {
            let couleur = couleur_groupe(groupe);
            for j in joueurs {
                ui.label(RichText::new(&*j.champion).color(couleur).strong());
                ui.label(&*j.pseudo);
                ui.label(j.niveau.to_string());
                ui.label(format!("{}  ({})", format::kda(j.kills, j.morts, j.assists), format::ratio_kda(j.kills, j.morts, j.assists)));
                ui.label(j.cs.to_string());
                ui.label(RichText::new(format::noms_objets(donnees, &j.items).join(", ")).small());
                ui.end_row();
            }
        }
    });
}
