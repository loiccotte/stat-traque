//! Thème sombre façon client LoL : bleu nuit + or. Police par défaut d'egui.

use eframe::egui::{self, Color32, CornerRadius, Stroke};

/// Fond de la fenêtre (bleu nuit presque noir).
pub const FOND: Color32 = Color32::from_rgb(1, 10, 19);
/// Fond des cadres (un peu plus clair).
pub const CADRE: Color32 = Color32::from_rgb(10, 20, 40);
/// Or « hextech » : titres, bordures.
pub const OR: Color32 = Color32::from_rgb(200, 170, 110);
/// Or sombre : bordures discrètes.
pub const OR_SOMBRE: Color32 = Color32::from_rgb(120, 90, 40);
/// Texte principal (beige clair).
pub const TEXTE: Color32 = Color32::from_rgb(240, 230, 210);
/// Texte secondaire.
pub const TEXTE_DOUX: Color32 = Color32::from_rgb(160, 155, 140);
/// Turquoise du client (boutons, sélection).
pub const TURQUOISE: Color32 = Color32::from_rgb(10, 200, 185);
pub const EQUIPE_BLEUE: Color32 = Color32::from_rgb(80, 150, 240);
pub const EQUIPE_ROUGE: Color32 = Color32::from_rgb(232, 64, 87);
/// Barre AP (bleu-violet) et AD (orange), comme sur les sites de stats.
pub const AP: Color32 = Color32::from_rgb(110, 120, 255);
pub const AD: Color32 = Color32::from_rgb(235, 140, 50);
pub const ALERTE: Color32 = Color32::from_rgb(240, 170, 60);
pub const VERT: Color32 = Color32::from_rgb(70, 200, 120);

/// Applique le thème à tout le contexte egui (une seule fois, à l'ouverture).
pub fn appliquer(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = FOND;
    v.window_fill = CADRE;
    v.window_stroke = Stroke::new(1.0, OR_SOMBRE);
    v.extreme_bg_color = Color32::from_rgb(0, 5, 12);
    v.faint_bg_color = Color32::from_rgb(8, 18, 34);
    v.override_text_color = Some(TEXTE);
    v.hyperlink_color = TURQUOISE;
    v.warn_fg_color = ALERTE;
    v.selection.bg_fill = Color32::from_rgb(20, 70, 90);
    v.selection.stroke = Stroke::new(1.0, TURQUOISE);
    let arrondi = CornerRadius::same(2);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = arrondi;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, OR_SOMBRE);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(16, 30, 50);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, OR_SOMBRE);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(24, 44, 70);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, OR);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(30, 60, 90);
    v.widgets.active.bg_stroke = Stroke::new(1.0, TURQUOISE);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 5.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        // Pas d'animation : chaque animation demande des images en plus (CPU).
        s.animation_time = 0.0;
    });
}

/// Cadre « carte » : fond légèrement plus clair, bordure or sombre.
pub fn carte() -> egui::Frame {
    egui::Frame::NONE
        .fill(CADRE)
        .stroke(Stroke::new(1.0, OR_SOMBRE))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(egui::Margin::same(10))
}

/// Titre de section en or.
pub fn titre(ui: &mut egui::Ui, texte: &str) {
    ui.label(egui::RichText::new(texte).color(OR).strong().size(15.0));
}

/// Couleur d'un palier (Gold, Diamond...).
pub fn couleur_palier(palier: &str) -> Color32 {
    let [r, g, b] = super::format::couleur_palier(palier);
    Color32::from_rgb(r, g, b)
}
