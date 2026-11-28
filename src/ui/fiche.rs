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
use super::composants::{self, TaillePortrait};
use super::{enfant, format, radar, textures, theme};
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
    /// puuid de la dernière fiche affichée : quand une nouvelle fiche arrive, la
    /// liste se place sur ce joueur.
    fiche_vue: String,
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
    dessiner_fiche(ui, f, copie, donnees);
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
            if let Some(f) = etat.fiche.as_deref().filter(|f| f.puuid != onglet.fiche_vue) {
                onglet.fiche_vue = f.puuid.clone();
                if !f.moi {
                    onglet.cible = Some(f.puuid.clone());
                }
            }
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

fn dessiner_fiche(ui: &mut Ui, f: &VueFiche, copie: &mut RetourCopie, donnees: Option<&DonneesJeu>) {
    entete(ui, f, copie, donnees);
    if let Some(e) = &f.erreur {
        ui.label(RichText::new(format!("! {e}")).color(theme::ALERTE));
    }
    ui.add_space(8.0);

    // Radar + spécificités à gauche ; forme et détail du MMR à droite.
    ui.columns(2, |col| {
        composants::panneau(&mut col[0], Some(&format!("Radar vs {} même rôle", f.palier_reference)), |ui| {
            dessiner_radar(ui, f);
            ui.label(RichText::new(note_radar(f)).color(theme::TEXTE_SECONDAIRE).small());
            if !f.etiquettes.is_empty() {
                ui.add_space(8.0);
                theme::titre(ui, "Spécificités");
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    for e in &f.etiquettes {
                        chip(ui, e);
                    }
                });
            }
        });
        let ui = &mut col[1];
        composants::panneau(ui, Some("Forme récente"), |ui| dessiner_forme(ui, &f.forme, f.serie));
        ui.add_space(8.0);
        composants::panneau(ui, None, |ui| dessiner_mmr(ui, f.mmr.as_ref()));
    });
    ui.add_space(8.0);
    ui.columns(2, |col| {
        composants::panneau(&mut col[0], Some("Champions"), |ui| dessiner_champions(ui, f, donnees));
        composants::panneau(&mut col[1], Some("Rôles"), |ui| dessiner_roles(ui, f));
    });
    ui.add_space(6.0);
    let requetes = if f.requetes == 0 { "0 requête Riot (base locale)".to_string() } else { format!("{} requêtes Riot", f.requetes) };
    ui.label(RichText::new(format!("{requetes} · MMR et radar sont des estimations à partir des données officielles Riot.")).color(theme::TEXTE_SECONDAIRE).small());
}

// ---------------------------------------------------------------- Fonctions pures (testées)

/// Titre de la fiche : « Ma fiche » ou « Fiche de Pseudo#TAG ».
fn titre_fiche(moi: bool, nom: &str) -> String {
    if moi { "Ma fiche".to_string() } else { format!("Fiche de {nom}") }
}

/// Nom (anglais) du champion principal = le plus joué (la liste est déjà triée).
fn champion_principal(f: &VueFiche) -> Option<&str> {
    f.champions.first().map(|c| c.champion.as_str())
}

/// Ligne sous le radar : « 50 = moyenne du palier · 12 parties analysées ».
fn note_radar(f: &VueFiche) -> String {
    let mut note = format!("50 = moyenne du palier · {} analysée", format::pluriel(f.parties_analysees, "partie"));
    if f.parties_analysees >= 2 {
        note.push('s');
    }
    if f.reference_approximative {
        note += " · référence approximative";
    }
    note
}

/// Texte de la série : « 3 victoires d'affilée », « 1 défaite d'affilée », vide à 0.
fn texte_serie(serie: i32) -> String {
    match serie {
        0 => String::new(),
        s if s > 0 => format!("{} d'affilée", format::pluriel(s as u32, "victoire")),
        s => format!("{} d'affilée", format::pluriel(s.unsigned_abs(), "défaite")),
    }
}

/// Les 10 dernières parties en booléens (true = victoire). Les lettres inconnues sont ignorées.
fn resultats_forme(forme: &str) -> Vec<bool> {
    forme.chars().filter(|c| matches!(c, 'V' | 'D')).take(10).map(|c| c == 'V').collect()
}

/// Libellé d'un rôle : « 54 % · 1 partie ».
fn texte_role(part: f32, parties: u32) -> String {
    format!("{} · {}", format::pourcent_entier(part), format::pluriel(parties, "partie"))
}

/// Couleur de la confiance du MMR.
fn couleur_confiance(confiance: &str) -> Color32 {
    match confiance {
        "bonne" => theme::VERT,
        "moyenne" => theme::OR,
        _ => theme::ALERTE,
    }
}

// ---------------------------------------------------------------- En-tête

/// Portrait via le cache partagé : le verrou n'est tenu que le temps du dessin du
/// portrait, jamais d'une trame à l'autre. Sans cache, on réserve juste la place.
fn portrait(ui: &mut Ui, id_dd: Option<&str>, t: TaillePortrait) {
    if let Some(partage) = textures::portraits(ui.ctx()) {
        if let Ok(mut p) = partage.lock() {
            composants::portrait(ui, &mut p, id_dd, None, t);
            return;
        }
    }
    ui.allocate_space(egui::vec2(t.cote(), t.cote()));
}

/// Portrait 66 px du champion principal, nom, rang, MMR estimé + confiance, bouton Discord.
fn entete(ui: &mut Ui, f: &VueFiche, copie: &mut RetourCopie, donnees: Option<&DonneesJeu>) {
    composants::panneau(ui, None, |ui| {
        ui.horizontal(|ui| {
            let id_dd = champion_principal(f).and_then(|n| format::id_dd_depuis_nom(donnees, n));
            let id_dd = id_dd.as_deref().or_else(|| champion_principal(f));
            portrait(ui, id_dd, TaillePortrait::Fiche);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(titre_fiche(f.moi, &f.nom)).font(theme::police_titre(20.0)).color(theme::OR));
                ui.horizontal_wrapped(|ui| {
                    match &f.rang {
                        Some(r) => ui.label(RichText::new(r).color(theme::couleur_palier(palier_anglais(r))).size(16.0)),
                        None => ui.label(RichText::new("Non classé").color(theme::TEXTE_SECONDAIRE)),
                    };
                    if let Some(m) = &f.mmr {
                        ui.label(RichText::new("·").color(theme::TEXTE_SECONDAIRE));
                        ui.label(RichText::new(format!("MMR ≈ {}", m.rang_equivalent)).color(theme::TEXTE));
                        ui.label(RichText::new("(estimation)").color(theme::TEXTE_SECONDAIRE).italics().small());
                        ui.label(RichText::new(format!("confiance {}", m.confiance)).color(couleur_confiance(&m.confiance)));
                    }
                });
                ui.add_space(4.0);
                copie.bouton(ui, "fiche", &f.texte_partage);
            });
        });
    });
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
        ui.label(RichText::new("Pas assez de données pour le radar.").color(theme::TEXTE_SECONDAIRE));
        return;
    }
    let cote = ui.available_width().clamp(220.0, 380.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), cote * 0.86), egui::Sense::hover());
    let c = [rect.center().x, rect.center().y + 4.0];
    // Place pour les libellés autour : ~70 px à gauche / à droite, ~22 px en haut / en bas.
    let rayon = radar::rayon_pour(rect.width(), rect.height());
    let p = ui.painter_at(rect.expand(4.0));
    let trait_toile = Stroke::new(1.0, theme::OR_SOMBRE.gamma_multiply(0.8));

    // Toile : graduations 25 / 50 / 75 / 100 (50 = moyenne, un peu plus marquée) + axes.
    for g in radar::GRADUATIONS.iter().copied().chain([100]) {
        let points = radar::toile(c, rayon * g as f32 / 100.0, n).into_iter().map(pos).collect();
        let s = if g == 50 { Stroke::new(1.0, theme::OR_SOMBRE) } else { trait_toile };
        p.add(egui::Shape::closed_line(points, s));
    }
    for i in 0..n {
        p.line_segment([pos(c), pos(radar::sommet(c, rayon, i, n))], trait_toile);
    }
    for g in radar::GRADUATIONS {
        // Valeur de la graduation, ENTRE les axes 0 et 1 (sur un axe, elle cacherait
        // la valeur du sommet) : l'angle « 1 sur 2n » tombe pile au milieu.
        let q = radar::point(c, rayon, 1, 2 * n, g as f32 / 100.0);
        p.text(pos(q), Align2::CENTER_CENTER, g.to_string(), FontId::proportional(9.0), theme::TEXTE_SECONDAIRE.gamma_multiply(0.7));
    }

    // Polygone des scores : rempli en « éventail » depuis le centre (marche même si
    // le polygone n'est pas convexe), puis contour.
    let scores: Vec<Option<u8>> = f.radar.iter().map(|a| a.score).collect();
    let points: Vec<Pos2> = radar::polygone(c, rayon, &scores).into_iter().map(pos).collect();
    let mut maillage = egui::Mesh::default();
    let remplissage = theme::OR.gamma_multiply(0.2);
    maillage.colored_vertex(pos(c), remplissage);
    for q in &points {
        maillage.colored_vertex(*q, remplissage);
    }
    for i in 0..n as u32 {
        maillage.add_triangle(0, 1 + i, 1 + (i + 1) % n as u32);
    }
    p.add(egui::Shape::mesh(maillage));
    p.add(egui::Shape::closed_line(points.clone(), Stroke::new(1.5, theme::OR)));

    // Sommets : point + valeur ; bout des axes : libellé.
    for (i, axe) in f.radar.iter().enumerate() {
        let (h, v) = radar::ancrage(i, n);
        let aligne = Align2([aligne_h(h), aligne_v(v)]);
        let decale = |q: [f32; 2], d: f32| pos([q[0] + h as f32 * d, q[1] + v as f32 * d]);
        let bout = radar::sommet(c, rayon, i, n);
        let couleur_nom = if axe.score.is_some() { theme::TEXTE } else { theme::TEXTE_SECONDAIRE };
        p.text(decale(bout, 8.0), aligne, &axe.nom, FontId::proportional(13.0), couleur_nom);
        match axe.score {
            Some(s) => {
                p.circle_filled(points[i], 3.0, theme::OR);
                let valeur = radar::point(c, rayon, i, n, s as f32 / 100.0);
                p.text(decale(valeur, 6.0), aligne, s.to_string(), FontId::proportional(12.0), couleur_score(s));
            }
            None => {
                let q = radar::point(c, rayon, i, n, 0.18);
                p.text(pos(q), Align2::CENTER_CENTER, "?", FontId::proportional(12.0), theme::TEXTE_SECONDAIRE);
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
        ui.label(RichText::new("estimation").color(theme::TEXTE_SECONDAIRE).italics().small());
    });
    ui.add_space(2.0);
    let Some(m) = mmr else {
        ui.label(RichText::new("Pas assez de parties classées récentes pour l'estimer.").color(theme::TEXTE_SECONDAIRE));
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&m.rang_equivalent).color(theme::couleur_palier(palier_anglais(m.rang_equivalent.trim_start_matches('~')))).size(20.0));
        let couleur = match m.ecart_lp {
            e if e >= 20 => theme::VERT,
            e if e <= -20 => theme::EQUIPE_ROUGE,
            _ => theme::TEXTE,
        };
        ui.label(RichText::new(format!("{:+} LP", m.ecart_lp)).color(couleur));
        ui.label(RichText::new("vs rang affiché").color(theme::TEXTE_SECONDAIRE));
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Confiance :").color(theme::TEXTE_SECONDAIRE));
        ui.label(RichText::new(&m.confiance).color(couleur_confiance(&m.confiance)));
    });
    for r in &m.raisons {
        ui.label(RichText::new(format!("• {r}")).color(theme::TEXTE_SECONDAIRE).small());
    }
    ui.label(RichText::new("Riot ne publie pas le MMR : ceci est une estimation.").color(theme::TEXTE_SECONDAIRE).italics().small());
}

/// Les 10 dernières parties : pastilles V / D (plus récente à gauche) et la série en cours.
fn dessiner_forme(ui: &mut Ui, forme: &str, serie: i32) {
    let resultats = resultats_forme(forme);
    if resultats.is_empty() {
        ui.label(RichText::new("Aucune partie récente.").color(theme::TEXTE_SECONDAIRE));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for &victoire in &resultats {
            composants::pastille_victoire(ui, victoire);
        }
        ui.add_space(10.0);
        let couleur = if serie > 0 { theme::VICTOIRE } else { theme::DEFAITE };
        ui.label(RichText::new(texte_serie(serie)).color(couleur));
    });
    let victoires = resultats.iter().filter(|&&v| v).count();
    ui.label(RichText::new(format!("{victoires} V / {} D", resultats.len() - victoires)).color(theme::TEXTE_SECONDAIRE).small());
}

/// Étiquette à angles droits (« Farmer », « One-trick »...) : filet or, texte clair.
fn chip(ui: &mut Ui, texte: &str) {
    egui::Frame::NONE
        .fill(theme::LIGNE_RANGEE)
        .stroke(Stroke::new(1.0, theme::OR_SOMBRE))
        .corner_radius(egui::CornerRadius::ZERO)
        .inner_margin(egui::Margin::symmetric(9, 3))
        .show(ui, |ui| ui.label(RichText::new(texte).color(theme::TEXTE)));
}

// ---------------------------------------------------------------- Champions, rôles

fn dessiner_champions(ui: &mut Ui, f: &VueFiche, donnees: Option<&DonneesJeu>) {
    if f.champions.is_empty() {
        ui.label(RichText::new("Aucune partie analysée.").color(theme::TEXTE_SECONDAIRE));
        return;
    }
    egui::Grid::new("fiche_champions").num_columns(5).spacing([14.0, 6.0]).show(ui, |ui| {
        for entete in ["", "Champion", "Parties", "Victoires", "KDA"] {
            ui.label(RichText::new(entete).color(theme::TEXTE_SECONDAIRE).small());
        }
        ui.end_row();
        for c in f.champions.iter().take(5) {
            let id_dd = format::id_dd_depuis_nom(donnees, &c.champion);
            portrait(ui, id_dd.as_deref().or(Some(c.champion.as_str())), TaillePortrait::Ligne);
            ui.label(RichText::new(&c.champion).color(theme::TEXTE));
            ui.label(RichText::new(format::pluriel(c.parties, "partie")).color(theme::TEXTE));
            ui.label(RichText::new(format::pourcent_entier(c.winrate)).color(couleur_winrate(c.winrate)));
            ui.label(RichText::new(format::decimal(c.kda, 1)).color(theme::TEXTE));
            ui.end_row();
        }
    });
}

fn dessiner_roles(ui: &mut Ui, f: &VueFiche) {
    if f.roles.is_empty() {
        ui.label(RichText::new("Rôles inconnus.").color(theme::TEXTE_SECONDAIRE));
        return;
    }
    egui::Grid::new("fiche_roles").num_columns(3).spacing([12.0, 8.0]).show(ui, |ui| {
        for r in &f.roles {
            ui.label(RichText::new(crate::fiche::role_fr(&r.role)).color(theme::TEXTE));
            composants::barre(ui, format::fraction(r.part, 100.0), 140.0);
            ui.label(RichText::new(texte_role(r.part, r.parties)).color(theme::TEXTE_SECONDAIRE));
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affichage::{VueAxeRadar, VueChampionFiche, VueRoleFiche};

    fn fiche() -> VueFiche {
        VueFiche {
            puuid: "p".into(),
            nom: "Test#EUW".into(),
            rang: Some("Émeraude II 45 LP".into()),
            palier_reference: "Émeraude".into(),
            radar: ["Combat", "Farm", "Vision", "Objectifs", "Phase de lane", "Survie", "Agressivité"]
                .iter()
                .enumerate()
                .map(|(i, n)| VueAxeRadar { nom: n.to_string(), score: if i == 2 { None } else { Some(40 + i as u8 * 8) } })
                .collect(),
            mmr: Some(VueMmr { rang_equivalent: "~Émeraude I".into(), ecart_lp: 30, confiance: "moyenne".into(), raisons: vec!["gains moyens +24 LP".into()] }),
            champions: vec![
                VueChampionFiche { champion: "Ahri".into(), parties: 1, victoires: 1, winrate: 100.0, part: 10.0, kda: 3.2 },
                VueChampionFiche { champion: "Inconnu".into(), parties: 5, victoires: 2, winrate: 40.0, part: 50.0, kda: 2.0 },
            ],
            roles: vec![VueRoleFiche { role: "middle".into(), parties: 1, part: 100.0 }],
            etiquettes: vec!["Farmer".into()],
            forme: "VVDVD".into(),
            serie: 2,
            parties_analysees: 1,
            texte_partage: "texte".into(),
            ..Default::default()
        }
    }

    #[test]
    fn titres() {
        assert_eq!(titre_fiche(true, "X"), "Ma fiche");
        assert_eq!(titre_fiche(false, "A#B"), "Fiche de A#B");
    }

    #[test]
    fn champion_principal_est_le_premier() {
        assert_eq!(champion_principal(&fiche()), Some("Ahri"));
        assert_eq!(champion_principal(&VueFiche::default()), None);
    }

    #[test]
    fn pluriels_corrects() {
        assert_eq!(texte_serie(0), "");
        assert_eq!(texte_serie(1), "1\u{a0}victoire d'affilée");
        assert_eq!(texte_serie(-3), "3\u{a0}défaites d'affilée");
        assert_eq!(texte_role(54.4, 1), "54\u{a0}% · 1\u{a0}partie");
        assert!(texte_role(10.0, 3).ends_with("parties"));
        let mut f = fiche();
        assert!(note_radar(&f).contains("1\u{a0}partie analysée") && !note_radar(&f).contains("parties"));
        f.parties_analysees = 12;
        f.reference_approximative = true;
        assert!(note_radar(&f).contains("12\u{a0}parties analysées") && note_radar(&f).ends_with("approximative"));
    }

    #[test]
    fn forme_ignore_les_lettres_inconnues_et_borne_a_10() {
        assert_eq!(resultats_forme("VD?V"), vec![true, false, true]);
        assert_eq!(resultats_forme("VVVVVVVVVVVVVV").len(), 10);
        assert!(resultats_forme("").is_empty());
    }

    #[test]
    fn confiance_coloree() {
        assert_eq!(couleur_confiance("bonne"), theme::VERT);
        assert_eq!(couleur_confiance("moyenne"), theme::OR);
        assert_eq!(couleur_confiance("faible"), theme::ALERTE);
    }

    /// Dessin complet (avec et sans données) dans un contexte egui sans fenêtre : pas de panique.
    #[test]
    fn dessin_sans_panique() {
        let ctx = egui::Context::default();
        theme::appliquer(&ctx);
        let vide = VueFiche { radar: vec![], ..Default::default() };
        for f in [fiche(), vide] {
            let mut copie = RetourCopie::default();
            for _ in 0..2 {
                let mut sortie = ctx.run_ui(Default::default(), |ui| dessiner_fiche(ui, &f, &mut copie, None));
                sortie.textures_delta.clear();
            }
        }
    }
}
