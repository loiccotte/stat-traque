//! Onglet « Fiche » : radar 7 axes, MMR estimé, champions, rôles, style, forme.
//! Et le bouton « Copier pour Discord » (fiche ici, récap dans l'onglet Historique).
//!
//! La fenêtre ne calcule rien : elle affiche `etat.fiche` (préparée par fiche.rs
//! dans le processus principal) et ENVOIE des demandes (`enfant::demander`) que le
//! processus principal revérifie. La géométrie du radar est dans radar.rs (pure).

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Stroke, Ui};

use super::ecrans::couleur_winrate;
use super::protocole::Demande;
use super::{enfant, format, radar, theme};
use crate::affichage::{EtatAffichage, VueCibleFiche, VueFiche, VueMmr};
use crate::ddragon::DonneesJeu;

/// Durée d'affichage du petit « Copié ».
const DUREE_RETOUR: Duration = Duration::from_secs(2);

/// Dernière copie faite (pour afficher « Copié » à côté du BON bouton).
#[derive(Default)]
pub struct RetourCopie {
    /// (quel bouton, réussi ?, quand)
    dernier: Option<(&'static str, bool, Instant)>,
}

impl RetourCopie {
    /// Bouton « Copier pour Discord » + « Copié » pendant 2 s. `cle` distingue les
    /// boutons (« fiche », « recap »). Un seul redessin est programmé pour effacer
    /// le message (pas d'animation, pas de boucle).
    pub fn bouton(&mut self, ui: &mut Ui, cle: &'static str, texte: &str) {
        let bouton = ui.button("Copier pour Discord").on_hover_text("Copie un texte prêt à coller dans Discord");
        if bouton.clicked() {
            let ok = enfant::copier(ui.ctx(), texte);
            self.dernier = Some((cle, ok, Instant::now()));
        }
        let Some((c, ok, quand)) = self.dernier else { return };
        let reste = DUREE_RETOUR.saturating_sub(quand.elapsed());
        if c != cle || reste.is_zero() {
            return;
        }
        if ok {
            ui.label(RichText::new("Copié").color(theme::VERT).strong());
        } else {
            ui.label(RichText::new("Copie impossible").color(theme::ALERTE));
        }
        ui.ctx().request_repaint_after(reste);
    }
}

/// Ce que l'onglet retient entre deux images.
#[derive(Default)]
pub struct EtatOnglet {
    /// Joueur choisi dans la liste (puuid).
    cible: Option<String>,
}

/// L'onglet entier.
pub fn dessiner(ui: &mut Ui, etat: &EtatAffichage, onglet: &mut EtatOnglet, copie: &mut RetourCopie, donnees: Option<&DonneesJeu>) {
    barre_demandes(ui, etat, onglet, donnees);
    ui.add_space(8.0);
    let Some(f) = etat.fiche.as_deref() else {
        let texte = if etat.fiche_en_cours {
            "Chargement de la fiche..."
        } else {
            "Pas encore de fiche. « Ma fiche » : radar, MMR estimé et champions à partir de ton historique \
             (aucune requête). En partie, choisis un joueur pour voir la sienne."
        };
        ui.add_space(30.0);
        ui.vertical_centered(|ui| ui.label(RichText::new(texte).color(theme::TEXTE_DOUX).size(15.0)));
        return;
    };
    dessiner_fiche(ui, f, copie);
}

/// Boutons du haut : « Ma fiche », choix d'un joueur de la partie, chargement.
fn barre_demandes(ui: &mut Ui, etat: &EtatAffichage, onglet: &mut EtatOnglet, donnees: Option<&DonneesJeu>) {
    ui.horizontal_wrapped(|ui| {
        if ui.button(RichText::new("Ma fiche").strong()).on_hover_text("À partir de ton historique local : 0 requête Riot").clicked() {
            enfant::demander(&Demande::MaFiche);
        }
        if !etat.cibles_fiche.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new("Joueur de la partie :").color(theme::TEXTE_DOUX));
            // La cible choisie doit toujours être un joueur de la liste actuelle.
            if !onglet.cible.as_ref().is_some_and(|p| etat.cibles_fiche.iter().any(|c| &c.puuid == p)) {
                onglet.cible = etat.cibles_fiche.first().map(|c| c.puuid.clone());
            }
            let choisi = etat.cibles_fiche.iter().find(|c| Some(&c.puuid) == onglet.cible.as_ref());
            egui::ComboBox::from_id_salt("cible_fiche")
                .width(230.0)
                .selected_text(choisi.map_or(String::new(), |c| libelle_cible(c, donnees)))
                .show_ui(ui, |ui| {
                    for c in &etat.cibles_fiche {
                        ui.selectable_value(&mut onglet.cible, Some(c.puuid.clone()), libelle_cible(c, donnees));
                    }
                });
            if ui.button("Voir sa fiche").on_hover_text("Rang + 9 dernières parties classées : 11 requêtes au plus").clicked() {
                if let Some(p) = &onglet.cible {
                    enfant::demander(&Demande::Fiche { puuid: p.clone() });
                }
            }
        }
        if etat.fiche_en_cours {
            ui.add_space(12.0);
            // Texte fixe (pas de « spinner » animé : il redessinerait en continu).
            ui.label(RichText::new("Chargement...").color(theme::TURQUOISE).strong());
        }
    });
}

/// « Pseudo#TAG (Champion) ».
fn libelle_cible(c: &VueCibleFiche, donnees: Option<&DonneesJeu>) -> String {
    format!("{} ({})", c.nom, format::nom_champion(donnees, c.champion))
}

fn dessiner_fiche(ui: &mut Ui, f: &VueFiche, copie: &mut RetourCopie) {
    // En-tête : nom, rang, copier.
    ui.horizontal_wrapped(|ui| {
        let nom = if f.moi { "Ma fiche".to_string() } else { format!("Fiche de {}", f.nom) };
        ui.label(RichText::new(nom).color(theme::OR).strong().size(20.0));
        match &f.rang {
            Some(r) => ui.label(RichText::new(r).color(theme::couleur_palier(palier_anglais(r))).strong().size(16.0)),
            None => ui.label(RichText::new("Non classé").color(theme::TEXTE_DOUX)),
        };
        ui.add_space(12.0);
        copie.bouton(ui, "fiche", &f.texte_partage);
    });
    if let Some(e) = &f.erreur {
        ui.label(RichText::new(format!("! {e}")).color(theme::ALERTE));
    }
    ui.add_space(6.0);

    // Radar à gauche, MMR + forme + style à droite.
    ui.columns(2, |col| {
        theme::carte().show(&mut col[0], |ui| {
            ui.set_min_width(ui.available_width());
            theme::titre(ui, &format!("Radar vs {} même rôle", f.palier_reference));
            dessiner_radar(ui, f);
            let mut note = format!("50 = moyenne du palier · {} parties analysées", f.parties_analysees);
            if f.reference_approximative {
                note += " · référence approximative";
            }
            ui.label(RichText::new(note).color(theme::TEXTE_DOUX).small());
        });
        let ui = &mut col[1];
        theme::carte().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            dessiner_mmr(ui, f.mmr.as_ref());
        });
        ui.add_space(8.0);
        theme::carte().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            theme::titre(ui, "Forme récente");
            dessiner_forme(ui, &f.forme, f.serie);
            if !f.etiquettes.is_empty() {
                ui.add_space(6.0);
                theme::titre(ui, "Style de jeu");
                ui.horizontal_wrapped(|ui| {
                    for e in &f.etiquettes {
                        chip(ui, e);
                    }
                });
            }
        });
    });
    ui.add_space(8.0);
    ui.columns(2, |col| {
        theme::carte().show(&mut col[0], |ui| {
            ui.set_min_width(ui.available_width());
            theme::titre(ui, "Champions");
            dessiner_champions(ui, f);
        });
        theme::carte().show(&mut col[1], |ui| {
            ui.set_min_width(ui.available_width());
            theme::titre(ui, "Rôles");
            dessiner_roles(ui, f);
        });
    });
    ui.add_space(6.0);
    let requetes = if f.requetes == 0 { "0 requête Riot (base locale)".to_string() } else { format!("{} requêtes Riot", f.requetes) };
    ui.label(RichText::new(format!("{requetes} · MMR et radar sont des estimations à partir des données officielles Riot.")).color(theme::TEXTE_DOUX).small());
}

/// Le rang affiché est en français (« Émeraude II 45 LP ») ; la couleur se cherche
/// avec le palier anglais (« EMERALD ») : on cherche quel nom français (fiche.rs,
/// `palier_fr`) commence le texte. « Grand Master » est testé avant « Master ».
fn palier_anglais(rang: &str) -> &'static str {
    const PALIERS: [&str; 10] =
        ["GRANDMASTER", "MASTER", "CHALLENGER", "IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND"];
    PALIERS.into_iter().find(|en| rang.starts_with(crate::fiche::palier_fr(en))).unwrap_or("")
}

// ---------------------------------------------------------------- Radar

fn pos(p: [f32; 2]) -> Pos2 {
    egui::pos2(p[0], p[1])
}

fn dessiner_radar(ui: &mut Ui, f: &VueFiche) {
    let n = f.radar.len();
    if n < 3 {
        ui.label(RichText::new("Pas assez de données pour le radar.").color(theme::TEXTE_DOUX));
        return;
    }
    let cote = ui.available_width().clamp(220.0, 380.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), cote * 0.86), egui::Sense::hover());
    let c = [rect.center().x, rect.center().y + 4.0];
    // Place pour les libellés autour : ~70 px à gauche / à droite, ~22 px en haut / en bas.
    let rayon = (rect.width() / 2.0 - 78.0).min(rect.height() / 2.0 - 24.0).max(40.0);
    let p = ui.painter_at(rect.expand(4.0));
    let trait_toile = Stroke::new(1.0, theme::OR_SOMBRE.gamma_multiply(0.7));

    // Toile : graduations 25 / 50 / 75 / 100 (50 = moyenne, un peu plus marquée) + axes.
    for g in radar::GRADUATIONS.iter().copied().chain([100]) {
        let points = radar::toile(c, rayon * g as f32 / 100.0, n).into_iter().map(pos).collect();
        let s = if g == 50 { Stroke::new(1.2, theme::OR_SOMBRE) } else { trait_toile };
        p.add(egui::Shape::closed_line(points, s));
    }
    for i in 0..n {
        p.line_segment([pos(c), pos(radar::sommet(c, rayon, i, n))], trait_toile);
    }
    for g in radar::GRADUATIONS {
        // Valeur de la graduation, le long de l'axe du haut.
        let q = radar::point(c, rayon, 0, n, g as f32 / 100.0);
        p.text(pos([q[0] + 3.0, q[1]]), Align2::LEFT_CENTER, g.to_string(), FontId::proportional(9.0), theme::TEXTE_DOUX.gamma_multiply(0.7));
    }

    // Polygone des scores : rempli en « éventail » depuis le centre (marche même si
    // le polygone n'est pas convexe), puis contour.
    let scores: Vec<Option<u8>> = f.radar.iter().map(|a| a.score).collect();
    let points: Vec<Pos2> = radar::polygone(c, rayon, &scores).into_iter().map(pos).collect();
    let mut maillage = egui::Mesh::default();
    let remplissage = Color32::from_rgba_unmultiplied(10, 200, 185, 60);
    maillage.colored_vertex(pos(c), remplissage);
    for q in &points {
        maillage.colored_vertex(*q, remplissage);
    }
    for i in 0..n as u32 {
        maillage.add_triangle(0, 1 + i, 1 + (i + 1) % n as u32);
    }
    p.add(egui::Shape::mesh(maillage));
    p.add(egui::Shape::closed_line(points.clone(), Stroke::new(2.0, theme::TURQUOISE)));

    // Sommets : point + valeur ; bout des axes : libellé.
    for (i, axe) in f.radar.iter().enumerate() {
        let (h, v) = radar::ancrage(i, n);
        let aligne = Align2([aligne_h(h), aligne_v(v)]);
        let decale = |q: [f32; 2], d: f32| pos([q[0] + h as f32 * d, q[1] + v as f32 * d]);
        let bout = radar::sommet(c, rayon, i, n);
        let couleur_nom = if axe.score.is_some() { theme::TEXTE } else { theme::TEXTE_DOUX };
        p.text(decale(bout, 8.0), aligne, &axe.nom, FontId::proportional(13.0), couleur_nom);
        match axe.score {
            Some(s) => {
                p.circle_filled(points[i], 3.5, theme::TURQUOISE);
                let valeur = radar::point(c, rayon, i, n, s as f32 / 100.0);
                p.text(decale(valeur, 6.0), aligne, s.to_string(), FontId::proportional(12.0), couleur_score(s));
            }
            None => {
                let q = radar::point(c, rayon, i, n, 0.18);
                p.text(pos(q), Align2::CENTER_CENTER, "?", FontId::proportional(12.0), theme::TEXTE_DOUX);
            }
        }
    }
}

fn aligne_h(h: i8) -> egui::Align {
    match h {
        1 => egui::Align::Min,  // texte vers la droite du point
        -1 => egui::Align::Max, // texte vers la gauche
        _ => egui::Align::Center,
    }
}

fn aligne_v(v: i8) -> egui::Align {
    match v {
        1 => egui::Align::Min,  // en dessous
        -1 => egui::Align::Max, // au-dessus
        _ => egui::Align::Center,
    }
}

/// Vert au-dessus de la moyenne (≥ 60), rouge en dessous (≤ 40).
fn couleur_score(s: u8) -> Color32 {
    if s >= 60 {
        theme::VERT
    } else if s <= 40 {
        theme::EQUIPE_ROUGE
    } else {
        theme::OR
    }
}

// ---------------------------------------------------------------- MMR, forme, style

fn dessiner_mmr(ui: &mut Ui, mmr: Option<&VueMmr>) {
    ui.horizontal(|ui| {
        theme::titre(ui, "MMR estimé");
        ui.label(RichText::new("estimation").color(theme::TEXTE_DOUX).italics().small());
    });
    let Some(m) = mmr else {
        ui.label(RichText::new("Pas assez de parties classées récentes pour l'estimer.").color(theme::TEXTE_DOUX));
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&m.rang_equivalent).color(theme::couleur_palier(palier_anglais(m.rang_equivalent.trim_start_matches('~')))).strong().size(20.0));
        let couleur = match m.ecart_lp {
            e if e >= 20 => theme::VERT,
            e if e <= -20 => theme::EQUIPE_ROUGE,
            _ => theme::TEXTE,
        };
        ui.label(RichText::new(format!("{:+} LP", m.ecart_lp)).color(couleur).strong());
        ui.label(RichText::new("vs rang affiché").color(theme::TEXTE_DOUX));
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Confiance :").color(theme::TEXTE_DOUX));
        let couleur = match m.confiance.as_str() {
            "bonne" => theme::VERT,
            "moyenne" => theme::OR,
            _ => theme::ALERTE,
        };
        ui.label(RichText::new(&m.confiance).color(couleur));
    });
    for r in &m.raisons {
        ui.label(RichText::new(format!("• {r}")).color(theme::TEXTE_DOUX).small());
    }
    ui.label(RichText::new("Riot ne publie pas le MMR : ceci est une estimation.").color(theme::TEXTE_DOUX).italics().small());
}

/// Les 10 dernières parties : une case verte (V) ou rouge (D), la plus récente à gauche.
fn dessiner_forme(ui: &mut Ui, forme: &str, serie: i32) {
    if forme.is_empty() {
        ui.label(RichText::new("Aucune partie récente.").color(theme::TEXTE_DOUX));
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for lettre in forme.chars().take(10) {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            let (fond, texte) = match lettre {
                'V' => (theme::VERT.gamma_multiply(0.75), "V"),
                'D' => (theme::EQUIPE_ROUGE.gamma_multiply(0.75), "D"),
                _ => (theme::CADRE, "?"),
            };
            ui.painter().rect_filled(rect, 3.0, fond);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, texte, FontId::proportional(12.0), Color32::WHITE);
        }
        ui.add_space(10.0);
        let (texte, couleur) = match serie {
            s if s > 0 => (format!("{s} victoire(s) d'affilée"), theme::VERT),
            s if s < 0 => (format!("{} défaite(s) d'affilée", -s), theme::EQUIPE_ROUGE),
            _ => (String::new(), theme::TEXTE),
        };
        ui.label(RichText::new(texte).color(couleur));
    });
    let victoires = forme.chars().filter(|&c| c == 'V').count();
    ui.label(RichText::new(format!("{victoires} V / {} D", forme.chars().count() - victoires)).color(theme::TEXTE_DOUX).small());
}

/// Étiquette arrondie (« Farmer », « One-trick »...).
fn chip(ui: &mut Ui, texte: &str) {
    egui::Frame::NONE
        .fill(Color32::from_rgb(20, 50, 70))
        .stroke(Stroke::new(1.0, theme::TURQUOISE.gamma_multiply(0.6)))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(9, 3))
        .show(ui, |ui| ui.label(RichText::new(texte).color(theme::TURQUOISE)));
}

// ---------------------------------------------------------------- Champions, rôles

fn dessiner_champions(ui: &mut Ui, f: &VueFiche) {
    if f.champions.is_empty() {
        ui.label(RichText::new("Aucune partie analysée.").color(theme::TEXTE_DOUX));
        return;
    }
    egui::Grid::new("fiche_champions").num_columns(5).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
        for entete in ["Champion", "Parties", "Victoires", "Part", "KDA"] {
            ui.label(RichText::new(entete).color(theme::TEXTE_DOUX).small());
        }
        ui.end_row();
        for c in f.champions.iter().take(5) {
            ui.label(RichText::new(&c.champion).strong());
            ui.label(c.parties.to_string());
            ui.label(RichText::new(format::pourcent_entier(c.winrate)).color(couleur_winrate(c.winrate)));
            ui.label(format::pourcent_entier(c.part));
            ui.label(format::decimal(c.kda, 1));
            ui.end_row();
        }
    });
}

fn dessiner_roles(ui: &mut Ui, f: &VueFiche) {
    if f.roles.is_empty() {
        ui.label(RichText::new("Rôles inconnus.").color(theme::TEXTE_DOUX));
        return;
    }
    egui::Grid::new("fiche_roles").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
        for r in &f.roles {
            ui.label(RichText::new(crate::fiche::role_fr(&r.role)).strong());
            ui.add(egui::ProgressBar::new(format::fraction(r.part, 100.0)).desired_width(140.0).desired_height(10.0).fill(theme::OR_SOMBRE));
            ui.label(RichText::new(format!("{} · {} parties", format::pourcent_entier(r.part), r.parties)).color(theme::TEXTE_DOUX));
            ui.end_row();
        }
    });
}
