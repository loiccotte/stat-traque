//! Jetons de couleur du thème « D, étoilé minimal » (docs/DESIGN.md).
//!
//! Fichier PUR : des `[u8; 3]` (rouge, vert, bleu), SANS egui, donc testable
//! partout et utilisable par l'overlay natif (GDI) comme par la fenêtre egui.

/// Une couleur : [rouge, vert, bleu], 0 à 255.
pub type Rgb = [u8; 3];

pub const FOND: Rgb = [0x05, 0x06, 0x0C];
/// Halo bleu nuit en haut de la fenêtre (dégradé radial dessiné une fois).
pub const FOND_HALO: Rgb = [0x10, 0x21, 0x3C];
pub const TEXTE: Rgb = [0xEC, 0xE8, 0xDC];
/// Texte secondaire : ne jamais descendre plus sombre (contraste >= 4,5:1).
pub const TEXTE_SECONDAIRE: Rgb = [0xA3, 0x9F, 0x94];
/// Or clair : notes S (>= 90) et accents.
pub const ACCENT: Rgb = [0xF0, 0xC8, 0x70];
/// Or : filets, titres, portraits.
pub const OR: Rgb = [0xC8, 0xAA, 0x6E];
/// Début du dégradé de barre de progression (la fin est `OR`).
pub const OR_BARRE_DEBUT: Rgb = [0x78, 0x5A, 0x28];
pub const LIGNE: Rgb = [0x1D, 0x22, 0x38];
/// Séparateur fin entre deux rangées.
pub const LIGNE_RANGEE: Rgb = [0x15, 0x1A, 0x2E];
pub const VICTOIRE: Rgb = [0x7F, 0xD8, 0xA8];
pub const DEFAITE: Rgb = [0xE8, 0x8A, 0x7A];
/// Équipes seulement, jamais pour les notes.
pub const EQUIPE_BLEUE: Rgb = [0x8F, 0xB4, 0xFF];
pub const EQUIPE_ROUGE: Rgb = [0xFF, 0x9D, 0x7A];
/// Fond des panneaux (rectangles à angles droits).
pub const PANNEAU: Rgb = [0x0B, 0x14, 0x28];

// Opacités (0 à 255) : panneau 67 %, filet or 19 %, ma ligne (fond or) 8 %.
pub const PANNEAU_ALPHA: u8 = 171;
pub const FILET_ALPHA: u8 = 48;
pub const MA_LIGNE_ALPHA: u8 = 20;

/// Mélange `dessus` (opacité `alpha`) sur `dessous` : la couleur réellement vue.
pub fn melanger(dessus: Rgb, alpha: u8, dessous: Rgb) -> Rgb {
    let a = alpha as u32;
    let mut sortie = [0u8; 3];
    for i in 0..3 {
        sortie[i] = ((dessus[i] as u32 * a + dessous[i] as u32 * (255 - a) + 127) / 255) as u8;
    }
    sortie
}

/// Luminance relative WCAG d'une couleur sRGB (0 = noir, 1 = blanc).
fn luminance(c: Rgb) -> f32 {
    let lineaire = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lineaire(c[0]) + 0.7152 * lineaire(c[1]) + 0.0722 * lineaire(c[2])
}

/// Rapport de contraste WCAG entre deux couleurs (de 1 à 21).
pub fn contraste(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (clair, sombre) = if la >= lb { (la, lb) } else { (lb, la) };
    (clair + 0.05) / (sombre + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contraste_de_reference() {
        assert!((contraste([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.01);
        assert!((contraste(TEXTE, TEXTE) - 1.0).abs() < 0.001);
    }

    #[test]
    fn texte_sur_fond() {
        assert!(contraste(TEXTE, FOND) >= 7.0);
    }

    #[test]
    fn texte_secondaire_sur_fond() {
        assert!(contraste(TEXTE_SECONDAIRE, FOND) >= 4.5);
    }

    #[test]
    fn texte_sur_panneau() {
        let panneau = melanger(PANNEAU, PANNEAU_ALPHA, FOND);
        assert!(contraste(TEXTE, panneau) >= 7.0);
        assert!(contraste(TEXTE_SECONDAIRE, panneau) >= 4.5);
    }
}
