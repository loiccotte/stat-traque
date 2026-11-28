//! Thème « D, étoilé minimal, version LoL » (voir docs/DESIGN.md).
//!
//! Les couleurs ne sont PAS définies ici : elles viennent toutes de `palette.rs`
//! (une seule source, sans egui, partagée avec l'overlay natif). Ce fichier ne fait
//! que les convertir en `Color32` et régler le style d'egui.
//!
//! Choix importants :
//!  - angles droits partout (`CornerRadius::ZERO`), aucune ombre ;
//!  - aucune animation (`animation_time = 0`) : rien ne redessine tout seul, donc 0 % CPU au repos ;
//!  - PAS de `override_text_color` : voir `appliquer` (c'était la cause du gras illisible).

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke,
};

use super::palette::{self, Rgb};

/// Convertit un jeton de la palette en couleur egui (opaque). `const` : utilisable dans les constantes.
pub const fn c(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Couleur de la palette avec une opacité (0 à 255), pour les filets et fonds translucides.
pub fn avec_alpha(rgb: Rgb, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], alpha)
}

// --- Jetons (noms conservés pour les écrans existants) ---------------------------------------

/// Fond de la fenêtre.
pub const FOND: Color32 = c(palette::FOND);
/// Fond des panneaux, opaque (fenêtres, menus). Les panneaux du thème sont translucides : voir `fond_panneau`.
pub const CADRE: Color32 = c(palette::PANNEAU);
/// Or : titres, filets, portraits.
pub const OR: Color32 = c(palette::OR);
/// Or sombre : début du dégradé des barres, bordures discrètes.
pub const OR_SOMBRE: Color32 = c(palette::OR_BARRE_DEBUT);
/// Or clair : notes S et accents (le seul « vif » du thème).
pub const ACCENT: Color32 = c(palette::ACCENT);
/// Texte principal.
pub const TEXTE: Color32 = c(palette::TEXTE);
/// Texte secondaire (contraste >= 4,5:1, ne jamais plus sombre).
pub const TEXTE_SECONDAIRE: Color32 = c(palette::TEXTE_SECONDAIRE);
/// Ancien nom de `TEXTE_SECONDAIRE`, gardé pour les écrans pas encore migrés.
pub const TEXTE_DOUX: Color32 = TEXTE_SECONDAIRE;
/// Ligne / séparateur.
pub const LIGNE: Color32 = c(palette::LIGNE);
/// Séparateur fin entre deux rangées.
pub const LIGNE_RANGEE: Color32 = c(palette::LIGNE_RANGEE);
pub const VICTOIRE: Color32 = c(palette::VICTOIRE);
pub const DEFAITE: Color32 = c(palette::DEFAITE);
/// Équipes seulement, jamais pour une note.
pub const EQUIPE_BLEUE: Color32 = c(palette::EQUIPE_BLEUE);
pub const EQUIPE_ROUGE: Color32 = c(palette::EQUIPE_ROUGE);

// Anciens noms, conservés TEMPORAIREMENT pour que les écrans non migrés compilent
// (ils sont réécrits par les tâches suivantes) : plus aucune couleur propre.
/// Ancien turquoise : supprimé du thème, remplacé par l'or clair.
pub const TURQUOISE: Color32 = ACCENT;
/// Ancien vert : devient le vert « victoire » de la palette.
pub const VERT: Color32 = VICTOIRE;
/// Ancien orange d'alerte : devient l'or clair.
pub const ALERTE: Color32 = ACCENT;
/// Barres AP / AD du radar (données, pas du décor) : inchangées.
pub const AP: Color32 = Color32::from_rgb(110, 120, 255);
pub const AD: Color32 = Color32::from_rgb(235, 140, 50);

/// Nom de la famille de police des titres (Cinzel).
pub const FAMILLE_TITRE: &str = "titre";

/// Police des titres : Cinzel si elle est chargée, sinon (repli automatique) la police par défaut.
pub fn police_titre(taille: f32) -> FontId {
    FontId::new(taille, FontFamily::Name(FAMILLE_TITRE.into()))
}

/// Fond d'un panneau : #0B1428 à ~67 %.
pub fn fond_panneau() -> Color32 {
    avec_alpha(palette::PANNEAU, palette::PANNEAU_ALPHA)
}

/// Filet d'un panneau : or à ~19 %.
pub fn filet_panneau() -> Color32 {
    avec_alpha(palette::OR, palette::FILET_ALPHA)
}

// --- Polices ---------------------------------------------------------------------------------

/// Sous-ensemble de Cinzel (graisse 600, latin + accents français), ~25 Ko. Licence : assets/OFL-Cinzel.txt.
static CINZEL: &[u8] = include_bytes!("../../assets/Cinzel-sub.ttf");

/// Le fichier ressemble-t-il à une police TrueType / OpenType ? (garde-fou : une police
/// corrompue ne doit jamais faire planter l'interface, on retombe sur la police par défaut).
fn police_plausible(octets: &[u8]) -> bool {
    octets.len() > 1024
        && matches!(&octets[..4], [0, 1, 0, 0] | b"OTTO" | b"true")
}

/// Familles de repli de la famille « titre » : la police proportionnelle par défaut d'egui.
fn polices_titre(defs: &FontDefinitions, avec_cinzel: bool) -> Vec<String> {
    let mut liste = Vec::new();
    if avec_cinzel {
        liste.push("Cinzel".to_string());
    }
    if let Some(base) = defs.families.get(&FontFamily::Proportional) {
        liste.extend(base.iter().cloned());
    }
    liste
}

/// Enregistre la famille `FontFamily::Name("titre")`. Elle existe TOUJOURS (egui panique si
/// on utilise une famille nommée non déclarée) : sans Cinzel, c'est la police par défaut.
fn installer_polices(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let cinzel_ok = police_plausible(CINZEL);
    if cinzel_ok {
        defs.font_data
            .insert("Cinzel".to_string(), Arc::new(FontData::from_static(CINZEL)));
    }
    let liste = polices_titre(&defs, cinzel_ok);
    defs.families.insert(FontFamily::Name(FAMILLE_TITRE.into()), liste);
    ctx.set_fonts(defs);
}

// --- Style -----------------------------------------------------------------------------------

/// Construit les `Visuals` du thème (séparé de `appliquer` pour pouvoir le tester).
pub fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();

    // Fonds.
    v.panel_fill = FOND;
    v.window_fill = CADRE;
    v.window_stroke = Stroke::new(1.0, filet_panneau());
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    // Pistes des barres et champs de saisie : sombres (plus de piste blanche).
    v.extreme_bg_color = FOND;
    v.faint_bg_color = LIGNE_RANGEE;

    // TEXTE. Cause du gras illisible : `RichText::strong()` prend la couleur
    // `widgets.active.fg_stroke` (egui, `Visuals::strong_text_color`), pas `override_text_color`.
    // Forcer `override_text_color` laissait donc `active.fg_stroke` à sa valeur d'origine d'egui.
    // Correctif : pas d'override, et `fg_stroke` de CHAQUE état fixé sur le texte du thème
    // (le gras et le texte normal restent lisibles quel que soit l'état du widget).
    v.override_text_color = None;
    v.weak_text_color = Some(TEXTE_SECONDAIRE); // `.weak()` : contraste >= 4,5:1 (sinon 50 % de transparence)
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = ACCENT;
    v.error_fg_color = DEFAITE;

    // Sélection : fond sombre et translucide (plus de bleu clair), trait or.
    v.selection.bg_fill = avec_alpha(palette::OR, 40);
    v.selection.stroke = Stroke::new(1.0, OR);

    // Widgets : angles droits, texte du thème, bordures or discrètes.
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = CADRE;
    w.noninteractive.weak_bg_fill = CADRE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, LIGNE); // sert aussi aux séparateurs
    w.inactive.weak_bg_fill = LIGNE_RANGEE;
    w.inactive.bg_fill = LIGNE_RANGEE;
    w.inactive.bg_stroke = Stroke::new(1.0, OR_SOMBRE);
    w.hovered.weak_bg_fill = LIGNE;
    w.hovered.bg_fill = LIGNE;
    w.hovered.bg_stroke = Stroke::new(1.0, OR);
    w.active.weak_bg_fill = LIGNE;
    w.active.bg_fill = LIGNE;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.open.weak_bg_fill = LIGNE_RANGEE;
    w.open.bg_fill = LIGNE_RANGEE;
    w.open.bg_stroke = Stroke::new(1.0, OR_SOMBRE);
    for etat in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        etat.corner_radius = CornerRadius::ZERO;
        etat.fg_stroke = Stroke::new(1.0, TEXTE);
        etat.expansion = 0.0;
    }
    v
}

/// Applique le thème à tout le contexte egui (une seule fois, à l'ouverture).
pub fn appliquer(ctx: &egui::Context) {
    // Toujours le thème sombre, même si Windows est en mode clair (sinon egui
    // basculerait sur ses visuels clairs : texte noir sur fond sombre).
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals());
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 5.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        // Pas d'animation : chaque animation demande des images en plus (CPU).
        s.animation_time = 0.0;
    });
    installer_polices(ctx);
}

/// Cadre « carte » (ancien nom, conservé pour les écrans non migrés) : même apparence
/// qu'un panneau, angles droits. Les nouveaux écrans utilisent `composants::panneau`.
pub fn carte() -> egui::Frame {
    egui::Frame::NONE
        .fill(fond_panneau())
        .stroke(Stroke::new(1.0, filet_panneau()))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(egui::Margin::same(10))
}

/// Titre de section : Cinzel or, en capitales espacées.
pub fn titre(ui: &mut egui::Ui, texte: &str) {
    ui.label(
        egui::RichText::new(texte.to_uppercase())
            .font(police_titre(13.0))
            .extra_letter_spacing(1.5)
            .color(OR),
    );
}

/// Couleur d'un palier (Gold, Diamond...).
pub fn couleur_palier(palier: &str) -> Color32 {
    let [r, g, b] = super::format::couleur_palier(palier);
    Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_palette() {
        assert_eq!(c([0xC8, 0xAA, 0x6E]), OR);
        assert_eq!(OR, Color32::from_rgb(200, 170, 110));
    }

    #[test]
    fn pas_d_override_et_gras_lisible() {
        let v = visuals();
        assert!(v.override_text_color.is_none());
        // Le gras prend `widgets.active.fg_stroke` : il doit être le texte du thème.
        assert_eq!(v.strong_text_color(), TEXTE);
        for e in [v.widgets.noninteractive, v.widgets.inactive, v.widgets.hovered, v.widgets.active, v.widgets.open] {
            assert_eq!(e.fg_stroke.color, TEXTE);
            assert_eq!(e.corner_radius, CornerRadius::ZERO);
        }
    }

    #[test]
    fn selection_sombre() {
        // Pas de fond clair : opacité faible.
        assert!(visuals().selection.bg_fill.a() < 80);
    }

    #[test]
    fn police_incluse_valide() {
        assert!(police_plausible(CINZEL));
        assert!(!police_plausible(&[]));
        assert!(!police_plausible(&[0u8; 4096]));
    }

    #[test]
    fn famille_titre_toujours_declaree() {
        let defs = FontDefinitions::default();
        // Sans Cinzel : uniquement le repli ; avec : Cinzel en premier.
        assert!(!polices_titre(&defs, false).iter().any(|n| n == "Cinzel"));
        assert_eq!(polices_titre(&defs, true)[0], "Cinzel");
        assert!(!polices_titre(&defs, false).is_empty());
    }
}
