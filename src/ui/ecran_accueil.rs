//! Écran Accueil : mon compte, mes champions, cinq dernières parties.
//!
//! Tout vient de l'état déjà reçu (historique, rang, stats perso) : aucune requête
//! réseau, aucune animation, rien ne redessine tant que l'état ne change pas.

use std::sync::MutexGuard;

use eframe::egui::{self, RichText, Ui};

use super::composants::{self, TaillePortrait};
use super::format;
use super::textures::{self, Portraits};
use super::theme;
use crate::affichage::EtatAffichage;
use crate::ddragon::DonneesJeu;
use crate::historique::{ResumePartie, StatsGroupe, StatsPerso};
use crate::rangs::Rang;

/// Nombre de champions montrés dans « Mes champions ».
const NB_CHAMPIONS: usize = 3;
/// Nombre de parties montrées dans « Cinq dernières parties ».
const NB_PARTIES: usize = 5;

// =====================================================================
// Fonctions pures (testées en bas du fichier)
// =====================================================================

/// Fraction de la barre de LP (0 à 1). `None` pour Master et plus : pas de division,
/// les LP ne sont plus bornés à 100, une barre n'aurait pas de sens.
pub(super) fn fraction_lp(r: &Rang) -> Option<f32> {
    if r.division.is_empty() {
        None
    } else {
        Some((r.lp.min(100) as f32) / 100.0)
    }
}

/// Texte du rang : « Gold II · 45 LP » ou « Master · 312 LP ».
pub(super) fn libelle_rang(r: &Rang) -> String {
    if r.division.is_empty() {
        format!("{} · {}\u{a0}LP", r.palier, r.lp)
    } else {
        format!("{} {} · {}\u{a0}LP", r.palier, r.division, r.lp)
    }
}

/// Texte des victoires/défaites : « 12 victoires · 9 défaites ».
pub(super) fn libelle_bilan(r: &Rang) -> String {
    format!("{} · {}", format::pluriel(r.victoires as u32, "victoire"), format::pluriel(r.defaites as u32, "défaite"))
}

/// Rang à montrer : solo/duo d'abord, sinon flex. Renvoie aussi le nom de la file.
pub(super) fn rang_a_montrer<'a>(solo: Option<&'a Rang>, flex: Option<&'a Rang>) -> Option<(&'a Rang, &'static str)> {
    solo.map(|r| (r, "Solo/Duo")).or_else(|| flex.map(|r| (r, "Flex")))
}

/// Les `n` champions les plus joués : (id champion, victoires, parties).
/// `par_champion` est déjà trié par nombre de parties décroissant.
pub(super) fn meilleurs_champions(stats: &StatsPerso, n: usize) -> Vec<(u16, u32, u32)> {
    stats.par_champion.iter().take(n).map(|(id, g): &(u16, StatsGroupe)| (*id, g.victoires, g.parties)).collect()
}

/// Les `n` dernières parties jouées (remakes exclus : ils ne comptent pas).
pub(super) fn dernieres_parties(h: &[ResumePartie], n: usize) -> Vec<&ResumePartie> {
    h.iter().filter(|p| !p.remake).take(n).collect()
}

/// Note de la partie en étoiles. L'historique ne conserve pas encore les scores par axe :
/// on renvoie `None` (étoiles éteintes) tant qu'aucune note n'est stockée.
pub(super) fn note_de_partie(_p: &ResumePartie) -> Option<u8> {
    None
}

// =====================================================================
// Dessin
// =====================================================================

/// Portrait si le gestionnaire de textures est disponible, sinon simple espace réservé.
/// Le verrou est pris pour la durée de l'écran seulement, jamais gardé entre deux trames.
fn portrait(ui: &mut Ui, p: &mut Option<MutexGuard<'_, Portraits>>, id_dd: Option<&str>, t: TaillePortrait) {
    match p {
        Some(p) => {
            composants::portrait(ui, p, id_dd, None, t);
        }
        None => {
            ui.allocate_space(egui::vec2(t.cote(), t.cote()));
        }
    }
}

/// Identifiant d'icône d'un champion à partir de son id numérique.
fn id_dd(donnees: Option<&DonneesJeu>, champion: u16) -> Option<String> {
    donnees?.champions.get(&champion).map(|c| c.id.to_string())
}

/// Dessine l'accueil.
pub(super) fn dessiner_accueil(ui: &mut Ui, etat: &EtatAffichage) {
    let donnees = etat.donnees.as_deref();
    let partage = textures::portraits(ui.ctx());
    let mut garde = partage.as_ref().and_then(|p| p.lock().ok());

    mon_compte(ui, etat, donnees, &mut garde);
    ui.add_space(10.0);
    mes_champions(ui, etat, donnees, &mut garde);
    ui.add_space(10.0);
    dernieres(ui, etat, donnees, &mut garde);
}

fn mon_compte(ui: &mut Ui, etat: &EtatAffichage, donnees: Option<&DonneesJeu>, p: &mut Option<MutexGuard<'_, Portraits>>) {
    composants::panneau(ui, Some("Mon compte"), |ui| {
        let prog = etat.progression_lp.as_deref();
        let rang = prog.and_then(|g| rang_a_montrer(g.solo.as_ref(), g.flex.as_ref()));
        ui.horizontal(|ui| {
            composants::embleme_rang(ui, rang.map(|(r, _)| r), 58.0);
            ui.vertical(|ui| match rang {
                Some((r, file)) => {
                    ui.label(RichText::new(libelle_rang(r)).size(16.0).color(theme::couleur_palier(&r.palier)));
                    ui.label(RichText::new(format!("{file} · {}", libelle_bilan(r))).color(theme::TEXTE_SECONDAIRE));
                    if let Some(f) = fraction_lp(r) {
                        ui.add_space(4.0);
                        composants::barre(ui, f, 220.0);
                    }
                }
                None => {
                    ui.label(RichText::new("Pas encore classé").size(16.0).color(theme::TEXTE));
                    ui.label(RichText::new("Le rang apparaît avec une clé API et une partie classée.").color(theme::TEXTE_SECONDAIRE));
                }
            });
        });
        // Dernière partie : portrait + résultat.
        if let Some(d) = dernieres_parties(&etat.historique, 1).first() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                portrait(ui, p, id_dd(donnees, d.champion).as_deref(), TaillePortrait::Ligne);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Dernière partie").color(theme::TEXTE_SECONDAIRE));
                    ui.horizontal(|ui| {
                        composants::pastille_victoire(ui, d.victoire);
                        ui.label(format!("{} · {}", format::nom_champion(donnees, d.champion), format::kda(d.kills, d.morts, d.assists)));
                    });
                });
            });
        }
    });
}

fn mes_champions(ui: &mut Ui, etat: &EtatAffichage, donnees: Option<&DonneesJeu>, p: &mut Option<MutexGuard<'_, Portraits>>) {
    composants::panneau(ui, Some("Mes champions"), |ui| {
        let top = etat.stats_perso.as_deref().map(|s| meilleurs_champions(s, NB_CHAMPIONS)).unwrap_or_default();
        if top.is_empty() {
            ui.label(RichText::new("Pas encore de parties suivies.").color(theme::TEXTE_SECONDAIRE));
        }
        for (champion, victoires, parties) in top {
            ui.horizontal(|ui| {
                portrait(ui, p, id_dd(donnees, champion).as_deref(), TaillePortrait::Ligne);
                ui.vertical(|ui| {
                    ui.label(RichText::new(format::nom_champion(donnees, champion)).strong().color(theme::TEXTE));
                    ui.label(
                        RichText::new(format!("{} · {}", format::pluriel(victoires, "victoire"), format::pluriel(parties, "partie")))
                            .color(theme::TEXTE_SECONDAIRE),
                    );
                });
            });
        }
    });
}

fn dernieres(ui: &mut Ui, etat: &EtatAffichage, donnees: Option<&DonneesJeu>, p: &mut Option<MutexGuard<'_, Portraits>>) {
    composants::panneau(ui, Some("Cinq dernières parties"), |ui| {
        let parties = dernieres_parties(&etat.historique, NB_PARTIES);
        if parties.is_empty() {
            ui.label(RichText::new("Aucune partie dans l'historique.").color(theme::TEXTE_SECONDAIRE));
        }
        for d in parties {
            ui.horizontal(|ui| {
                composants::pastille_victoire(ui, d.victoire);
                portrait(ui, p, id_dd(donnees, d.champion).as_deref(), TaillePortrait::Ligne);
                ui.vertical(|ui| {
                    ui.label(RichText::new(format::nom_champion(donnees, d.champion)).color(theme::TEXTE));
                    ui.label(RichText::new(format::kda(d.kills, d.morts, d.assists)).color(theme::TEXTE_SECONDAIRE));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    composants::etoiles(ui, note_de_partie(d));
                });
            });
            ui.add_space(2.0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rang(palier: &str, division: &str, lp: u16) -> Rang {
        Rang { palier: palier.into(), division: division.into(), lp, victoires: 1, defaites: 0, serie: false }
    }

    #[test]
    fn fraction_et_libelles() {
        assert_eq!(fraction_lp(&rang("GOLD", "II", 45)), Some(0.45));
        assert_eq!(fraction_lp(&rang("GOLD", "II", 250)), Some(1.0));
        assert_eq!(fraction_lp(&rang("MASTER", "", 312)), None);
        assert_eq!(libelle_rang(&rang("GOLD", "II", 45)), "GOLD II · 45\u{a0}LP");
        assert_eq!(libelle_rang(&rang("MASTER", "", 312)), "MASTER · 312\u{a0}LP");
        assert_eq!(libelle_bilan(&rang("GOLD", "I", 0)), "1\u{a0}victoire · 0\u{a0}défaite");
    }

    #[test]
    fn solo_prioritaire_sur_flex() {
        let (s, f) = (rang("GOLD", "I", 1), rang("IRON", "IV", 2));
        assert_eq!(rang_a_montrer(Some(&s), Some(&f)).map(|x| x.1), Some("Solo/Duo"));
        assert_eq!(rang_a_montrer(None, Some(&f)).map(|x| x.1), Some("Flex"));
        assert!(rang_a_montrer(None, None).is_none());
    }

    #[test]
    fn top_champions_limite_a_n() {
        let g = |parties, victoires| StatsGroupe { parties, victoires, ..Default::default() };
        let stats = StatsPerso {
            total: g(10, 5),
            par_champion: vec![(1, g(5, 3)), (2, g(3, 1)), (3, g(1, 1)), (4, g(1, 0))],
            par_role: vec![],
            par_patch: vec![],
            par_famille: vec![],
            par_file: vec![],
        };
        assert_eq!(meilleurs_champions(&stats, 3), vec![(1, 3, 5), (2, 1, 3), (3, 1, 1)]);
    }

    #[test]
    fn accueil_sans_donnees_ne_panique_pas() {
        let ctx = egui::Context::default();
        theme::appliquer(&ctx);
        let etat = EtatAffichage::default();
        for _ in 0..2 {
            let mut sortie = ctx.run_ui(Default::default(), |ui| dessiner_accueil(ui, &etat));
            // egui exige que les textures de la trame soient traitées : on les jette volontairement.
            sortie.textures_delta.clear();
        }
    }
}
