//! Écran Partie : deux équipes dans des panneaux, une ligne par joueur
//! (portrait, pseudo, champion, rang, victoires, KDA, note en étoiles).
//! Rendu uniquement sur événement : aucune animation, aucune requête réseau ici.

use eframe::egui::{self, vec2, Align, Color32, Layout, RichText, Ui};

use super::composants::{self, TaillePortrait};
use super::ecrans::vide;
use super::format;
use super::palette;
use super::protocole::Demande;
use super::textures::{self, PortraitsPartage};
use super::theme;
use crate::affichage::{EtatAffichage, VueCibleFiche};
use crate::ddragon::DonneesJeu;
use crate::live::{InstantanePartie, JoueurLive};
use crate::rangs::InfoJoueur;

/// Préfixe des identifiants de la démonstration (voir `demo.rs`).
const PREFIXE_DEMO: &str = "demo-puuid-";

/// Largeurs des colonnes (en points) : pseudo/champion, rang, victoires, KDA.
const L_NOM: f32 = 190.0;
const L_RANG: f32 = 110.0;
const L_VICTOIRES: f32 = 70.0;
const L_KDA: f32 = 110.0;

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
    // Le partage est récupéré une fois ; on ne verrouille que le temps de peindre un portrait.
    let portraits = textures::portraits(ui.ctx());
    if !etat.rangs.is_empty() {
        for (groupe, joueurs) in format::groupes(&etat.rangs, etat.sans_equipes, |j| j.bleu) {
            let couleur = couleur_groupe(groupe);
            composants::panneau(ui, None, |ui| {
                ui.label(RichText::new(groupe.titre()).color(couleur).strong().size(16.0));
                ui.add_space(2.0);
                for j in joueurs {
                    let live = etat.partie.as_ref().and_then(|p| trouver_live(p, donnees, j));
                    demandee |= ligne_joueur(ui, j, live, &etat.cibles_fiche, donnees, portraits.as_ref());
                }
            });
            ui.add_space(8.0);
        }
    } else if let Some(p) = &etat.partie {
        // Pas de rangs (pas de clé API) : on montre quand même la partie en direct.
        composants::panneau(ui, Some(&format!("En direct · {} · {}", p.mode, format::duree(p.secondes))), |ui| {
            for (groupe, joueurs) in format::groupes(&p.joueurs, etat.sans_equipes, |j: &JoueurLive| j.bleu) {
                ui.label(RichText::new(groupe.titre()).color(couleur_groupe(groupe)).strong());
                for j in joueurs {
                    ligne_live(ui, j, donnees, portraits.as_ref());
                }
            }
        });
    }
    demandee
}

/// Couleur du titre d'un groupe (neutre en Arena : pas de couleur d'équipe inventée).
fn couleur_groupe(groupe: format::Groupe) -> Color32 {
    match groupe {
        format::Groupe::Bleue => theme::EQUIPE_BLEUE,
        format::Groupe::Rouge => theme::EQUIPE_ROUGE,
        format::Groupe::Tous => theme::TEXTE,
    }
}

/// Retrouve les chiffres en direct d'un joueur : même équipe, même champion.
fn trouver_live<'a>(p: &'a InstantanePartie, donnees: Option<&DonneesJeu>, j: &InfoJoueur) -> Option<&'a JoueurLive> {
    let nom = format::nom_champion(donnees, j.champion);
    p.joueurs.iter().find(|l| l.bleu == j.bleu && l.champion.eq_ignore_ascii_case(&nom))
}

/// Note à afficher : seulement en démonstration (identifiant « demo-puuid-N »).
/// Hors démo : `None` (étoiles éteintes), jamais de valeur inventée.
fn note_affichee(cible: Option<&VueCibleFiche>) -> Option<u8> {
    let i: usize = cible?.puuid.strip_prefix(PREFIXE_DEMO)?.parse().ok()?;
    super::demo::note_joueur(i)
}

/// Fond or à ~8 % (20/255) de la ligne « moi ».
fn fond_ligne(moi: bool) -> Color32 {
    if moi { theme::avec_alpha(palette::OR, 20) } else { Color32::TRANSPARENT }
}

/// Portrait 38 px ; le verrou est pris seulement pendant le dessin.
fn portrait(ui: &mut Ui, partage: Option<&PortraitsPartage>, donnees: Option<&DonneesJeu>, nom: &str, niveau: Option<u8>) {
    let id = format::id_dd_depuis_nom(donnees, nom);
    match partage.and_then(|p| p.lock().ok()) {
        Some(mut p) => {
            composants::portrait(ui, &mut p, id.as_deref(), niveau, TaillePortrait::Ligne);
        }
        None => {
            ui.allocate_space(vec2(TaillePortrait::Ligne.cote(), TaillePortrait::Ligne.cote()));
        }
    }
}

/// Une cellule de largeur fixe, centrée verticalement.
fn cellule(ui: &mut Ui, largeur: f32, contenu: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(largeur, 38.0), Layout::left_to_right(Align::Center), contenu);
}

/// Une ligne de joueur. Renvoie true si « Fiche » a été cliqué.
fn ligne_joueur(
    ui: &mut Ui,
    j: &InfoJoueur,
    live: Option<&JoueurLive>,
    cibles: &[VueCibleFiche],
    donnees: Option<&DonneesJeu>,
    portraits: Option<&PortraitsPartage>,
) -> bool {
    let mut demandee = false;
    let champion = format::nom_champion(donnees, j.champion);
    let cible = format::cible_du_joueur(cibles, &j.nom, j.champion);
    egui::Frame::NONE.fill(fond_ligne(j.moi)).inner_margin(egui::Margin::symmetric(4, 3)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            portrait(ui, portraits, donnees, &champion, live.map(|l| l.niveau));
            // Pseudo (déjà masqué par le module rangs en mode streamer) + champion.
            cellule(ui, L_NOM, |ui| {
                ui.vertical(|ui| {
                    let pseudo = if j.moi { format!("{} (toi)", j.nom) } else { j.nom.to_string() };
                    ui.label(RichText::new(pseudo).strong().color(if j.moi { theme::OR } else { theme::TEXTE }));
                    ui.label(RichText::new(&champion).small().color(theme::TEXTE_SECONDAIRE));
                });
            });
            cellule(ui, L_RANG, |ui| match &j.solo {
                Some(r) => {
                    ui.label(RichText::new(format::rang_court(Some(r))).color(theme::couleur_palier(&r.palier)));
                }
                None => {
                    ui.label(RichText::new("Non classé").color(theme::TEXTE_SECONDAIRE));
                }
            });
            cellule(ui, L_VICTOIRES, |ui| {
                if let Some(r) = &j.solo {
                    ui.label(format!("{} V", r.victoires));
                }
            });
            cellule(ui, L_KDA, |ui| {
                if let Some(l) = live {
                    ui.label(format::kda(l.kills, l.morts, l.assists));
                }
            });
            cellule(ui, 120.0, |ui| composants::etoiles(ui, note_affichee(cible)));
            // Bouton « Fiche » : moi (base locale) ou un joueur connu par spectator.
            let demande = if j.moi {
                Some(Demande::MaFiche)
            } else {
                cible.map(|c| Demande::Fiche { puuid: c.puuid.clone() })
            };
            if let Some(d) = demande {
                let aide = if j.moi { "Ta fiche : 0 requête Riot" } else { "Radar, MMR estimé, champions : 11 requêtes au plus" };
                if ui.small_button("Fiche").on_hover_text(aide).clicked() {
                    super::enfant::demander(&d);
                    demandee = true;
                }
            }
        });
    });
    demandee
}

/// Ligne sans rang (partie en direct seule) : portrait, pseudo, champion, KDA, CS.
fn ligne_live(ui: &mut Ui, j: &JoueurLive, donnees: Option<&DonneesJeu>, portraits: Option<&PortraitsPartage>) {
    ui.horizontal(|ui| {
        portrait(ui, portraits, donnees, &j.champion, Some(j.niveau));
        cellule(ui, L_NOM, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(&*j.pseudo).strong());
                ui.label(RichText::new(&*j.champion).small().color(theme::TEXTE_SECONDAIRE));
            });
        });
        cellule(ui, L_KDA, |ui| {
            ui.label(format::kda(j.kills, j.morts, j.assists));
        });
        ui.label(RichText::new(format!("{} CS", j.cs)).color(theme::TEXTE_SECONDAIRE));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::demo;

    #[test]
    fn note_seulement_en_demo() {
        let demo_c = VueCibleFiche { puuid: "demo-puuid-1".into(), ..Default::default() };
        assert_eq!(note_affichee(Some(&demo_c)), demo::note_joueur(1));
        let reel = VueCibleFiche { puuid: "abc".into(), ..Default::default() };
        assert_eq!(note_affichee(Some(&reel)), None);
        assert_eq!(note_affichee(None), None);
    }

    #[test]
    fn fond_or_seulement_pour_moi() {
        assert_eq!(fond_ligne(false), Color32::TRANSPARENT);
        assert_ne!(fond_ligne(true), Color32::TRANSPARENT);
    }

    #[test]
    fn live_retrouve_par_equipe_et_champion() {
        let etat = demo::etat(crate::gameflow::Phase::InProgress);
        let d = demo::donnees();
        let p = etat.partie.as_ref().unwrap();
        let moi = etat.rangs.iter().find(|j| j.moi).unwrap();
        let l = trouver_live(p, Some(&d), moi).expect("ma ligne live");
        assert!(l.bleu);
        assert_eq!(l.champion.to_lowercase(), format::nom_champion(Some(&d), moi.champion).to_lowercase());
    }
}
