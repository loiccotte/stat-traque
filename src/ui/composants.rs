//! Composants graphiques du thème (panneau, étoiles, portrait, onglets...).
//!
//! Règles communes :
//!  - tout est peint à la main avec le `Painter` d'egui, à angles droits ;
//!  - rien ne demande de redessin (`request_repaint`) : le coût au repos est nul ;
//!  - la géométrie (positions des losanges, des étoiles...) est calculée par de petites
//!    fonctions PURES, testées plus bas, pour garder le dessin lui-même très simple ;
//!  - chiffres alignés : egui n'a pas de chiffres tabulaires, donc les nombres sont
//!    alignés à DROITE dans des colonnes de largeur fixe.
//!
//! Rempli par T-01 (panneau, titre, onglets, barre, pastille, étoiles, portrait, emblème).

use eframe::egui::{
    epaint::Mesh, vec2, Align2, Color32, CornerRadius, FontId, Frame, Margin, Pos2, Rect, Response, Sense, Shape,
    Stroke, StrokeKind, Ui, Vec2,
};

use super::onglets::Onglet;
use super::palette;
use super::textures::Portraits;
use super::theme::{self, avec_alpha};
use crate::icones::Taille;
use crate::note;
use crate::rangs::Rang;

/// Taille d'un portrait de champion : 38 px (ligne) ou 66 px (fiche).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaillePortrait {
    Ligne,
    Fiche,
}

impl TaillePortrait {
    /// Côté du carré, en points.
    pub fn cote(self) -> f32 {
        match self {
            TaillePortrait::Ligne => 38.0,
            TaillePortrait::Fiche => 66.0,
        }
    }

    /// Épaisseur de la bordure or : 1 px, ou 2 px pour la fiche.
    pub fn bordure(self) -> f32 {
        match self {
            TaillePortrait::Ligne => 1.0,
            TaillePortrait::Fiche => 2.0,
        }
    }

    /// Taille de texte du niveau (dans son petit cadre).
    fn texte_niveau(self) -> f32 {
        match self {
            TaillePortrait::Ligne => 9.0,
            TaillePortrait::Fiche => 11.0,
        }
    }

    fn icone(self) -> Taille {
        match self {
            TaillePortrait::Ligne => Taille::Ligne,
            TaillePortrait::Fiche => Taille::Fiche,
        }
    }
}

// =============================================================================================
// Géométrie pure (testée)
// =============================================================================================

/// Côté d'un petit losange décoratif du panneau, en points (7 px d'après le design).
const LOSANGE_PANNEAU: f32 = 7.0;

/// Les 4 sommets d'un losange (haut, droite, bas, gauche), dans le sens des aiguilles d'une montre.
pub fn sommets_losange(centre: Pos2, demi_largeur: f32, demi_hauteur: f32) -> [Pos2; 4] {
    [
        Pos2::new(centre.x, centre.y - demi_hauteur),
        Pos2::new(centre.x + demi_largeur, centre.y),
        Pos2::new(centre.x, centre.y + demi_hauteur),
        Pos2::new(centre.x - demi_largeur, centre.y),
    ]
}

/// Centres des deux losanges posés sur le bord haut d'un panneau (gauche et droite).
/// `retrait` : distance entre le coin et le centre du losange.
pub fn centres_losanges_panneau(rect: Rect, retrait: f32) -> [Pos2; 2] {
    [
        Pos2::new(rect.left() + retrait, rect.top()),
        Pos2::new(rect.right() - retrait, rect.top()),
    ]
}

/// Les 10 sommets d'une étoile à 5 branches (alternance pointe / creux), première pointe en haut.
pub fn sommets_etoile(centre: Pos2, rayon_pointe: f32, rayon_creux: f32) -> [Pos2; 10] {
    let mut pts = [centre; 10];
    for (i, p) in pts.iter_mut().enumerate() {
        let rayon = if i % 2 == 0 { rayon_pointe } else { rayon_creux };
        // 36 degrés entre deux sommets ; -90 degrés = vers le haut (l'axe y d'egui descend).
        let angle = (i as f32) * std::f32::consts::PI / 5.0 - std::f32::consts::FRAC_PI_2;
        *p = Pos2::new(centre.x + rayon * angle.cos(), centre.y + rayon * angle.sin());
    }
    pts
}

/// Centres des 5 étoiles alignées : la première commence à `x0`, `pas` entre deux centres.
pub fn centres_etoiles(x0: f32, y: f32, pas: f32) -> [Pos2; 5] {
    let mut c = [Pos2::ZERO; 5];
    for (i, p) in c.iter_mut().enumerate() {
        *p = Pos2::new(x0 + pas * (i as f32 + 0.5), y);
    }
    c
}

/// Largeur de la partie remplie d'une barre (fraction bornée à 0..=1, NaN = 0).
pub fn largeur_remplie(fraction: f32, largeur: f32) -> f32 {
    if fraction.is_nan() {
        return 0.0;
    }
    fraction.clamp(0.0, 1.0) * largeur
}

/// Soulignement de 2 px collé en bas d'un onglet.
pub fn rect_soulignement(onglet: Rect) -> Rect {
    Rect::from_min_max(Pos2::new(onglet.left(), onglet.bottom() - 2.0), onglet.right_bottom())
}

/// Petit cadre du niveau, en bas à droite du portrait (il déborde un peu vers l'extérieur de 1 px
/// pour recouvrir la bordure). `texte` : taille du texte du niveau.
pub fn rect_niveau(portrait: Rect, texte: Vec2) -> Rect {
    let taille = texte + vec2(5.0, 1.0);
    Rect::from_min_size(portrait.right_bottom() - taille + vec2(1.0, 1.0), taille)
}

/// Initiale affichée dans l'emblème de rang ou dans un portrait absent (« ? » si inconnue).
pub fn initiale(texte: Option<&str>) -> String {
    texte
        .and_then(|t| t.trim().chars().next())
        .map_or_else(|| "?".to_string(), |c| c.to_uppercase().collect())
}

/// Or clair à partir de `note::SEUIL_S`, or normal sinon. JAMAIS une autre couleur pour une note.
pub fn couleur_etoiles(note: u8) -> Color32 {
    if note >= note::SEUIL_S {
        theme::ACCENT
    } else {
        theme::OR
    }
}

// =============================================================================================
// Petits outils de dessin
// =============================================================================================

/// Remplit un polygone en éventail depuis son centre (valable pour une étoile, qui n'est pas convexe).
fn polygone_etoile(centre: Pos2, sommets: &[Pos2], couleur: Color32) -> Shape {
    let mut m = Mesh::default();
    m.colored_vertex(centre, couleur);
    for p in sommets {
        m.colored_vertex(*p, couleur);
    }
    let n = sommets.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    Shape::mesh(m)
}

/// Losange plein + contour.
fn losange(ui: &Ui, centre: Pos2, demi: f32, fond: Color32, trait_: Stroke) {
    ui.painter()
        .add(Shape::convex_polygon(sommets_losange(centre, demi, demi).to_vec(), fond, trait_));
}

// =============================================================================================
// Composants
// =============================================================================================

/// Panneau à angles droits avec titre optionnel : fond #0B1428 à ~67 %, filet or à ~19 %,
/// deux losanges or de 7 px sur le bord haut. Occupe toute la largeur disponible.
pub fn panneau<R>(ui: &mut Ui, titre: Option<&str>, contenu: impl FnOnce(&mut Ui) -> R) -> R {
    let reponse = Frame::NONE
        .fill(theme::fond_panneau())
        .stroke(Stroke::new(1.0, theme::filet_panneau()))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            // Le panneau prend toute la largeur (sinon il se serre sur son contenu).
            ui.set_min_width(ui.available_width());
            if let Some(t) = titre {
                titre_section(ui, t);
                ui.add_space(2.0);
            }
            contenu(ui)
        });
    // Les losanges sont peints APRÈS le cadre : ils restent visibles par-dessus le filet.
    // On saute le dessin si le panneau est hors de l'écran (listes défilantes).
    if ui.is_rect_visible(reponse.response.rect) {
        for centre in centres_losanges_panneau(reponse.response.rect, 14.0) {
            losange(ui, centre, LOSANGE_PANNEAU / 2.0 + 0.5, theme::OR, Stroke::NONE);
        }
    }
    reponse.inner
}

/// Titre de section (Cinzel or, capitales espacées).
pub fn titre_section(ui: &mut Ui, texte: &str) {
    theme::titre(ui, texte);
}

/// Dimensions d'une étoile : diamètre, pas entre deux étoiles, largeur de la colonne du nombre.
const ETOILE_DIAMETRE: f32 = 13.0;
const ETOILE_PAS: f32 = 16.0;
const ETOILE_COLONNE_NOMBRE: f32 = 30.0;

/// Cinq étoiles alignées reliées par une fine ligne ; `ceil(note / 20)` allumées ; le nombre à droite
/// (aligné à droite dans une colonne fixe). `None` : cinq étoiles éteintes, pas de nombre.
pub fn etoiles(ui: &mut Ui, note: Option<u8>) {
    let largeur = ETOILE_PAS * 5.0 + 6.0 + ETOILE_COLONNE_NOMBRE;
    let (rect, _) = ui.allocate_exact_size(vec2(largeur, ETOILE_DIAMETRE + 3.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let allumees = note.map_or(0, note::etoiles_allumees);
    let couleur = note.map_or(theme::OR, couleur_etoiles);
    let centres = centres_etoiles(rect.left(), rect.center().y, ETOILE_PAS);
    let p = ui.painter();

    // Fine ligne qui relie les étoiles (dessinée d'abord : les étoiles la recouvrent).
    p.line_segment([centres[0], centres[4]], Stroke::new(1.0, avec_alpha(palette::OR, 70)));

    let eteinte_fond = theme::LIGNE;
    let eteinte_trait = Stroke::new(1.0, avec_alpha(palette::OR, 90));
    for (i, centre) in centres.iter().enumerate() {
        let pts = sommets_etoile(*centre, ETOILE_DIAMETRE / 2.0, ETOILE_DIAMETRE / 4.6);
        if (i as u8) < allumees {
            p.add(polygone_etoile(*centre, &pts, couleur));
        } else {
            // Éteinte : remplissage sombre OPAQUE (cache la ligne) + contour or très discret.
            p.add(polygone_etoile(*centre, &pts, eteinte_fond));
            p.add(Shape::closed_line(pts.to_vec(), eteinte_trait));
        }
    }
    if let Some(n) = note {
        p.text(
            Pos2::new(rect.right(), rect.center().y),
            Align2::RIGHT_CENTER,
            n.to_string(),
            FontId::proportional(13.0),
            couleur,
        );
    }
}

/// Portrait de champion : carré 38 px (66 px pour la fiche), bordure or, niveau en bas à droite.
/// Si l'image n'est pas encore chargée (ou hors ligne) : carré or sombre avec l'initiale.
pub fn portrait(
    ui: &mut Ui,
    p: &mut Portraits,
    id_dd: Option<&str>,
    niveau: Option<u8>,
    t: TaillePortrait,
) -> Response {
    let cote = t.cote();
    let reponse = ui.allocate_response(vec2(cote, cote), Sense::hover());
    let rect = reponse.rect;
    // Hors de l'écran : on ne touche pas au cache de textures (il garde ~10 images au plus).
    if !ui.is_rect_visible(rect) {
        return reponse;
    }
    let painter = ui.painter();
    let texture = id_dd.and_then(|id| p.texture(id, t.icone()));
    match texture {
        Some(tex) => {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(tex, rect, uv, Color32::WHITE);
        }
        None => {
            painter.rect_filled(rect, CornerRadius::ZERO, avec_alpha(palette::OR_BARRE_DEBUT, 90));
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                initiale(id_dd),
                theme::police_titre(cote * 0.42),
                theme::OR,
            );
        }
    }
    // Bordure DANS le carré (elle ne déborde pas sur les voisins).
    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(t.bordure(), theme::OR), StrokeKind::Inside);

    if let Some(n) = niveau {
        let galley = painter.layout_no_wrap(n.to_string(), FontId::proportional(t.texte_niveau()), theme::TEXTE);
        let cadre = rect_niveau(rect, galley.size());
        painter.rect_filled(cadre, CornerRadius::ZERO, theme::FOND);
        painter.rect_stroke(cadre, CornerRadius::ZERO, Stroke::new(1.0, theme::OR), StrokeKind::Inside);
        painter.galley(cadre.center() - galley.size() / 2.0, galley, theme::TEXTE);
    }
    reponse
}

/// Emblème de rang : losange à double contour or avec l'initiale du rang (« ? » sans rang).
/// `cote` : largeur et hauteur du losange (58 px sur l'accueil).
pub fn embleme_rang(ui: &mut Ui, rang: Option<&Rang>, cote: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(cote, cote), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let demi = cote / 2.0 - 1.0;
    losange(ui, rect.center(), demi, theme::fond_panneau(), Stroke::new(1.5, theme::OR));
    // Deuxième contour, plus fin et plus petit : le « double contour ».
    losange(ui, rect.center(), demi * 0.82, Color32::TRANSPARENT, Stroke::new(1.0, avec_alpha(palette::OR, 150)));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initiale(rang.map(|r| &*r.palier)),
        theme::police_titre(cote * 0.34),
        theme::OR,
    );
}

/// Barre d'onglets : texte or + soulignement or de 2 px pour l'actif, texte secondaire pour les
/// autres. `visibles` donne l'ordre et les onglets montrés. Renvoie true si l'onglet actif a changé.
pub fn onglets(ui: &mut Ui, actif: &mut Onglet, visibles: &[Onglet]) -> bool {
    let mut change = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        for &o in visibles {
            // On mesure le texte d'abord (couleur PLACEHOLDER : choisie au moment de peindre).
            let galley = ui.painter().layout_no_wrap(
                o.libelle().to_string(),
                FontId::proportional(14.0),
                Color32::PLACEHOLDER,
            );
            let taille = galley.size() + vec2(2.0, 12.0);
            let (rect, reponse) = ui.allocate_exact_size(taille, Sense::click());
            let est_actif = *actif == o;
            if ui.is_rect_visible(rect) {
                let couleur = if est_actif {
                    theme::OR
                } else if reponse.hovered() {
                    theme::TEXTE
                } else {
                    theme::TEXTE_SECONDAIRE
                };
                let pos = Pos2::new(rect.left() + 1.0, rect.center().y - galley.size().y / 2.0 - 1.0);
                ui.painter().galley(pos, galley, couleur);
                if est_actif {
                    ui.painter().rect_filled(rect_soulignement(rect), CornerRadius::ZERO, theme::OR);
                }
            }
            if reponse.clicked() && !est_actif {
                *actif = o;
                change = true;
            }
        }
    });
    // Filet fin sous toute la barre.
    let y = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, theme::LIGNE));
    change
}

/// Hauteur d'une barre de progression.
const BARRE_HAUTEUR: f32 = 8.0;

/// Barre de progression : piste sombre, dégradé #785A28 -> #C8AA6E. `fraction` de 0 à 1.
pub fn barre(ui: &mut Ui, fraction: f32, largeur: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(largeur, BARRE_HAUTEUR), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, theme::LIGNE_RANGEE);
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, theme::LIGNE), StrokeKind::Inside);
    let w = largeur_remplie(fraction, largeur);
    if w >= 1.0 {
        // Quadrilatère à deux couleurs : egui interpole le dégradé entre les sommets.
        let rempli = Rect::from_min_size(rect.min, vec2(w, rect.height()));
        let (a, b) = (theme::OR_SOMBRE, theme::OR);
        let mut m = Mesh::default();
        m.colored_vertex(rempli.left_top(), a);
        m.colored_vertex(rempli.right_top(), b);
        m.colored_vertex(rempli.right_bottom(), b);
        m.colored_vertex(rempli.left_bottom(), a);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        p.add(Shape::mesh(m));
    }
}

/// Pastille V / D d'une partie : petit carré (angles droits), vert pour la victoire, rouge sinon.
pub fn pastille_victoire(ui: &mut Ui, victoire: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let (couleur, lettre) = if victoire { (theme::VICTOIRE, "V") } else { (theme::DEFAITE, "D") };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, couleur.gamma_multiply(0.16));
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, couleur.gamma_multiply(0.7)), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, lettre, FontId::proportional(12.0), couleur);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn couleur_selon_la_note() {
        assert_eq!(couleur_etoiles(0), theme::OR);
        assert_eq!(couleur_etoiles(89), theme::OR);
        assert_eq!(couleur_etoiles(90), theme::ACCENT);
        assert_eq!(couleur_etoiles(100), theme::ACCENT);
        // Une note n'a jamais d'autre couleur que ces deux ors.
        for n in 0..=255u8 {
            let c = couleur_etoiles(n);
            assert!(c == theme::OR || c == theme::ACCENT);
        }
    }

    #[test]
    fn losange_symetrique() {
        let s = sommets_losange(Pos2::new(10.0, 20.0), 3.0, 4.0);
        assert_eq!(s[0], Pos2::new(10.0, 16.0));
        assert_eq!(s[1], Pos2::new(13.0, 20.0));
        assert_eq!(s[2], Pos2::new(10.0, 24.0));
        assert_eq!(s[3], Pos2::new(7.0, 20.0));
    }

    #[test]
    fn losanges_sur_le_bord_haut() {
        let r = Rect::from_min_max(Pos2::new(0.0, 5.0), Pos2::new(100.0, 50.0));
        let [g, d] = centres_losanges_panneau(r, 14.0);
        assert_eq!(g, Pos2::new(14.0, 5.0));
        assert_eq!(d, Pos2::new(86.0, 5.0));
    }

    #[test]
    fn etoile_pointe_en_haut_et_rayons() {
        let c = Pos2::new(50.0, 50.0);
        let s = sommets_etoile(c, 10.0, 4.0);
        assert!((s[0].x - 50.0).abs() < 1e-4 && (s[0].y - 40.0).abs() < 1e-4);
        for (i, p) in s.iter().enumerate() {
            let attendu = if i % 2 == 0 { 10.0 } else { 4.0 };
            assert!(((*p - c).length() - attendu).abs() < 1e-3, "sommet {i}");
        }
    }

    #[test]
    fn etoiles_alignees_et_regulieres() {
        let c = centres_etoiles(10.0, 7.0, 16.0);
        assert!(c.iter().all(|p| p.y == 7.0));
        assert_eq!(c[0].x, 18.0);
        for i in 1..5 {
            assert!((c[i].x - c[i - 1].x - 16.0).abs() < 1e-4);
        }
    }

    #[test]
    fn barre_remplie() {
        assert_eq!(largeur_remplie(0.5, 100.0), 50.0);
        assert_eq!(largeur_remplie(-1.0, 100.0), 0.0);
        assert_eq!(largeur_remplie(7.0, 100.0), 100.0);
        assert_eq!(largeur_remplie(f32::NAN, 100.0), 0.0);
    }

    #[test]
    fn soulignement_de_2_px_en_bas() {
        let r = Rect::from_min_max(Pos2::new(10.0, 0.0), Pos2::new(60.0, 30.0));
        let s = rect_soulignement(r);
        assert_eq!(s.height(), 2.0);
        assert_eq!(s.bottom(), 30.0);
        assert_eq!(s.width(), 50.0);
    }

    #[test]
    fn niveau_en_bas_a_droite() {
        let p = Rect::from_min_size(Pos2::ZERO, vec2(38.0, 38.0));
        let n = rect_niveau(p, vec2(8.0, 10.0));
        assert!(n.center().x > 19.0 && n.center().y > 19.0);
        // Déborde d'1 px au plus.
        assert!(n.right() <= 39.0 && n.bottom() <= 39.0);
    }

    #[test]
    fn initiales() {
        assert_eq!(initiale(Some("GOLD")), "G");
        assert_eq!(initiale(Some("ahri")), "A");
        assert_eq!(initiale(Some("")), "?");
        assert_eq!(initiale(None), "?");
    }

    #[test]
    fn tailles_de_portrait() {
        assert_eq!(TaillePortrait::Ligne.cote(), 38.0);
        assert_eq!(TaillePortrait::Fiche.cote(), 66.0);
        assert_eq!(TaillePortrait::Ligne.bordure(), 1.0);
        assert_eq!(TaillePortrait::Fiche.bordure(), 2.0);
    }

    /// Dessine tous les composants dans un contexte egui sans fenêtre : vérifie l'absence de panique
    /// (police « titre » déclarée, géométrie, texture absente...). Deux trames : la 2e peint pour de bon.
    #[test]
    fn dessin_sans_panique() {
        let ctx = eframe::egui::Context::default();
        theme::appliquer(&ctx);
        let mut p = Portraits::nouveau(&ctx, None);
        let rang = Rang { palier: "GOLD".into(), division: "II".into(), lp: 40, victoires: 10, defaites: 8, serie: false };
        let mut actif = Onglet::Partie;
        for _ in 0..2 {
            let mut sortie = ctx.run_ui(Default::default(), |ui| {
                panneau(ui, Some("Mon compte"), |ui| {
                    for n in [None, Some(0), Some(55), Some(90), Some(100)] {
                        etoiles(ui, n);
                    }
                    portrait(ui, &mut p, Some("Ahri"), Some(17), TaillePortrait::Ligne);
                    portrait(ui, &mut p, None, None, TaillePortrait::Fiche);
                    embleme_rang(ui, Some(&rang), 58.0);
                    embleme_rang(ui, None, 58.0);
                    barre(ui, 0.6, 140.0);
                    barre(ui, f32::NAN, 140.0);
                    pastille_victoire(ui, true);
                    pastille_victoire(ui, false);
                    onglets(ui, &mut actif, &Onglet::TOUS);
                });
            });
            // egui exige que les textures de la trame soient « traitées » : on les jette volontairement.
            sortie.textures_delta.clear();
        }
    }
}
